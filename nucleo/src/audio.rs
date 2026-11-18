//! Audio: lista de dispositivos, micro con cpal y lectura de WAV, todo a 16 kHz mono.

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{InterfaceType, SampleFormat};
use crossbeam_channel::Sender;

/// Lo que pide Whisper.
pub const FRECUENCIA: u32 = 16_000;
/// 30 ms de audio: la unidad con la que se decide si hay voz.
pub const TRAMA: usize = (FRECUENCIA as usize) * 30 / 1000;

#[derive(Clone, Debug, PartialEq)]
pub struct Dispositivo {
    pub nombre: String,
    pub predeterminado: bool,
    /// Altavoces o micro del propio Mac (no cascos ni USB ni Bluetooth).
    pub integrado: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dispositivos {
    pub entradas: Vec<Dispositivo>,
    pub salidas: Vec<Dispositivo>,
}

pub fn dispositivos() -> Dispositivos {
    let host = cpal::default_host();
    let nombre = |d: &cpal::Device| d.description().ok().map(|x| x.name().to_owned());
    let defecto_in = host.default_input_device().as_ref().and_then(nombre);
    let defecto_out = host.default_output_device().as_ref().and_then(nombre);
    let lista = |devs: Option<Vec<cpal::Device>>, defecto: &Option<String>| -> Vec<Dispositivo> {
        let mut vistos = Vec::new();
        for d in devs.unwrap_or_default() {
            let Ok(desc) = d.description() else { continue };
            let n = desc.name().to_owned();
            if vistos.iter().any(|v: &Dispositivo| v.nombre == n) {
                continue;
            }
            vistos.push(Dispositivo {
                predeterminado: defecto.as_deref() == Some(n.as_str()),
                integrado: desc.interface_type() == InterfaceType::BuiltIn || parece_integrado(&n),
                nombre: n,
            });
        }
        vistos
    };
    Dispositivos {
        entradas: lista(host.input_devices().ok().map(Iterator::collect), &defecto_in),
        salidas: lista(host.output_devices().ok().map(Iterator::collect), &defecto_out),
    }
}

/// cpal no siempre sabe el tipo de conexión en macOS: se mira el nombre que pone Apple a
/// los altavoces y micros del propio Mac ("MacBook Pro Speakers", "iMac Microphone"…).
fn parece_integrado(nombre: &str) -> bool {
    let n = nombre.to_lowercase();
    ["macbook", "imac", "mac mini", "mac studio", "mac pro", "built-in", "integrad"].iter().any(|m| n.contains(m))
}

/// Lo que llega del micro (o del fichero que hace de micro).
pub enum Audio {
    /// 30 ms a 16 kHz mono.
    Trama(Vec<f32>),
    /// Se ha cortado: el dispositivo ha desaparecido o el fichero se ha acabado.
    Fin(Option<String>),
}

/// Abre el micro y manda tramas de 30 ms por `salida`. El flujo para al soltar el valor
/// devuelto (en macOS no se puede pasar a otro hilo: se queda en el que lo abre).
pub fn abrir_micro(nombre: Option<&str>, salida: Sender<Audio>) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    let dispositivo = match nombre {
        Some(n) => host
            .input_devices()?
            .find(|d| d.description().map(|x| x.name() == n).unwrap_or(false))
            .ok_or_else(|| anyhow!("No encuentro el micro «{n}». ¿Sigue conectado?"))?,
        None => host.default_input_device().context("No hay ningún micrófono")?,
    };
    let config = dispositivo.default_input_config().context("El micro no da su configuración")?;
    let canales = config.channels() as usize;
    let frecuencia = config.sample_rate();
    let formato = config.sample_format();
    let config: cpal::StreamConfig = config.into();

    let mut troceo = Troceo::new(frecuencia, canales, salida.clone());
    let aviso = salida.clone();
    // Un desbordamiento puntual del búfer (el Mac va cargado) se pierde unos ms de audio y
    // sigue; solo se para si el micro desaparece o se queda sin permiso.
    let error = move |e: cpal::Error| {
        use cpal::ErrorKind::*;
        if matches!(e.kind(), DeviceNotAvailable | PermissionDenied | StreamInvalidated | HostUnavailable) {
            let _ = aviso.send(Audio::Fin(Some(format!("Se ha cortado el micro: {e}"))));
        }
    };
    let flujo = match formato {
        SampleFormat::F32 => dispositivo.build_input_stream::<f32, _, _>(
            config,
            move |datos, _| troceo.meter(datos.iter().copied()),
            error,
            None,
        ),
        SampleFormat::I16 => dispositivo.build_input_stream::<i16, _, _>(
            config,
            move |datos, _| troceo.meter(datos.iter().map(|&m| m as f32 / 32768.0)),
            error,
            None,
        ),
        SampleFormat::I32 => dispositivo.build_input_stream::<i32, _, _>(
            config,
            move |datos, _| troceo.meter(datos.iter().map(|&m| m as f32 / 2_147_483_648.0)),
            error,
            None,
        ),
        otro => bail!("Formato de micro no soportado: {otro:?}"),
    }
    .context("No puedo abrir el micro (¿permiso de micrófono?)")?;
    flujo.play().context("No puedo arrancar el micro")?;
    Ok(flujo)
}

