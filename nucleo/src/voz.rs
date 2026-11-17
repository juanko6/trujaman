//! El locutor: dice en alto los trozos firmes con las voces del sistema (crate `tts`).
//!
//! Es el `scripts/locutor/locutor.swift` de antes, con la misma cola para no quedarse
//! atrás cuando llega texto más rápido de lo que puede decirlo:
//!   - al acabar un trozo, junta todo lo que esté esperando y lo dice de un tirón;
//!   - cuanto más hay esperando, más rápido habla;
//!   - si hay demasiado, se salta lo más viejo (como los subtítulos en directo).
//!
//! Vive en un hilo propio durante toda la app: el backend de macOS de `tts` solo se
//! puede crear una vez por proceso.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};

use crate::Evento;

const RITMO_BASE: f32 = 0.5; // AVSpeechUtteranceDefaultSpeechRate
const RITMO_MAXIMO: f32 = 0.62; // más rápido deja de entenderse
const PALABRAS_SIN_PRISA: usize = 5; // hasta aquí, ritmo normal
const PALABRAS_MAXIMAS: usize = 28; // más que esto esperando: se salta lo viejo
/// Tras callar, el micro sigue silenciado un poco más, por el eco de la sala.
const ECO: Duration = Duration::from_millis(400);

/// Una voz instalada.
#[derive(Clone, Debug, PartialEq)]
pub struct Voz {
    pub id: String,
    /// Como la enseña la interfaz: "Ava (Premium)".
    pub nombre: String,
    pub idioma: String,
    /// 1 normal, 2 mejorada, 3 premium (sale del identificador de la voz).
    pub calidad: u8,
}

enum Orden {
    Voz(Option<String>),
    Decir(String),
    Callar,
    Eventos(Option<Sender<Evento>>),
}

pub struct Locutor {
    ordenes: Sender<Orden>,
    hablando: Arc<AtomicBool>,
    voces: Vec<Voz>,
}

static LOCUTOR: OnceLock<Result<Locutor, String>> = OnceLock::new();

impl Locutor {
    /// El locutor de la app (se crea la primera vez que se pide).
    pub fn global() -> Result<&'static Locutor, String> {
        LOCUTOR.get_or_init(Locutor::arrancar).as_ref().map_err(Clone::clone)
    }

    fn arrancar() -> Result<Locutor, String> {
        let (ordenes, recibir) = unbounded();
        let (listo, esperar) = std::sync::mpsc::channel();
        let hablando = Arc::new(AtomicBool::new(false));
        let marca = hablando.clone();
        std::thread::Builder::new()
            .name("locutor".into())
            .spawn(move || match tts::Tts::default() {
                Ok(tts) => {
                    let _ = listo.send(Ok(listar_voces(&tts)));
                    bucle(tts, recibir, marca);
                }
                Err(e) => {
                    let _ = listo.send(Err(format!("No puedo usar la voz del sistema: {e}")));
                }
            })
            .map_err(|e| e.to_string())?;
        let voces = esperar.recv().map_err(|e| e.to_string())??;
        Ok(Locutor { ordenes, hablando, voces })
    }

    /// Voces instaladas, de mejor a peor calidad.
    pub fn voces(&self) -> &[Voz] {
        &self.voces
    }

    /// Por nombre visible ("Ava (Premium)") o identificador. `None`: la del sistema.
    pub fn elegir_voz(&self, voz: Option<String>) {
        let _ = self.ordenes.send(Orden::Voz(voz));
    }

    pub fn decir(&self, texto: &str) {
        if !texto.trim().is_empty() {
            let _ = self.ordenes.send(Orden::Decir(texto.trim().to_owned()));
        }
    }

    /// Calla ya y olvida lo pendiente.
    pub fn callar(&self) {
        let _ = self.ordenes.send(Orden::Callar);
    }

    /// A quién avisar de cuándo empieza y acaba de hablar.
    pub fn conectar(&self, eventos: Option<Sender<Evento>>) {
        let _ = self.ordenes.send(Orden::Eventos(eventos));
    }

    /// Verdadero mientras habla y un poco después (para silenciar el micro).
    pub fn hablando(&self) -> bool {
        self.hablando.load(Ordering::Relaxed)
    }

    /// Para medir: verdadero mientras quede algo por decir.
    pub fn ocupado(&self) -> bool {
        self.hablando() || !self.ordenes.is_empty()
    }
}

