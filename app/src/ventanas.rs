//! Las ventanas pequeñas: Acerca de y Ajustes. Solo hay una de cada; pedirla otra vez
//! la trae al frente.

use std::time::Duration;

use gpui::{
    div, img, point, prelude::*, px, size, AnyWindowHandle, App, Bounds, Context, FontWeight, Global, HighlightStyle,
    SharedString, StyledText, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use nucleo::audio::{self, Dispositivos};
use nucleo::voz::Locutor;

use crate::comun::{
    barra, casilla, desplegable, etiqueta_dispositivo, icono, nombre, paleta, segmentado, Paleta, ALTO_BARRA, AUTOR,
    WEB,
};
use crate::preferencias::{Letra, Preferencias, Tamano, Tema};

#[derive(Default)]
struct Abiertas {
    ajustes: Option<AnyWindowHandle>,
    acerca: Option<AnyWindowHandle>,
}

impl Global for Abiertas {}

pub fn opciones_ventana(titulo: &str, ancho: f32, alto: f32, redimensionable: bool, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(ancho), px(alto)), cx))),
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from(titulo.to_owned())),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(ALTO_BARRA / 2. - 7.))),
        }),
        is_resizable: redimensionable,
        // Por debajo de esto la tarjeta de Escuchar no cabe y el texto en vivo se queda sin sitio.
        window_min_size: redimensionable.then(|| size(px(480.), px(620.))),
        ..Default::default()
    }
}

/// Trae al frente la ventana si sigue abierta; si no, la abre con `abrir`.
fn traer_o_abrir(
    cx: &mut App,
    cual: fn(&mut Abiertas) -> &mut Option<AnyWindowHandle>,
    abrir: impl FnOnce(&mut App) -> Option<AnyWindowHandle>,
) {
    if !cx.has_global::<Abiertas>() {
        cx.set_global(Abiertas::default());
    }
    if let Some(h) = *cual(cx.global_mut::<Abiertas>()) {
        if h.update(cx, |_, window, _| window.activate_window()).is_ok() {
            return;
        }
    }
    let nueva = abrir(cx);
    *cual(cx.global_mut::<Abiertas>()) = nueva;
}

// ---- Acerca de ------------------------------------------------------------------

pub fn abrir_acerca(cx: &mut App) {
    traer_o_abrir(
        cx,
        |a| &mut a.acerca,
        |cx| {
            cx.open_window(opciones_ventana("Acerca de Trujamán", 360., 440., false, cx), |_, cx| cx.new(|_| AcercaDe))
                .ok()
                .map(Into::into)
        },
    );
}

struct AcercaDe;

