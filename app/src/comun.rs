//! Lo que comparten las ventanas: colores, marca y controles.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    anchored, deferred, div, img, prelude::*, px, rgb, AnyElement, App, Context, FontWeight, HighlightStyle, Hsla,
    Image, ImageFormat, MouseButton, SharedString, StyledText, Window,
};
use nucleo::audio::Dispositivo;

use crate::preferencias::Preferencias;

// ---- Colores --------------------------------------------------------------------
// Los de ui.html, con el cobre rojizo del icono como acento. Los avisos van en ámbar
// para no confundirse con él.

#[derive(Clone, Copy)]
pub struct Paleta {
    pub fondo: Hsla,
    pub papel: Hsla,
    pub tinta: Hsla,
    pub tenue: Hsla,
    pub raya: Hsla,
    /// El cobre vivo del icono: botones, casillas, bordes.
    pub acento: Hsla,
    /// El mismo cobre, más oscuro (o más claro, en oscuro), para texto pequeño.
    pub acento_texto: Hsla,
    pub acento_suave: Hsla,
    pub sobre_acento: Hsla,
    pub alerta: Hsla,
    pub alerta_suave: Hsla,
}

/// Los colores según el tema elegido en Ajustes (o el del sistema).
pub fn paleta(window: &Window, cx: &App) -> Paleta {
    let c = |x: u32| -> Hsla { rgb(x).into() };
    if cx.global::<Preferencias>().tema.oscuro(window.appearance()) {
        Paleta {
            fondo: c(0x151513),
            papel: c(0x1e1e1b),
            tinta: c(0xeceae3),
            tenue: c(0x9a968b),
            raya: c(0x33322d),
            acento: c(0xf0573a),
            acento_texto: c(0xff8a70),
            acento_suave: c(0x3d2019),
            sobre_acento: c(0xffffff),
            alerta: c(0xf2c46d),
            alerta_suave: c(0x3a2e14),
        }
    } else {
        Paleta {
            fondo: c(0xf6f5f1),
            papel: c(0xffffff),
            tinta: c(0x1c1b18),
            tenue: c(0x6f6c64),
            raya: c(0xe2dfd6),
            acento: c(0xf0573a),
            acento_texto: c(0xc0391f),
            acento_suave: c(0xfde6df),
            sobre_acento: c(0xffffff),
            alerta: c(0x8a5300),
            alerta_suave: c(0xfbf0da),
        }
    }
}

/// Lo que ocupa la barra de título transparente (los tres botones de la ventana).
pub const ALTO_BARRA: f32 = 32.;

/// La franja fija de arriba, donde van los botones de la ventana. Es opaca para que lo
/// que se desplaza pase por debajo y no por encima de los botones.
pub fn barra(titulo: Option<&'static str>, raya: bool, p: Paleta) -> impl IntoElement {
    div()
        .flex_none()
        .h(px(ALTO_BARRA))
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(p.fondo)
        .when(raya, |d| d.border_b_1().border_color(p.raya))
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .children(titulo)
}

// ---- Marca ----------------------------------------------------------------------

/// La letra del nombre (de las que trae macOS).
pub const LETRA_NOMBRE: &str = "Charter";

pub const AUTOR: &str = "Juan Gutierrez";
pub const WEB: &str = "https://github.com/juanko6/trujaman";

/// El icono, el mismo dibujo del que sale el de la app (`examples/icono.rs`).
///
/// Se pasa a PNG aquí porque GPUI 0.2.2 pinta los SVG en color con el rojo y el azul
/// cambiados (no pasa de RGBA a BGRA, como sí hace con los PNG).
pub fn icono() -> Arc<Image> {
    static ICONO: std::sync::OnceLock<Arc<Image>> = std::sync::OnceLock::new();
    ICONO
        .get_or_init(|| {
            use resvg::{tiny_skia, usvg};
            let svg = include_str!("../recursos/icono.svg");
            let png = usvg::Tree::from_str(svg, &usvg::Options::default()).ok().and_then(|arbol| {
                let lado = 256;
                let mut lienzo = tiny_skia::Pixmap::new(lado, lado)?;
                let k = lado as f32 / arbol.size().width();
                resvg::render(&arbol, tiny_skia::Transform::from_scale(k, k), &mut lienzo.as_mut());
                lienzo.encode_png().ok()
            });
            Arc::new(Image::from_bytes(ImageFormat::Png, png.unwrap_or_default()))
        })
        .clone()
}

/// «trujamán» con la «á» en cobre, como en el logotipo.
pub fn nombre(tamano: f32, p: Paleta) -> impl IntoElement {
    div().text_size(px(tamano)).font_weight(FontWeight::MEDIUM).font_family(LETRA_NOMBRE).child(
        StyledText::new("trujamán").with_highlights([(
            "trujam".len().."trujamá".len(),
            HighlightStyle { color: Some(p.acento), ..Default::default() },
        )]),
    )
}

pub fn logotipo(icono_px: f32, nombre_px: f32, p: Paleta) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(icono_px * 0.3))
        .child(img(icono()).size(px(icono_px)))
        .child(nombre(nombre_px, p))
}

// ---- Controles ------------------------------------------------------------------