/// Pasa lo que da el dispositivo (cualquier frecuencia y canales) a tramas de 30 ms a
/// 16 kHz mono.
struct Troceo {
    canales: usize,
    remuestreo: Remuestreo,
    mono: Vec<f32>,
    trama: Vec<f32>,
    salida: Sender<Audio>,
    canal_actual: usize,
    suma_canales: f32,
}

impl Troceo {
    fn new(frecuencia: u32, canales: usize, salida: Sender<Audio>) -> Self {
        Self {
            canales: canales.max(1),
            remuestreo: Remuestreo::new(frecuencia, FRECUENCIA),
            mono: Vec::with_capacity(4096),
            trama: Vec::with_capacity(TRAMA),
            salida,
            canal_actual: 0,
            suma_canales: 0.0,
        }
    }

    fn meter(&mut self, muestras: impl Iterator<Item = f32>) {
        self.mono.clear();
        for m in muestras {
            self.suma_canales += m;
            self.canal_actual += 1;
            if self.canal_actual == self.canales {
                self.mono.push(self.suma_canales / self.canales as f32);
                self.canal_actual = 0;
                self.suma_canales = 0.0;
            }
        }
        let mut a_16k = Vec::with_capacity(self.mono.len());
        self.remuestreo.pasar(&self.mono, &mut a_16k);
        for m in a_16k {
            self.trama.push(m);
            if self.trama.len() == TRAMA {
                let llena = std::mem::replace(&mut self.trama, Vec::with_capacity(TRAMA));
                let _ = self.salida.send(Audio::Trama(llena));
            }
        }
    }
}

/// Cambio de frecuencia sencillo: cada muestra de salida es la media de las de entrada
/// que caen en su intervalo (con 48 kHz → 16 kHz, la media de cada tres). Para voz que va
/// a Whisper basta; si la entrada va más lenta que la salida, repite muestras.
pub struct Remuestreo {
    paso: f64,
    limite: f64,
    n: u64,
    suma: f32,
    cuantas: u32,
    ultima: f32,
}

impl Remuestreo {
    pub fn new(de: u32, a: u32) -> Self {
        let paso = de as f64 / a as f64;
        Self { paso, limite: paso, n: 0, suma: 0.0, cuantas: 0, ultima: 0.0 }
    }

    pub fn pasar(&mut self, entrada: &[f32], salida: &mut Vec<f32>) {
        for &m in entrada {
            self.suma += m;
            self.cuantas += 1;
            self.n += 1;
            while self.n as f64 >= self.limite {
                let valor = if self.cuantas > 0 { self.suma / self.cuantas as f32 } else { self.ultima };
                salida.push(valor);
                self.ultima = valor;
                self.suma = 0.0;
                self.cuantas = 0;
                self.limite += self.paso;
            }
        }
    }
}

/// Lee un WAV (cualquier frecuencia, canales y formato PCM) y lo deja a 16 kHz mono.
pub fn leer_wav(ruta: &Path) -> Result<Vec<f32>> {
    let mut lector = hound::WavReader::open(ruta).with_context(|| format!("No puedo leer {}", ruta.display()))?;
    let spec = lector.spec();
    let muestras: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => lector.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let escala = (1u64 << (spec.bits_per_sample - 1)) as f32;
            lector.samples::<i32>().map(|m| m.map(|m| m as f32 / escala)).collect::<Result<_, _>>()?
        }
    };
    let canales = spec.channels.max(1) as usize;
    let mono: Vec<f32> = muestras.chunks(canales).map(|c| c.iter().sum::<f32>() / canales as f32).collect();
    if spec.sample_rate == FRECUENCIA {
        return Ok(mono);
    }
    let mut salida = Vec::with_capacity(mono.len() * FRECUENCIA as usize / spec.sample_rate as usize + 1);
    Remuestreo::new(spec.sample_rate, FRECUENCIA).pasar(&mono, &mut salida);
    Ok(salida)
}

/// Volumen (RMS) de una trama, de 0 a 1.
pub fn volumen(trama: &[f32]) -> f32 {
    if trama.is_empty() {
        return 0.0;
    }
    (trama.iter().map(|m| m * m).sum::<f32>() / trama.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn de_48k_a_16k_saca_un_tercio() {
        let mut r = Remuestreo::new(48_000, 16_000);
        let mut salida = Vec::new();
        r.pasar(&[0.3; 4800], &mut salida);
        assert_eq!(salida.len(), 1600);
        assert!(salida.iter().all(|m| (m - 0.3).abs() < 1e-6));
    }

    #[test]
    fn de_44100_a_16k_no_pierde_ni_inventa_muestras() {
        let mut r = Remuestreo::new(44_100, 16_000);
        let mut salida = Vec::new();
        for _ in 0..10 {
            r.pasar(&[0.0; 4410], &mut salida);
        }
        assert!((salida.len() as i64 - 16_000).abs() <= 1);
    }
}
