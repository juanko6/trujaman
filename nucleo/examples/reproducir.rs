//! Reproduce un WAV por una salida concreta. Sirve para probar la app en vivo sin
//! hablar: se reproduce la frase de prueba en un dispositivo virtual (por ejemplo
//! «Steam Streaming Speakers») y la app escucha su micro gemelo.
//!
//!   cargo run --release -p nucleo --example reproducir -- fichero.wav "nombre de la salida"

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use nucleo::audio::{leer_wav, Remuestreo, FRECUENCIA};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let fichero = args.next().context("falta el fichero")?;
    let nombre = args.next();
    let host = cpal::default_host();
    let salida = match &nombre {
        Some(n) => host
            .output_devices()?
            .find(|d| d.description().map(|x| x.name() == n).unwrap_or(false))
            .with_context(|| format!("no encuentro la salida {n}"))?,
        None => host.default_output_device().context("no hay salida")?,
    };
    let config = salida.default_output_config()?;
    let canales = config.channels() as usize;
    let frecuencia = config.sample_rate();
    let mut muestras = Vec::new();
    Remuestreo::new(FRECUENCIA, frecuencia).pasar(&leer_wav(fichero.as_ref())?, &mut muestras);
    let total = muestras.len();
    let pos = Arc::new(AtomicUsize::new(0));
    let p = pos.clone();
    let flujo = salida.build_output_stream::<f32, _, _>(
        config.into(),
        move |datos, _| {
            for marco in datos.chunks_mut(canales) {
                let i = p.fetch_add(1, Ordering::Relaxed);
                marco.fill(muestras.get(i).copied().unwrap_or(0.0));
            }
        },
        |e| eprintln!("error: {e}"),
        None,
    )?;
    flujo.play()?;
    while pos.load(Ordering::Relaxed) < total {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    Ok(())
}
