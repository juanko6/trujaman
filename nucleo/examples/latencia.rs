//! Prueba de latencia sin micro: pasa un WAV por el núcleo a ritmo real y mide cuándo
//! sale la primera palabra y cuánto va por detrás de la voz.
//!
//!   cargo run --release -p nucleo --example latencia -- [fichero.wav] [--veces N] [--transcribir]
//!                                                     [--intervalo 0.5] [--reserva 2]
//!   cargo run --release -p nucleo --example latencia -- --trozos salida.txt [fichero.wav]
//!
//! Sin fichero usa `audio/frase-prueba.wav`, la frase de 8,6 s de la GUIA, y si no
//! existe la genera con `say -v Mónica` (solo Mac).
//!
//! Con `--trozos` no pasa nada por Whisper: lee líneas `[  2.3s] texto` (lo que escribe
//! `scripts/simultaneo.py` con `MEDIR=1`) y les aplica las mismas cuentas, para comparar
//! las dos versiones con la misma vara.
//!
//! Cómo se mide el retraso: no se sabe a qué segundo dijo cada palabra, así que se supone
//! que hablaste a ritmo constante entre el principio y el final de la voz. Un trozo que
//! acaba en la palabra k de N se dijo hacia `inicio + k/N · (fin − inicio)`; su retraso
//! es cuándo salió menos eso.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use nucleo::audio::{leer_wav, volumen, FRECUENCIA, TRAMA};
use nucleo::whisper::{carpeta_modelos, Modelos};
use nucleo::{Ajustes, Evento, Fuente, Sesion};

const FRASE: &str = "Mañana por la mañana tengo que ir al banco a abrir una cuenta nueva, y luego quiero ir \
                     al mercado a comprar fruta, porque mis padres vienen a cenar a casa el fin de semana.";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut fichero, mut veces, mut traducir, mut trozos) = (None, 3, true, None);
    let mut ajustes = Ajustes { hablar: false, ..Ajustes::default() };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--veces" => veces = args.next().context("--veces N")?.parse()?,
            "--transcribir" => traducir = false,
            "--intervalo" => ajustes.intervalo = args.next().context("--intervalo s")?.parse()?,
            "--reserva" => ajustes.reserva = args.next().context("--reserva n")?.parse()?,
            "--trozos" => trozos = Some(PathBuf::from(args.next().context("--trozos fichero")?)),
            otro => fichero = Some(PathBuf::from(otro)),
        }
    }
    let fichero = match fichero {
        Some(f) => f,
        None => frase_de_prueba()?,
    };
    let muestras = leer_wav(&fichero)?;
    let (inicio_voz, fin_voz) = limites_de_voz(&muestras);
    println!(
        "Audio: {} · {:.1} s (voz de {:.2} a {:.2} s)",
        fichero.display(),
        muestras.len() as f32 / FRECUENCIA as f32,
        inicio_voz,
        fin_voz
    );

    if let Some(ruta) = trozos {
        let lista = leer_trozos(&ruta)?;
        imprimir(&lista);
        let m = medir(&lista, inicio_voz, fin_voz);
        println!("{m}");
        return Ok(());
    }

    let reloj = Instant::now();
    let (modelos, errores) = Modelos::cargar(&carpeta_modelos());
    if !errores.is_empty() {
        bail!(errores.join("\n"));
    }
    println!("Modelos cargados en {:.2} s\n", reloj.elapsed().as_secs_f32());

    ajustes.traducir = traducir;
    println!("Intervalo {} s, reserva {} palabras", ajustes.intervalo, ajustes.reserva);
    let mut todas = Vec::new();
    for vez in 1..=veces {
        let (tx, rx) = crossbeam_channel::unbounded();
        let sesion = Sesion::iniciar(&modelos, ajustes.clone(), Fuente::Muestras(muestras.clone()), tx)?;
        let mut lista = Vec::new();
        let mut tardas = Vec::new();
        for e in rx.iter() {
            match e {
                Evento::Firme { texto, t } => lista.push((t, texto)),
                Evento::Provisional { tarda, .. } => tardas.push(tarda),
                Evento::Aviso(a) => eprintln!("aviso: {a}"),
                Evento::Terminado => break,
                _ => {}
            }
        }
        sesion.parar();
        println!("— Vez {vez}");
        imprimir(&lista);
        let m = medir(&lista, inicio_voz, fin_voz);
        let media_pasada = tardas.iter().sum::<f32>() / tardas.len().max(1) as f32;
        println!("{m}\n  {} pasadas parciales, {:.2} s de media cada una\n", tardas.len(), media_pasada);
        todas.push(m);
    }
    if todas.len() > 1 {
        let n = todas.len() as f32;
        println!(
            "Media de {} veces: primera palabra {:.2} s · retraso medio {:.2} s · última {:.2} s",
            todas.len(),
            todas.iter().map(|m| m.primera).sum::<f32>() / n,
            todas.iter().map(|m| m.retraso_medio).sum::<f32>() / n,
            todas.iter().map(|m| m.ultima).sum::<f32>() / n,
        );
    }
    Ok(())
}

