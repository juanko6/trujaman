//! Una sesión de escucha: el bucle de `scripts/simultaneo.py`, en su propio hilo.
//!
//! Cada `intervalo` segundos se manda a Whisper todo el audio de la frase en curso; el
//! estabilizador decide qué parte ya es firme y eso va a la pantalla y al locutor. Al
//! callar (`pausa`), se traduce la frase entera y se dice lo que faltaba.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, RecvTimeoutError, Sender};

use crate::audio::{self, Audio, FRECUENCIA, TRAMA};
use crate::estabilizador::{es_inventada, Estabilizador};
use crate::voz::Locutor;
use crate::whisper::{Modelo, Modelos, Peticion};
use crate::Evento;

const SEG: f32 = FRECUENCIA as f32;
/// No se traduce una frase hasta tener esto de audio.
const MIN_AUDIO: f32 = 1.0;
/// Audio anterior al arranque de la voz que se conserva.
const PREVIO: f32 = 0.3;

/// De dónde sale el audio.
pub enum Fuente {
    /// Un micro por su nombre; `None`, el predeterminado.
    Micro(Option<String>),
    /// Audio ya grabado (16 kHz mono), que se pasa a ritmo real como si fuera el micro.
    Muestras(Vec<f32>),
}

#[derive(Clone, Debug)]
pub struct Ajustes {
    /// Al inglés (modelo medium) o solo transcribir (turbo).
    pub traducir: bool,
    /// Con cascos se traduce mientras hablas; con altavoces, frase a frase y con el micro
    /// silenciado mientras habla el locutor, para no oírse a sí mismo.
    pub cascos: bool,
    /// Leer en voz alta lo firme.
    pub hablar: bool,
    pub voz: Option<String>,
    pub idioma: String,
    /// Nombres propios y términos que falla.
    pub vocabulario: String,
    /// Silencio que cierra una frase (s).
    pub pausa: f32,
    /// Volumen mínimo para contar como voz (0..1).
    pub umbral: f32,
    /// Cada cuánto se retraduce la frase en curso (s). Con 0,3 la primera palabra sale
    /// 0,4 s antes que con el 0,5 de la versión en Python y el texto es el mismo; a cambio
    /// Whisper trabaja casi sin parar mientras hablas (una pasada de medium tarda unos
    /// 0,3 s en un M3 Max). En un Mac más lento las pasadas simplemente van seguidas.
    pub intervalo: f32,
    /// Palabras finales que no se dan por firmes.
    pub reserva: usize,
    /// Palabras mínimas para decir un trozo sin puntuación.
    pub min_trozo: usize,
    /// Una frase más larga se corta aquí (s).
    pub max_frase: f32,
}

impl Default for Ajustes {
    fn default() -> Self {
        Self {
            traducir: true,
            cascos: false,
            hablar: true,
            voz: None,
            idioma: "es".into(),
            vocabulario: String::new(),
            pausa: 0.5,
            umbral: 0.015,
            intervalo: 0.3,
            reserva: 2,
            min_trozo: 3,
            max_frase: 20.0,
        }
    }
}

impl Ajustes {
    /// Con altavoces el micro se silencia mientras habla el locutor, así que hablar a la
    /// vez que él no sirve: se va frase a frase.
    pub fn simultaneo(&self) -> bool {
        self.cascos || !self.hablar
    }
}

pub struct Sesion {
    parar: Arc<AtomicBool>,
    hilo: Option<JoinHandle<()>>,
}

impl Sesion {
    /// Arranca la escucha en su hilo. Lo que pase llega por `eventos`; el último siempre
    /// es [`Evento::Terminado`].
    pub fn iniciar(modelos: &Modelos, ajustes: Ajustes, fuente: Fuente, eventos: Sender<Evento>) -> Result<Sesion> {
        let modelo = modelos.para(ajustes.traducir).ok_or_else(|| {
            anyhow!("Falta el modelo para {}", if ajustes.traducir { "traducir" } else { "transcribir" })
        })?;
        let parar = Arc::new(AtomicBool::new(false));
        let bandera = parar.clone();
        let hilo = std::thread::Builder::new().name("sesion".into()).spawn(move || {
            if let Err(e) = escuchar(&modelo, &ajustes, fuente, &eventos, &bandera) {
                let _ = eventos.send(Evento::Aviso(format!("{e:#}")));
            }
            let _ = eventos.send(Evento::Terminado);
        })?;
        Ok(Sesion { parar, hilo: Some(hilo) })
    }