impl Render for AcercaDe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = paleta(window, cx);
        let enlace = WEB.trim_start_matches("https://");
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .bg(p.fondo)
            .text_color(p.tinta)
            .font_family(".SystemUIFont")
            .text_center()
            .pt(px(ALTO_BARRA + 4.))
            .px(px(32.))
            .pb(px(22.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(img(icono()).size(px(84.)))
                    .child(div().mt(px(10.)).child(nombre(30., p)))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(p.tenue)
                            .child(format!("Versión {}", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(
                        div()
                            .mt(px(14.))
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .child("Transcribe y traduce tu voz en tiempo real, sin salir de tu Mac."),
                    )
                    .child(div().mt(px(16.)).text_size(px(13.)).text_color(p.tenue).child("Hecho por").child(
                        div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(p.tinta).child(AUTOR),
                    ))
                    .child(
                        div()
                            .id("web")
                            .mt(px(8.))
                            .text_size(px(13.))
                            .text_color(p.acento_texto)
                            .cursor_pointer()
                            .hover(|d| d.underline())
                            .child(enlace)
                            .on_click(|_, _, cx| cx.open_url(WEB)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .mt(px(16.))
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(p.raya)
                    .text_size(px(11.))
                    .text_color(p.tenue)
                    .child("Licencia MIT · Whisper (whisper.cpp) en local"),
            )
    }
}

// ---- Ajustes --------------------------------------------------------------------

pub fn abrir_ajustes(cx: &mut App) {
    traer_o_abrir(
        cx,
        |a| &mut a.ajustes,
        |cx| {
            cx.open_window(opciones_ventana("Ajustes", 520., 780., true, cx), |_, cx| cx.new(Ajustes::new))
                .ok()
                .map(Into::into)
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
enum Lista {
    Entrada,
    Salida,
    Voz,
}

struct Ajustes {
    dispositivos: Dispositivos,
    voces: Vec<String>,
    abierta: Option<Lista>,
}

impl Ajustes {
    fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            let voces: Vec<String> = cx
                .background_executor()
                .spawn(async {
                    Locutor::global()
                        .map(|l| {
                            l.voces().iter().filter(|v| v.idioma.starts_with("en")).map(|v| v.nombre.clone()).collect()
                        })
                        .unwrap_or_default()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.voces = voces;
                cx.notify();
            });
        })
        .detach();
        cx.spawn(async move |this, cx| loop {
            let nuevos = cx.background_executor().spawn(async { audio::dispositivos() }).await;
            let seguir = this.update(cx, |this, cx| {
                if nuevos != this.dispositivos {
                    this.dispositivos = nuevos;
                    cx.notify();
                }
            });
            if seguir.is_err() {
                break;
            }
            cx.background_executor().timer(Duration::from_secs(2)).await;
        })
        .detach();
        Self { dispositivos: Dispositivos::default(), voces: Vec::new(), abierta: None }
    }

    fn lista(
        &self,
        cual: Lista,
        id: &'static str,
        prefs: &Preferencias,
        p: Paleta,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (elegido, mut opciones, sistema) = match cual {
            Lista::Entrada => (
                prefs.entrada.clone(),
                self.dispositivos.entradas.iter().map(etiqueta_dispositivo).collect::<Vec<_>>(),
                "El de macOS",
            ),
            Lista::Salida => (
                prefs.salida.clone(),
                self.dispositivos.salidas.iter().map(etiqueta_dispositivo).collect(),
                "La de macOS",
            ),
            Lista::Voz => (
                prefs.voz.clone(),
                self.voces.iter().map(|v| (Some(v.clone()), v.clone())).collect(),
                "Samantha (o la primera en inglés)",
            ),
        };
        opciones.insert(0, (None, sistema.to_string()));
        let valor = elegido.clone().unwrap_or_else(|| sistema.to_string());
        desplegable(
            id,
            valor,
            opciones,
            self.abierta == Some(cual),
            false,
            p,
            cx,
            move |this: &mut Self, abrir| this.abierta = abrir.then_some(cual),
            move |_: &mut Self, valor, cx| {
                Preferencias::cambiar(cx, |pr| match cual {
                    Lista::Entrada => pr.entrada = valor,
                    Lista::Salida => pr.salida = valor,
                    Lista::Voz => pr.voz = valor,
                })
            },
        )
    }
}

impl Render for Ajustes {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = paleta(window, cx);
        let prefs = cx.global::<Preferencias>().clone();

        let seccion = |titulo: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .bg(p.papel)
                .border_1()
                .border_color(p.raya)
                .rounded(px(14.))
                .p(px(16.))
                .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(p.tenue).child(titulo))
        };
        let fila = |etiqueta: &'static str, control: gpui::AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(div().w(px(120.)).flex_none().text_color(p.tenue).child(etiqueta))
                .child(div().flex_1().min_w_0().child(control))
        };

        let tema =
            segmentado("tema", &Tema::TODOS, prefs.tema, p, cx, |_, t, cx| Preferencias::cambiar(cx, |pr| pr.tema = t));
        let apariencia = seccion("APARIENCIA").child(fila("Tema", tema.into_any_element()));

        let entrada = self.lista(Lista::Entrada, "def-entrada", &prefs, p, cx);
        let salida = self.lista(Lista::Salida, "def-salida", &prefs, p, cx);
        let voz = self.lista(Lista::Voz, "def-voz", &prefs, p, cx);
        let modo = segmentado(
            "def-modo",
            &[(true, "Traducir al inglés"), (false, "Solo transcribir")],
            prefs.traducir,
            p,
            cx,
            |_, t, cx| Preferencias::cambiar(cx, |pr| pr.traducir = t),
        );
        let hablar = casilla("def-hablar", "Leer en voz alta la traducción", prefs.hablar, false, p, |_, _, cx| {
            Preferencias::cambiar(cx, |pr| pr.hablar = !pr.hablar)
        });
        let al_abrir = seccion("AL ABRIR")
            .child(fila("Micrófono", entrada))
            .child(fila("Salida", salida))
            .child(fila("Modo", modo.into_any_element()))
            .child(fila("", hablar.into_any_element()))
            .child(fila("Voz", voz));

        let letra = segmentado("letra", &Letra::TODAS, prefs.letra, p, cx, |_, l, cx| {
            Preferencias::cambiar(cx, |pr| pr.letra = l)
        });
        let tamano = segmentado("tamano", &Tamano::TODOS, prefs.tamano, p, cx, |_, t, cx| {
            Preferencias::cambiar(cx, |pr| pr.tamano = t)
        });
        let puntos = prefs.tamano.puntos();
        let muestra = div()
            .bg(p.fondo)
            .border_1()
            .border_color(p.raya)
            .rounded(px(10.))
            .px(px(14.))
            .py(px(12.))
            .font_family(prefs.letra.familia())
            .text_size(px(puntos))
            .line_height(px(puntos * 1.5))
            .child({
                // Como en la ventana principal: lo firme y, en gris, lo que aún puede cambiar.
                let firme = "Tomorrow morning I have to go to the bank to open a new account, ";
                let todo = format!("{firme}and then I want to go to the market.");
                let gris = HighlightStyle { color: Some(p.tenue), ..Default::default() };
                StyledText::new(todo.clone()).with_highlights([(firme.len()..todo.len(), gris)])
            });
        let texto = seccion("TEXTO EN VIVO")
            .child(fila("Letra", letra.into_any_element()))
            .child(fila("Tamaño", tamano.into_any_element()))
            .child(muestra);

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.fondo)
            .text_color(p.tinta)
            .text_size(px(14.))
            .font_family(".SystemUIFont")
            .child(barra(Some("Ajustes"), true, p))
            .child(
                div()
                    .id("ajustes")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .px(px(16.))
                    .pt(px(12.))
                    .pb(px(24.))
                    .child(apariencia)
                    .child(al_abrir)
                    .child(texto)
                    .child(div().text_size(px(12.)).text_color(p.tenue).child("Los cambios se guardan solos.")),
            )
    }
}
