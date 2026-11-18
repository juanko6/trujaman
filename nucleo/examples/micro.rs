//! Diagnóstico: lista los dispositivos y enseña el volumen del micro durante unos segundos.
//!
//!   cargo run --release -p nucleo --example micro -- [segundos] [nombre del micro]

use std::time::{Duration, Instant};

use nucleo::audio::{abrir_micro, dispositivos, volumen, Audio};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let segundos: f32 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(5.0);
    let nombre = args.next();
    let d = dispositivos();
    println!("Entradas:");
    for x in &d.entradas {
        println!(
            "  {} {}{}",
            x.nombre,
            if x.predeterminado { "· predeterminado " } else { "" },
            if x.integrado { "· integrado" } else { "" }
        );
    }
    println!("Salidas:");
    for x in &d.salidas {
        println!(
            "  {} {}{}",
            x.nombre,
            if x.predeterminado { "· predeterminado " } else { "" },
            if x.integrado { "· integrado" } else { "" }
        );
    }
    let (tx, rx) = crossbeam_channel::unbounded();
    let _flujo = abrir_micro(nombre.as_deref(), tx)?;
    let inicio = Instant::now();
    let (mut pico, mut tramas) = (0.0f32, 0);
    let mut ultimo = Instant::now();
    while inicio.elapsed().as_secs_f32() < segundos {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Audio::Trama(t)) => {
                pico = pico.max(volumen(&t));
                tramas += 1;
            }
            Ok(Audio::Fin(m)) => anyhow::bail!("se cortó: {m:?}"),
            Err(_) => {}
        }
        if ultimo.elapsed() >= Duration::from_millis(500) {
            println!(
                "{:4.1} s  pico {:.2}%  ({} tramas de 30 ms)",
                inicio.elapsed().as_secs_f32(),
                pico * 100.0,
                tramas
            );
            pico = 0.0;
            ultimo = Instant::now();
        }
    }
    Ok(())
}