pub fn casilla(
    id: impl Into<SharedString>,
    texto: impl Into<SharedString>,
    marcada: bool,
    bloqueada: bool,
    p: Paleta,
    al_pulsar: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(7.))
        .when(bloqueada, |d| d.opacity(0.55))
        .when(!bloqueada, |d| d.cursor_pointer().on_click(al_pulsar))
        .child(
            div()
                .size(px(16.))
                .rounded(px(4.))
                .border_1()
                .border_color(if marcada { p.acento } else { p.tenue })
                .when(marcada, |d| d.bg(p.acento))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .text_color(p.sobre_acento)
                .when(marcada, |d| d.child("✓")),
        )
        .child(texto.into())
}

/// Botones pegados, uno elegido (Traducir | Transcribir, Sistema | Claro | Oscuro…).
pub fn segmentado<V: 'static, T: Copy + PartialEq + 'static>(
    id: &'static str,
    opciones: &[(T, &'static str)],
    actual: T,
    p: Paleta,
    cx: &mut Context<V>,
    elegir: impl Fn(&mut V, T, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let elegir = Rc::new(elegir);
    let mut fila = div().flex().p(px(3.)).bg(p.fondo).border_1().border_color(p.raya).rounded(px(10.));
    for (i, &(valor, texto)) in opciones.iter().enumerate() {
        let activo = valor == actual;
        let elegir = elegir.clone();
        fila = fila.child(
            div()
                .id(SharedString::from(format!("{id}-{i}")))
                .flex_1()
                .flex()
                .justify_center()
                .py(px(7.))
                .px(px(8.))
                .rounded(px(7.))
                .cursor_pointer()
                .text_color(if activo { p.tinta } else { p.tenue })
                .when(activo, |d| d.bg(p.papel).font_weight(FontWeight::SEMIBOLD).border_1().border_color(p.raya))
                .child(texto)
                .on_click(cx.listener(move |this, _, _, cx| {
                    elegir(this, valor, cx);
                    cx.notify();
                })),
        );
    }
    fila
}

/// Un desplegable sencillo: el valor elegido y, al pulsar, la lista encima de todo.
/// `opciones`: (valor, texto); `None` es «el del sistema».
#[allow(clippy::too_many_arguments)]
pub fn desplegable<V: 'static>(
    id: &'static str,
    valor: String,
    opciones: Vec<(Option<String>, String)>,
    abierta: bool,
    bloqueado: bool,
    p: Paleta,
    cx: &mut Context<V>,
    alternar: impl Fn(&mut V, bool) + 'static,
    elegir: impl Fn(&mut V, Option<String>, &mut Context<V>) + 'static,
) -> AnyElement {
    let alternar = Rc::new(alternar);
    let elegir = Rc::new(elegir);
    let abierta = abierta && !bloqueado;
    let caja = {
        let alternar = alternar.clone();
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .w_full()
            .px(px(10.))
            .py(px(8.))
            .bg(p.fondo)
            .border_1()
            .border_color(if abierta { p.acento } else { p.raya })
            .rounded(px(9.))
            .text_size(px(15.))
            .text_color(p.tinta)
            .when(bloqueado, |d| d.opacity(0.55))
            .when(!bloqueado, |d| d.cursor_pointer())
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(valor))
            .child(div().text_color(p.tenue).text_size(px(11.)).child("▼"))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !bloqueado {
                    alternar(this, !abierta);
                    cx.notify();
                }
            }))
    };

    let mut contenedor = div().relative().w_full().child(caja);
    if abierta {
        let cerrar = alternar.clone();
        let mut lista = div()
            .id(SharedString::from(format!("{id}-opciones")))
            .occlude()
            .mt(px(4.))
            .min_w(px(220.))
            .max_h(px(280.))
            .overflow_y_scroll()
            .bg(p.papel)
            .border_1()
            .border_color(p.raya)
            .rounded(px(9.))
            .shadow_lg()
            .py(px(4.))
            .text_size(px(14.))
            .text_color(p.tinta)
            .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                cerrar(this, false);
                cx.notify();
            }));
        if opciones.is_empty() {
            lista = lista.child(div().px(px(10.)).py(px(6.)).text_color(p.tenue).child("Nada disponible"));
        }
        for (i, (valor, texto)) in opciones.into_iter().enumerate() {
            let (elegir, cerrar) = (elegir.clone(), alternar.clone());
            lista = lista.child(
                div()
                    .id(SharedString::from(format!("{id}-{i}")))
                    .px(px(10.))
                    .py(px(6.))
                    .cursor_pointer()
                    .hover(|d| d.bg(p.acento_suave))
                    .child(texto)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            elegir(this, valor.clone(), cx);
                            cerrar(this, false);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            );
        }
        contenedor = contenedor.child(deferred(anchored().snap_to_window().child(lista)).with_priority(1));
    }
    contenedor.into_any_element()
}

pub fn etiqueta_dispositivo(d: &Dispositivo) -> (Option<String>, String) {
    let texto = if d.predeterminado { format!("{}  · predeterminado", d.nombre) } else { d.nombre.clone() };
    (Some(d.nombre.clone()), texto)
}