    /// Deja de escuchar y calla al locutor. Espera a que el hilo acabe.
    pub fn parar(mut self) {
        self.parar.store(true, Ordering::Relaxed);
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
    }

    /// Pide que pare, sin esperar a que acabe (para eso, [`Sesion::parar`]).
    pub fn pedir_parada(&self) {
        self.parar.store(true, Ordering::Relaxed);
    }

    pub fn terminada(&self) -> bool {
        self.hilo.as_ref().is_none_or(|h| h.is_finished())
    }
}

impl Drop for Sesion {
    fn drop(&mut self) {
        self.parar.store(true, Ordering::Relaxed);
    }
}

fn escuchar(modelo: &Modelo, a: &Ajustes, fuente: Fuente, eventos: &Sender<Evento>, parar: &AtomicBool) -> Result<()> {
    let enviar = |e: Evento| {
        let _ = eventos.send(e);
    };
    let peticion = |parcial| Peticion {
        idioma: a.idioma.clone(),
        traducir: a.traducir,
        parcial,
        vocabulario: a.vocabulario.clone(),
    };

    let locutor = if a.hablar {
        let l = Locutor::global().map_err(|e| anyhow!(e))?;
        l.elegir_voz(a.voz.clone());
        l.conectar(Some(eventos.clone()));
        Some(l)
    } else {
        None
    };
    let simultaneo = a.simultaneo();

    // Primera pasada en frío, para que no la pague la primera frase.
    modelo.pasada(&vec![0.0; FRECUENCIA as usize], &peticion(true))?;

    let (tx, rx) = unbounded();
    let es_micro = matches!(fuente, Fuente::Micro(_));
    let _flujo = match fuente {
        Fuente::Micro(nombre) => Some(audio::abrir_micro(nombre.as_deref(), tx)?),
        Fuente::Muestras(muestras) => {
            std::thread::spawn(move || pasar_a_ritmo_real(muestras, tx));
            None
        }
    };

    enviar(Evento::Escuchando { simultaneo });
    let t0 = Instant::now();

    let mut estab = Estabilizador::new(a.reserva, a.min_trozo);
    let mut previo: Vec<f32> = Vec::new(); // últimos PREVIO s, por si arranca la voz
    let mut frase: Vec<f32> = Vec::new(); // audio de la frase en curso
    let mut en_frase = false;
    let mut seguidas = 0; // tramas seguidas con voz (para arrancar)
    let mut silencio = 0.0f32; // segundos callado dentro de la frase
    let (mut pico, mut avisado) = (0.0f32, false);
    let mut ultima_pasada = Instant::now();
    let mut audio_en_pasada = 0usize;
    let mut pasadas = 0u32;
    let (mut nivel_max, mut nivel_enviado) = (0.0f32, Instant::now());
    let max_previo = (PREVIO * SEG) as usize;

    let decir = |texto: &str| {
        enviar(Evento::Firme { texto: texto.to_owned(), t: t0.elapsed().as_secs_f32() });
        if let Some(l) = locutor {
            l.decir(texto);
        }
    };

    let cerrar_frase = |frase: &mut Vec<f32>, estab: &mut Estabilizador| -> Result<()> {
        enviar(Evento::Whisper(true));
        let final_ = modelo.pasada(frase, &peticion(false));
        enviar(Evento::Whisper(false));
        let final_ = final_?;
        if let Some(resto) = estab.cierre(&final_) {
            decir(&resto);
        }
        enviar(Evento::FinFrase);
        frase.clear();
        Ok(())
    };

    let resultado = 'bucle: loop {
        if parar.load(Ordering::Relaxed) {
            break Ok(());
        }

        // 1. Recoger todo el audio que haya llegado.
        let mut llegado = Vec::new();
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(x) => {
                llegado.push(x);
                llegado.extend(rx.try_iter());
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => llegado.push(Audio::Fin(None)),
        }

        for pieza in llegado {
            let mut trama = match pieza {
                Audio::Trama(t) => t,
                Audio::Fin(motivo) => {
                    if en_frase {
                        if let Err(e) = cerrar_frase(&mut frase, &mut estab) {
                            break 'bucle Err(e);
                        }
                    }
                    if es_micro {
                        let m = motivo.unwrap_or_else(|| "Se ha cortado el micro.".into());
                        enviar(Evento::Aviso(format!("{m} Revisa que sigue conectado y el permiso de micrófono.")));
                    } else if let Some(l) = locutor {
                        // Fichero: dejar que termine de hablar.
                        while l.ocupado() && !parar.load(Ordering::Relaxed) {
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                    break 'bucle Ok(());
                }
            };
            // No oírse a sí mismo por los altavoces.
            if es_micro && !a.cascos && locutor.is_some_and(|l| l.hablando()) {
                trama.fill(0.0);
            }

            let nivel = audio::volumen(&trama);
            let hay_voz = nivel > a.umbral;
            pico = pico.max(nivel);
            nivel_max = nivel_max.max(nivel);
            if !en_frase {
                previo.extend_from_slice(&trama);
                if previo.len() > max_previo {
                    previo.drain(..previo.len() - max_previo);
                }
                seguidas = if hay_voz { seguidas + 1 } else { 0 };
                if seguidas >= 3 {
                    // 90 ms de voz: empieza una frase.
                    en_frase = true;
                    frase = std::mem::take(&mut previo);
                    silencio = 0.0;
                    ultima_pasada = Instant::now();
                    audio_en_pasada = 0;
                }
            } else {
                frase.extend_from_slice(&trama);
                silencio = if hay_voz { 0.0 } else { silencio + TRAMA as f32 / SEG };
            }
        }

        if nivel_enviado.elapsed() >= Duration::from_millis(100) {
            enviar(Evento::Nivel { v: nivel_max, voz: en_frase });
            nivel_max = 0.0;
            nivel_enviado = Instant::now();
        }

        if !avisado && es_micro && t0.elapsed() > Duration::from_secs(4) {
            avisado = true;
            if pico < 0.0005 {
                enviar(Evento::Aviso(
                    "El micrófono solo entrega silencio. Elige otro en Entrada o revisa el permiso de micrófono \
                     de Trujamán en Ajustes del Sistema → Privacidad y seguridad → Micrófono."
                        .into(),
                ));
            } else if pico < a.umbral {
                enviar(Evento::Aviso("Te oigo muy bajo. Acércate al micrófono o elige otro en Entrada.".into()));
            }
        }

        if !en_frase {
            continue;
        }

        // 2. ¿Se ha acabado la frase?
        let segundos = frase.len() as f32 / SEG;
        if silencio >= a.pausa || segundos >= a.max_frase {
            if let Err(e) = cerrar_frase(&mut frase, &mut estab) {
                break Err(e);
            }
            en_frase = false;
            silencio = 0.0;
            seguidas = 0;
            continue;
        }

        // 3. Pasada parcial: retraducir lo que va de frase y decir lo que ya es firme.
        if !simultaneo || silencio > 0.15 {
            continue; // callado: mejor esperar al cierre
        }
        if ultima_pasada.elapsed().as_secs_f32() < a.intervalo || segundos < MIN_AUDIO {
            continue;
        }
        if frase.len() == audio_en_pasada {
            continue;
        }
        ultima_pasada = Instant::now();
        audio_en_pasada = frase.len();
        enviar(Evento::Whisper(true));
        let actual = modelo.pasada(&frase, &peticion(true));
        enviar(Evento::Whisper(false));
        let actual = match actual {
            Ok(t) => t,
            Err(e) => break Err(e),
        };
        pasadas += 1;
        // Un parcial que es puro ruido no cuenta para nada.
        if estab.dichas() == 0 && es_inventada(&actual) {
            continue;
        }
        let parcial = estab.parcial(&actual);
        if let Some(firme) = &parcial.firme {
            decir(firme);
        }
        enviar(Evento::Provisional {
            texto: parcial.provisional,
            pasadas,
            tarda: ultima_pasada.elapsed().as_secs_f32(),
        });
    };

    if let Some(l) = locutor {
        if parar.load(Ordering::Relaxed) {
            l.callar();
        }
        l.conectar(None);
    }
    resultado
}

/// Hace de micro con audio ya grabado: tramas de 30 ms a ritmo real, y 2 s de silencio
/// al final para que se cierre la última frase.
fn pasar_a_ritmo_real(mut muestras: Vec<f32>, tx: Sender<Audio>) {
    muestras.extend(std::iter::repeat_n(0.0, 2 * FRECUENCIA as usize));
    let inicio = Instant::now();
    for (i, trama) in muestras.chunks(TRAMA).enumerate() {
        if tx.send(Audio::Trama(trama.to_vec())).is_err() {
            return;
        }
        let toca = Duration::from_secs_f64(((i + 1) * TRAMA) as f64 / FRECUENCIA as f64);
        if let Some(falta) = toca.checked_sub(inicio.elapsed()) {
            std::thread::sleep(falta);
        }
    }
    let _ = tx.send(Audio::Fin(None));
}
