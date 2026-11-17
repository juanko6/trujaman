//! El núcleo de Trujamán, sin interfaz: micro → detección de voz → Whisper →
//! estabilizador → voz.
//!
//! Corre en su propio hilo y cuenta lo que pasa con [`Evento`]s por un canal; la app
//! (o la prueba de latencia) solo tiene que escucharlos.

pub mod audio;
pub mod estabilizador;
pub mod sesion;
pub mod voz;
pub mod whisper;

pub use sesion::{Ajustes, Fuente, Sesion};

/// Lo que cuenta el núcleo mientras escucha.
#[derive(Clone, Debug, PartialEq)]
pub enum Evento {
    /// Ha empezado a escuchar. `simultaneo`: traduce mientras hablas (con cascos o sin
    /// voz); si no, frase a frase.
    Escuchando {
        simultaneo: bool,
    },
    /// Volumen del micro (0..1), unas diez veces por segundo; `voz`: dentro de una frase.
    Nivel {
        v: f32,
        voz: bool,
    },
    /// Whisper está trabajando (o ha acabado) una pasada.
    Whisper(bool),
    /// El locutor ha empezado (o acabado) de hablar.
    Voz(bool),
    /// Lo que aún no es firme, para pintarlo en gris. `tarda`: segundos de la pasada.
    Provisional {
        texto: String,
        pasadas: u32,
        tarda: f32,
    },
    /// Un trozo firme. `t`: segundos desde que empezó a escuchar.
    Firme {
        texto: String,
        t: f32,
    },
    /// Se ha cerrado la frase (silencio).
    FinFrase,
    Aviso(String),
    /// Ha dejado de escuchar (lo han parado, se acabó el fichero o se cortó el micro).
    Terminado,
}
