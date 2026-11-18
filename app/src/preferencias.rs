//! Lo que el usuario deja elegido en Ajustes. Se guarda en
//! `~/Library/Application Support/Trujaman/preferencias.json` y la app entera lo lee
//! como global de GPUI.

use std::path::PathBuf;

use gpui::{App, Global, WindowAppearance};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tema {
    #[default]
    Sistema,
    Claro,
    Oscuro,
}

impl Tema {
    pub const TODOS: [(Tema, &'static str); 3] =
        [(Tema::Sistema, "Sistema"), (Tema::Claro, "Claro"), (Tema::Oscuro, "Oscuro")];

    /// ¿Se pinta en oscuro, con la apariencia que tiene ahora la ventana?
    pub fn oscuro(self, apariencia: WindowAppearance) -> bool {
        match self {
            Tema::Sistema => matches!(apariencia, WindowAppearance::Dark | WindowAppearance::VibrantDark),
            Tema::Claro => false,
            Tema::Oscuro => true,
        }
    }
}

/// La letra del texto que se va formando.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Letra {
    #[default]
    Sistema,
    Serif,
    Mono,
}

impl Letra {
    pub const TODAS: [(Letra, &'static str); 3] =
        [(Letra::Sistema, "Sistema"), (Letra::Serif, "Serif"), (Letra::Mono, "Mono")];

    pub fn familia(self) -> &'static str {
        match self {
            Letra::Sistema => ".SystemUIFont",
            Letra::Serif => "Charter",
            Letra::Mono => "Menlo",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tamano {
    Pequeno,
    #[default]
    Normal,
    Grande,
}

impl Tamano {
    pub const TODOS: [(Tamano, &'static str); 3] =
        [(Tamano::Pequeno, "Pequeña"), (Tamano::Normal, "Normal"), (Tamano::Grande, "Grande")];

    pub fn puntos(self) -> f32 {
        match self {
            Tamano::Pequeno => 16.,
            Tamano::Normal => 19.,
            Tamano::Grande => 23.,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferencias {
    pub tema: Tema,
    /// Micro con el que arranca. `None`: el predeterminado de macOS.
    pub entrada: Option<String>,
    /// Salida con la que arranca (decide si vas con cascos). `None`: la de macOS.
    pub salida: Option<String>,
    /// Voz para leer en alto. `None`: Samantha o la primera en inglés.
    pub voz: Option<String>,
    pub traducir: bool,
    pub hablar: bool,
    pub letra: Letra,
    pub tamano: Tamano,
}

impl Default for Preferencias {
    fn default() -> Self {
        Self {
            tema: Tema::Sistema,
            entrada: None,
            salida: None,
            voz: None,
            traducir: true,
            hablar: true,
            letra: Letra::Sistema,
            tamano: Tamano::Normal,
        }
    }
}

impl Global for Preferencias {}

impl Preferencias {
    fn ruta() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("Trujaman").join("preferencias.json"))
    }

    /// Las guardadas, o las de siempre si no hay o no se entienden.
    pub fn cargar() -> Self {
        Self::ruta()
            .and_then(|r| std::fs::read_to_string(r).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn guardar(&self) {
        let Some(ruta) = Self::ruta() else { return };
        let escrito = ruta
            .parent()
            .map(std::fs::create_dir_all)
            .transpose()
            .and_then(|_| std::fs::write(&ruta, serde_json::to_string_pretty(self).unwrap_or_default()));
        if let Err(e) = escrito {
            eprintln!("[trujamán] no puedo guardar {}: {e}", ruta.display());
        }
    }

    /// Cambia las preferencias, las guarda y avisa a las ventanas.
    pub fn cambiar(cx: &mut App, f: impl FnOnce(&mut Preferencias)) {
        let mut nuevas = cx.global::<Preferencias>().clone();
        f(&mut nuevas);
        if &nuevas != cx.global::<Preferencias>() {
            nuevas.guardar();
            cx.set_global(nuevas);
            cx.refresh_windows();
        }
    }
}