fn listar_voces(tts: &tts::Tts) -> Vec<Voz> {
    let mut voces: Vec<Voz> = tts
        .voices()
        .unwrap_or_default()
        .into_iter()
        .map(|v| {
            let id = v.id();
            let calidad = if id.contains(".premium.") {
                3
            } else if id.contains(".enhanced.") {
                2
            } else {
                1
            };
            let nombre = match calidad {
                3 => format!("{} (Premium)", v.name()),
                2 => format!("{} (Mejorada)", v.name()),
                _ => v.name(),
            };
            Voz { id, nombre, idioma: v.language().to_string(), calidad }
        })
        // Fuera las voces de broma (Bells, Bubbles…): solo las que tienen nombre de persona.
        .filter(|v| v.id.contains(".voice.") || v.calidad > 1)
        .collect();
    voces.sort_by(|a, b| b.calidad.cmp(&a.calidad).then(a.nombre.cmp(&b.nombre)));
    voces.dedup_by(|a, b| a.nombre == b.nombre);
    voces
}

fn contar(trozos: &[String]) -> usize {
    trozos.iter().map(|t| t.split_whitespace().count()).sum()
}

fn bucle(mut tts: tts::Tts, ordenes: Receiver<Orden>, hablando: Arc<AtomicBool>) {
    let todas = tts.voices().unwrap_or_default();
    let mut cola: Vec<String> = Vec::new();
    let mut eventos: Option<Sender<Evento>> = None;
    // Desde que se manda un trozo hasta que el sintetizador dice que habla pasan unos ms.
    let mut lanzado: Option<Instant> = None;
    let mut sonando = false;
    let mut callado_desde: Option<Instant> = None;

    let avisar = |eventos: &Option<Sender<Evento>>, e: Evento| {
        if let Some(tx) = eventos {
            let _ = tx.send(e);
        }
    };

    loop {
        let primera = match ordenes.recv_timeout(Duration::from_millis(20)) {
            Ok(o) => Some(o),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        for orden in primera.into_iter().chain(ordenes.try_iter()) {
            match orden {
                Orden::Decir(t) => cola.push(t),
                Orden::Voz(nombre) => {
                    let lista = listar_voces(&tts);
                    let elegida = nombre.and_then(|n| lista.into_iter().find(|v| v.nombre == n || v.id == n));
                    if let Some(v) = elegida.and_then(|e| todas.iter().find(|v| v.id() == e.id)) {
                        let _ = tts.set_voice(v);
                    }
                }
                Orden::Callar => {
                    cola.clear();
                    let _ = tts.stop();
                }
                Orden::Eventos(tx) => eventos = tx,
            }
        }

        let habla = tts.is_speaking().unwrap_or(false);
        if habla {
            lanzado = None;
        }
        let esperando_inicio = lanzado.is_some_and(|t| t.elapsed() < Duration::from_secs(1));
        let ahora_suena = habla || esperando_inicio;

        if sonando && !ahora_suena {
            sonando = false;
            callado_desde = Some(Instant::now());
        }

        if !ahora_suena && !cola.is_empty() {
            // Demasiado esperando: fuera lo más viejo.
            let mut saltadas = 0;
            while cola.len() > 1 && contar(&cola) > PALABRAS_MAXIMAS {
                saltadas += cola.remove(0).split_whitespace().count();
            }
            if saltadas > 0 {
                avisar(&eventos, Evento::Aviso(format!("Voz atrasada: me salto {saltadas} palabras.")));
            }
            let palabras = contar(&cola);
            let texto = cola.join(" ");
            cola.clear();
            let prisa = palabras.saturating_sub(PALABRAS_SIN_PRISA) as f32 * 0.008;
            let _ = tts.set_rate((RITMO_BASE + prisa).min(RITMO_MAXIMO));
            if tts.speak(texto, false).is_ok() {
                lanzado = Some(Instant::now());
                sonando = true;
                callado_desde = None;
                // Solo se avisa al empezar a hablar tras un silencio, no en cada trozo.
                if !hablando.swap(true, Ordering::Relaxed) {
                    avisar(&eventos, Evento::Voz(true));
                }
            }
        }

        if let Some(desde) = callado_desde {
            if desde.elapsed() >= ECO {
                callado_desde = None;
                hablando.store(false, Ordering::Relaxed);
                avisar(&eventos, Evento::Voz(false));
            }
        }
    }
}