struct Medida {
    primera: f32,
    retraso_medio: f32,
    ultima: f32,
    retraso_final: f32,
}

impl std::fmt::Display for Medida {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "  primera palabra a los {:.2} s · retraso medio {:.2} s · última a los {:.2} s ({:.2} s tras callar)",
            self.primera, self.retraso_medio, self.ultima, self.retraso_final
        )
    }
}

fn imprimir(lista: &[(f32, String)]) {
    for (t, texto) in lista {
        println!("  {t:5.2} s  {texto}");
    }
}

fn medir(lista: &[(f32, String)], inicio: f32, fin: f32) -> Medida {
    let total: usize = lista.iter().map(|(_, t)| t.split_whitespace().count()).sum::<usize>().max(1);
    let mut hechas = 0;
    let mut retrasos = Vec::new();
    for (t, texto) in lista {
        hechas += texto.split_whitespace().count();
        let dicho = inicio + (hechas as f32 / total as f32) * (fin - inicio);
        retrasos.push(t - dicho);
    }
    let ultima = lista.last().map_or(f32::NAN, |(t, _)| *t);
    Medida {
        primera: lista.first().map_or(f32::NAN, |(t, _)| *t),
        retraso_medio: retrasos.iter().sum::<f32>() / retrasos.len().max(1) as f32,
        ultima,
        retraso_final: ultima - fin,
    }
}

/// Primer y último instante con voz (tramas de 30 ms por encima del umbral de siempre).
fn limites_de_voz(muestras: &[f32]) -> (f32, f32) {
    let umbral = Ajustes::default().umbral;
    let con_voz: Vec<usize> =
        muestras.chunks(TRAMA).enumerate().filter(|(_, t)| volumen(t) > umbral).map(|(i, _)| i).collect();
    let seg = |i: usize| (i * TRAMA) as f32 / FRECUENCIA as f32;
    match (con_voz.first(), con_voz.last()) {
        (Some(&a), Some(&b)) => (seg(a), seg(b + 1)),
        _ => (0.0, seg(muestras.len() / TRAMA)),
    }
}

fn leer_trozos(ruta: &Path) -> Result<Vec<(f32, String)>> {
    let texto = std::fs::read_to_string(ruta)?;
    let mut lista = Vec::new();
    for linea in texto.lines() {
        let Some(resto) = linea.trim().strip_prefix('[') else { continue };
        let Some((t, frase)) = resto.split_once("s]") else { continue };
        if let Ok(t) = t.trim().replace(',', ".").parse::<f32>() {
            lista.push((t, frase.trim().to_owned()));
        }
    }
    Ok(lista)
}

/// `audio/frase-prueba.wav`; si no está, la genera con la voz Mónica de macOS.
fn frase_de_prueba() -> Result<PathBuf> {
    let carpeta = carpeta_modelos().parent().map(|p| p.join("audio")).unwrap_or_else(|| "audio".into());
    let wav = carpeta.join("frase-prueba.wav");
    if wav.exists() {
        return Ok(wav);
    }
    std::fs::create_dir_all(&carpeta)?;
    let aiff = carpeta.join("frase-prueba.aiff");
    let ok = Command::new("say").args(["-v", "Mónica", "-o"]).arg(&aiff).arg(FRASE).status()?.success()
        && Command::new("afconvert")
            .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
            .arg(&aiff)
            .arg(&wav)
            .status()?
            .success();
    let _ = std::fs::remove_file(&aiff);
    if !ok {
        bail!("No he podido generar {} (hace falta `say` y `afconvert`, de macOS)", wav.display());
    }
    Ok(wav)
}
