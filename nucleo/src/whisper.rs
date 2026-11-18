//! Whisper dentro del proceso (whisper.cpp con Metal), con el modelo cargado una vez por modo.
//!
//! Sustituye a los dos `whisper-server` de la versión en Python: el audio ya no viaja
//! por HTTP ni se empaqueta en WAV, se le pasa a whisper.cpp tal cual.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};

use anyhow::{Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

use crate::estabilizador::limpiar;

/// Turbo transcribe muy bien y rápido, pero no sabe traducir: con la opción de traducir
/// devuelve el español. Para el inglés hace falta medium.
pub const TURBO: &str = "ggml-large-v3-turbo-q5_0.bin";
pub const MEDIUM: &str = "ggml-medium-q5_0.bin";

/// Hilos de CPU para whisper.cpp; con Metal casi todo va en la GPU. Es lo mismo que
/// usa `whisper-server` por defecto.
const HILOS: i32 = 4;

pub struct Modelo {
    nombre: String,
    // El contexto (los pesos) se comparte; el estado (búferes de trabajo) es uno por
    // modelo y se reutiliza en cada pasada para no reservar memoria cada vez.
    _contexto: WhisperContext,
    estado: Mutex<WhisperState>,
}

impl Modelo {
    pub fn cargar(ruta: &Path) -> Result<Self> {
        silenciar_whisper();
        let mut parametros = WhisperContextParameters::default();
        parametros.use_gpu(true).flash_attn(true);
        let ruta_texto = ruta.to_str().context("ruta del modelo no es UTF-8")?;
        let contexto = WhisperContext::new_with_params(ruta_texto, parametros)
            .with_context(|| format!("no puedo cargar el modelo {}", ruta.display()))?;
        let estado = contexto.create_state().context("no puedo preparar el modelo")?;
        Ok(Self {
            nombre: ruta.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            _contexto: contexto,
            estado: Mutex::new(estado),
        })
    }

    pub fn nombre(&self) -> &str {
        &self.nombre
    }

    /// Una pasada sobre `pcm` (16 kHz mono, -1..1). `parcial`: audio a medias, sin
    /// reintentos a más temperatura (más rápido, y un parcial malo se descarta solo).
    pub fn pasada(&self, pcm: &[f32], peticion: &Peticion) -> Result<String> {
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 2 });
        p.set_n_threads(HILOS);
        p.set_language(Some(&peticion.idioma));
        p.set_translate(peticion.traducir);
        p.set_no_context(true);
        p.set_no_timestamps(true);
        p.set_print_special(false);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_timestamps(false);
        p.set_temperature(0.0);
        p.set_temperature_inc(if peticion.parcial { 0.0 } else { 0.2 });
        if !peticion.vocabulario.is_empty() {
            p.set_initial_prompt(&peticion.vocabulario);
        }

        let mut estado = self.estado.lock().unwrap_or_else(|e| e.into_inner());
        estado.full(p, pcm).context("whisper ha fallado")?;
        let mut texto = String::new();
        for segmento in estado.as_iter() {
            if let Ok(t) = segmento.to_str_lossy() {
                texto.push_str(&t);
                texto.push(' ');
            }
        }
        Ok(limpiar(&texto))
    }
}

#[derive(Clone, Debug)]
pub struct Peticion {
    pub idioma: String,
    pub traducir: bool,
    pub parcial: bool,
    pub vocabulario: String,
}

/// Los modelos cargados. La app carga solo el del modo en que está (cada uno ocupa
/// medio giga largo en memoria); la prueba de latencia carga los dos.
pub struct Modelos {
    pub turbo: Option<Arc<Modelo>>,
    pub medium: Option<Arc<Modelo>>,
}

/// El fichero del modelo que hace falta: medium para traducir, turbo para transcribir.
pub fn nombre_para(traducir: bool) -> &'static str {
    if traducir {
        MEDIUM
    } else {
        TURBO
    }
}

fn cargar_uno(carpeta: &Path, nombre: &str) -> Result<Arc<Modelo>, String> {
    let ruta = carpeta.join(nombre);
    if !ruta.exists() {
        return Err(format!("Falta el modelo {} en {}", nombre, carpeta.display()));
    }
    Modelo::cargar(&ruta).map(Arc::new).map_err(|e| format!("{e:#}"))
}

impl Modelos {
    /// Carga los dos a la vez, cada uno en su hilo. Si falta uno, el otro modo sigue
    /// funcionando; el error se devuelve aparte para avisar.
    pub fn cargar(carpeta: &Path) -> (Self, Vec<String>) {
        let hilo = |nombre: &'static str| {
            let carpeta = carpeta.to_path_buf();
            std::thread::spawn(move || cargar_uno(&carpeta, nombre))
        };
        let (turbo, medium) = (hilo(TURBO), hilo(MEDIUM));
        let mut errores = Vec::new();
        let mut recoger = |h: std::thread::JoinHandle<Result<Arc<Modelo>, String>>| match h.join() {
            Ok(Ok(m)) => Some(m),
            Ok(Err(e)) => {
                errores.push(e);
                None
            }
            Err(_) => {
                errores.push("se ha caído la carga de un modelo".into());
                None
            }
        };
        let modelos = Self { turbo: recoger(turbo), medium: recoger(medium) };
        (modelos, errores)
    }

    /// Solo el modelo de un modo; el otro se queda sin cargar.
    pub fn cargar_para(carpeta: &Path, traducir: bool) -> Result<Self, String> {
        let modelo = Some(cargar_uno(carpeta, nombre_para(traducir))?);
        Ok(if traducir { Self { turbo: None, medium: modelo } } else { Self { turbo: modelo, medium: None } })
    }

    /// El modelo que toca: medium para traducir, turbo para transcribir.
    pub fn para(&self, traducir: bool) -> Option<Arc<Modelo>> {
        if traducir {
            self.medium.clone()
        } else {
            self.turbo.clone()
        }
    }
}

/// Dónde están los modelos: `TRUJAMAN_MODELOS`, o la carpeta `modelos/` del directorio
/// actual o de alguno de sus padres, o la que hay junto al ejecutable (o sus padres).
pub fn carpeta_modelos() -> PathBuf {
    if let Some(ruta) = std::env::var_os("TRUJAMAN_MODELOS") {
        return PathBuf::from(ruta);
    }
    let mut sitios: Vec<PathBuf> = Vec::new();
    if let Ok(actual) = std::env::current_dir() {
        sitios.extend(actual.ancestors().map(Path::to_path_buf));
    }
    if let Ok(exe) = std::env::current_exe() {
        sitios.extend(exe.ancestors().skip(1).map(Path::to_path_buf));
    }
    sitios
        .iter()
        .map(|s| s.join("modelos"))
        .find(|m| m.join(TURBO).exists() || m.join(MEDIUM).exists())
        .unwrap_or_else(|| PathBuf::from("modelos"))
}

fn silenciar_whisper() {
    static UNA_VEZ: Once = Once::new();
    UNA_VEZ.call_once(whisper_rs::install_logging_hooks);
}
