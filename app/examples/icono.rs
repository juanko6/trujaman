//! Saca de `app/recursos/icono.svg` las imágenes que pide un icono de Mac, con el margen
//! y la sombra de la plantilla de Apple (el dibujo ocupa 824 de cada 1024 píxeles).
//!
//!   cargo run --release -p trujaman --example icono -- target/Trujaman.iconset
//!   iconutil -c icns target/Trujaman.iconset

use std::path::PathBuf;

use resvg::{tiny_skia, usvg};

const ICONO: &str = include_str!("../recursos/icono.svg");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let salida = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "Trujaman.iconset".into()));
    std::fs::create_dir_all(&salida)?;

    // El dibujo, sin la etiqueta <svg> de fuera, metido en el lienzo de 1024 con sombra.
    let dentro = &ICONO[ICONO.find('>').ok_or("icono.svg raro")? + 1..ICONO.rfind("</svg>").ok_or("icono.svg raro")?];
    let lienzo = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <defs><filter id="s" x="-10%" y="-10%" width="120%" height="125%">
    <feDropShadow dx="0" dy="10" stdDeviation="12" flood-color="#000" flood-opacity="0.3"/>
  </filter></defs>
  <g filter="url(#s)"><svg x="100" y="100" width="824" height="824" viewBox="0 0 100 100">{dentro}</svg></g>
</svg>"##
    );
    let arbol = usvg::Tree::from_str(&lienzo, &usvg::Options::default())?;

    for base in [16u32, 32, 128, 256, 512] {
        for escala in [1u32, 2] {
            let lado = base * escala;
            let mut lienzo = tiny_skia::Pixmap::new(lado, lado).ok_or("tamaño imposible")?;
            let k = lado as f32 / 1024.0;
            resvg::render(&arbol, tiny_skia::Transform::from_scale(k, k), &mut lienzo.as_mut());
            let nombre =
                if escala == 1 { format!("icon_{base}x{base}.png") } else { format!("icon_{base}x{base}@2x.png") };
            lienzo.save_png(salida.join(nombre))?;
        }
    }
    println!("{}", salida.display());
    Ok(())
}
