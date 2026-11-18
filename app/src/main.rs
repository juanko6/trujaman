//! Trujamán como app de escritorio: la interfaz de `scripts/ui.html` hecha con GPUI.
//!
//! El núcleo (crate `nucleo`) escucha en su propio hilo y cuenta lo que pasa por un
//! canal; aquí solo se pinta.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    anchored, deferred, div, img, prelude::*, px, rgb, size, App, Application, Bounds, ClipboardItem, Context,
    FontWeight, HighlightStyle, Hsla, Image, ImageFormat, MouseButton, ScrollHandle, SharedString, StyledText, Window,
    WindowAppearance, WindowBounds, WindowOptions,
};
use nucleo::audio::{self, Dispositivo, Dispositivos};
use nucleo::voz::Locutor;
use nucleo::whisper::{carpeta_modelos, Modelos};
use nucleo::{Ajustes, Evento, Fuente, Sesion};

// ---- Colores --------------------------------------------------------------------
// Los de ui.html, con el cobre rojizo del icono como acento. Los avisos van en ámbar
// para no confundirse con él.

#[derive(Clone, Copy)]
struct Paleta {
    fondo: Hsla,
    papel: Hsla,
    tinta: Hsla,
    tenue: Hsla,
    raya: Hsla,
    /// El cobre vivo del icono: botones, casillas, bordes.
    acento: Hsla,
    /// El mismo cobre, más oscuro (o más claro, en oscuro), para texto pequeño.
    acento_texto: Hsla,
    acento_suave: Hsla,
    sobre_acento: Hsla,
    alerta: Hsla,
    alerta_suave: Hsla,
}

fn paleta(window: &Window) -> Paleta {
    let c = |x: u32| -> Hsla { rgb(x).into() };
    match window.appearance() {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Paleta {
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
        },
        _ => Paleta {
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
        },
    }
}

// ---- Marca ----------------------------------------------------------------------

/// La letra del nombre en la cabecera (de las que trae macOS).
const LETRA_NOMBRE: &str = "Charter";

/// El icono, el mismo dibujo del que sale el de la app (`examples/icono.rs`).
///
/// Se pasa a PNG aquí porque GPUI 0.2.2 pinta los SVG en color con el rojo y el azul
/// cambiados (no pasa de RGBA a BGRA, como sí hace con los PNG).
fn icono() -> Arc<Image> {
    static ICONO: std::sync::OnceLock<Arc<Image>> = std::sync::OnceLock::new();
    ICONO
        .get_or_init(|| {
            use resvg::{tiny_skia, usvg};
            let svg = include_str!("../recursos/icono.svg");
            let png = usvg::Tree::from_str(svg, &usvg::Options::default()).ok().and_then(|arbol| {
                let lado = 128;
                let mut lienzo = tiny_skia::Pixmap::new(lado, lado)?;
                let k = lado as f32 / arbol.size().width();
                resvg::render(&arbol, tiny_skia::Transform::from_scale(k, k), &mut lienzo.as_mut());
                lienzo.encode_png().ok()
            });
            Arc::new(Image::from_bytes(ImageFormat::Png, png.unwrap_or_default()))
        })
        .clone()
}

// ---- Estado -------------------------------------------------------------------

enum EstadoModelos {
    Cargando,
    Listo,
    Mal(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Lista {
    Entrada,
    Salida,
    Voz,
}

#[derive(Default)]
struct Parrafo {
    firme: String,
    provisional: String,
}

struct Trujaman {
    modelos: Option<Arc<Modelos>>,
    estado: EstadoModelos,
    dispositivos: Dispositivos,
    voces: Vec<String>,
    entrada: Option<String>,
    salida: Option<String>,
    voz: Option<String>,
    traducir: bool,
    hablar: bool,
    /// `None`: lo decide la salida (altavoces del Mac → no; cualquier otra → sí).
    cascos_manual: Option<bool>,
    sesion: Option<Sesion>,
    sesion_id: u64,
    parrafos: Vec<Parrafo>,
    abierto: bool,
    nivel: f32,
    en_voz: bool,
    whisper_activo: bool,
    voz_activa: bool,
    datos: String,
    aviso: Option<String>,
    lista_abierta: Option<Lista>,
    scroll: ScrollHandle,
}

impl Trujaman {
    fn new(cx: &mut Context<Self>) -> Self {
        let yo = Self {
            modelos: None,
            estado: EstadoModelos::Cargando,
            dispositivos: audio::dispositivos(),
            voces: Vec::new(),
            entrada: None,
            salida: None,
            voz: None,
            traducir: true,
            hablar: true,
            cascos_manual: None,
            sesion: None,
            sesion_id: 0,
            parrafos: Vec::new(),
            abierto: false,
            nivel: 0.0,
            en_voz: false,
            whisper_activo: false,
            voz_activa: false,
            datos: String::new(),
            aviso: None,
            lista_abierta: None,
            scroll: ScrollHandle::new(),
        };

        // Los dos modelos, una sola vez, fuera del hilo de la interfaz.
        cx.spawn(async move |this, cx| {
            let carpeta = carpeta_modelos();
            let (modelos, errores) = cx.background_executor().spawn(async move { Modelos::cargar(&carpeta) }).await;
            let _ = this.update(cx, |this, cx| {
                let empezar = depurar() && errores.is_empty();
                this.estado =
                    if errores.is_empty() { EstadoModelos::Listo } else { EstadoModelos::Mal(errores.join(" · ")) };
                this.modelos = Some(Arc::new(modelos));
                if empezar {
                    this.entrada = std::env::var("TRUJAMAN_MICRO").ok();
                    if let Ok(c) = std::env::var("TRUJAMAN_CASCOS") {
                        this.cascos_manual = Some(c != "0");
                    }
                    eprintln!(
                        "[trujamán] modelos listos; empiezo a escuchar ({:?}, cascos {})",
                        this.entrada,
                        this.cascos()
                    );
                    this.alternar_escucha(cx);
                }
                cx.notify();
            });
        })
        .detach();

        // El locutor y sus voces en inglés.
        cx.spawn(async move |this, cx| {
            let voces = cx
                .background_executor()
                .spawn(async move {
                    Locutor::global().map(|l| {
                        l.voces().iter().filter(|v| v.idioma.starts_with("en")).map(|v| v.nombre.clone()).collect()
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match voces {
                    Ok(v) => {
                        this.voces = v;
                        let samantha = this.voces.iter().find(|n| n.starts_with("Samantha")).cloned();
                        this.voz = samantha.or_else(|| this.voces.first().cloned());
                    }
                    Err(e) => this.aviso = Some(e),
                }
                cx.notify();
            });
        })
        .detach();

        // Los dispositivos cambian (cascos que se conectan…): se miran cada 2 s.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let nuevos = cx.background_executor().spawn(async { audio::dispositivos() }).await;
            let seguir = this.update(cx, |this, cx| {
                if this.sesion.is_none() && nuevos != this.dispositivos {
                    this.dispositivos = nuevos;
                    cx.notify();
                }
            });
            if seguir.is_err() {
                break;
            }
        })
        .detach();

        yo
    }

    fn entrada_elegida(&self) -> Option<&Dispositivo> {
        elegido(&self.dispositivos.entradas, &self.entrada)
    }

    fn salida_elegida(&self) -> Option<&Dispositivo> {
        elegido(&self.dispositivos.salidas, &self.salida)
    }

    fn cascos(&self) -> bool {
        self.cascos_manual.unwrap_or_else(|| !self.salida_elegida().is_none_or(|s| s.integrado))
    }

    fn listo(&self) -> bool {
        self.modelos.as_ref().is_some_and(|m| m.para(self.traducir).is_some())
    }

    fn alternar_escucha(&mut self, cx: &mut Context<Self>) {
        if let Some(sesion) = self.sesion.take() {
            // Parar espera a que acabe la pasada de Whisper en curso: fuera de la interfaz.
            cx.background_executor().spawn(async move { sesion.parar() }).detach();
            self.sesion_id += 1;
            self.fin_de_escucha();
            cx.notify();
            return;
        }
        let Some(modelos) = self.modelos.clone() else { return };
        let hablar = self.hablar && self.traducir;
        if hablar && self.salida_elegida().is_some_and(|s| !s.predeterminado) {
            self.aviso = Some(
                "La voz sale por la salida predeterminada de macOS: elegir otra para la voz aún no está hecho \
                 en esta versión. La salida elegida solo decide si vas con cascos."
                    .into(),
            );
        }
        let ajustes = Ajustes {
            traducir: self.traducir,
            cascos: self.cascos(),
            hablar,
            voz: self.voz.clone(),
            ..Ajustes::default()
        };
        let micro = self.entrada_elegida().filter(|d| !d.predeterminado).map(|d| d.nombre.clone());
        let (tx, rx) = crossbeam_channel::unbounded();
        match Sesion::iniciar(&modelos, ajustes, Fuente::Micro(micro), tx) {
            Ok(s) => self.sesion = Some(s),
            Err(e) => {
                self.aviso = Some(format!("{e:#}"));
                cx.notify();
                return;
            }
        }
        self.sesion_id += 1;
        let id = self.sesion_id;
        // Puente: el núcleo usa un canal normal; la interfaz espera en uno asíncrono.
        let (atx, arx) = async_channel::unbounded();
        std::thread::spawn(move || {
            for e in rx {
                if atx.send_blocking(e).is_err() {
                    break;
                }
            }
        });
        cx.spawn(async move |this, cx| {
            while let Ok(e) = arx.recv().await {
                let seguir = this.update(cx, |this, cx| {
                    if this.sesion_id != id {
                        return false;
                    }
                    this.al_evento(e);
                    cx.notify();
                    true
                });
                if !matches!(seguir, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn fin_de_escucha(&mut self) {
        self.cerrar_parrafo();
        self.nivel = 0.0;
        self.en_voz = false;
        self.whisper_activo = false;
        self.voz_activa = false;
    }

    fn al_evento(&mut self, e: Evento) {
        if depurar() && !matches!(e, Evento::Nivel { .. }) {
            eprintln!("[trujamán] {e:?}");
        }
        match e {
            Evento::Escuchando { .. } => {}
            Evento::Nivel { v, voz } => {
                self.nivel = v;
                self.en_voz = voz;
            }
            Evento::Whisper(si) => self.whisper_activo = si,
            Evento::Voz(si) => self.voz_activa = si,
            Evento::Provisional { texto, pasadas, tarda } => {
                if !texto.is_empty() || self.abierto {
                    self.parrafo().provisional = texto;
                }
                self.datos = format!("{pasadas} pasadas · la última tardó {:.2} s", tarda).replace('.', ",");
                self.scroll.scroll_to_bottom();
            }
            Evento::Firme { texto, .. } => {
                let p = self.parrafo();
                if !p.firme.is_empty() {
                    p.firme.push(' ');
                }
                p.firme.push_str(&texto);
                p.provisional.clear();
                self.scroll.scroll_to_bottom();
            }
            Evento::FinFrase => self.cerrar_parrafo(),
            Evento::Aviso(a) => self.aviso = Some(a),
            Evento::Terminado => {
                self.sesion = None;
                self.fin_de_escucha();
            }
        }
    }

    fn parrafo(&mut self) -> &mut Parrafo {
        if !self.abierto {
            self.parrafos.push(Parrafo::default());
            self.abierto = true;
        }
        self.parrafos.last_mut().unwrap()
    }

    fn cerrar_parrafo(&mut self) {
        if !self.abierto {
            return;
        }
        self.abierto = false;
        if let Some(p) = self.parrafos.last_mut() {
            p.provisional.clear();
            if p.firme.trim().is_empty() {
                self.parrafos.pop();
            }
        }
    }

    fn texto_en_vivo(&self) -> String {
        self.parrafos.iter().map(|p| p.firme.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n")
    }
}

/// `TRUJAMAN_DEPURAR=1`: escribe en la terminal lo que cuenta el núcleo y empieza a
/// escuchar en cuanto cargan los modelos (para probar sin tocar la ventana). Con él,
/// `TRUJAMAN_MICRO="nombre"` elige la entrada y `TRUJAMAN_CASCOS=1|0` fuerza los cascos.
fn depurar() -> bool {
    std::env::var_os("TRUJAMAN_DEPURAR").is_some_and(|v| v != "0")
}

fn elegido<'a>(lista: &'a [Dispositivo], nombre: &Option<String>) -> Option<&'a Dispositivo> {
    nombre
        .as_ref()
        .and_then(|n| lista.iter().find(|d| &d.nombre == n))
        .or_else(|| lista.iter().find(|d| d.predeterminado))
        .or_else(|| lista.first())
}

// ---- Pintar ---------------------------------------------------------------------

impl Render for Trujaman {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = paleta(window);
        let escuchando = self.sesion.is_some();

        let (clase, texto_estado) = match &self.estado {
            EstadoModelos::Cargando => (p.tenue, "Cargando modelos…".to_string()),
            EstadoModelos::Listo => (p.acento, "Listo".to_string()),
            EstadoModelos::Mal(e) => (p.alerta, e.clone()),
        };

        let cabecera = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px(px(2.))
            .pb(px(6.))
            .child(div().flex().items_center().gap(px(10.)).child(img(icono()).size(px(34.))).child(
                div().text_size(px(30.)).font_weight(FontWeight::MEDIUM).font_family(LETRA_NOMBRE).child(
                    // La «á» en cobre, como en el logotipo.
                    StyledText::new("trujamán").with_highlights([(
                        "trujam".len().."trujamá".len(),
                        HighlightStyle { color: Some(p.acento), ..Default::default() },
                    )]),
                ),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(13.))
                    .text_color(p.tenue)
                    .child(div().size(px(8.)).rounded_full().bg(clase))
                    .child(texto_estado),
            );

        let aviso = self.aviso.clone().map(|a| {
            div()
                .flex()
                .justify_between()
                .gap_3()
                .w_full()
                .bg(p.alerta_suave)
                .text_color(p.alerta)
                .rounded(px(11.))
                .px(px(14.))
                .py(px(10.))
                .text_size(px(14.))
                .child(div().flex_1().min_w_0().child(a))
                .child(div().id("cerrar-aviso").cursor_pointer().child("✕").on_click(cx.listener(|this, _, _, cx| {
                    this.aviso = None;
                    cx.notify();
                })))
        });

        // -- Escuchar --
        let entrada = self.entrada_elegida().map(|d| d.nombre.clone()).unwrap_or_else(|| "Sin micrófono".into());
        let salida = self.salida_elegida().map(|d| d.nombre.clone()).unwrap_or_else(|| "Sin salida".into());
        let ancho_nivel = (self.nivel.sqrt() * 170.0).min(100.0) / 100.0;
        let nivel = div().h(px(5.)).rounded(px(3.)).bg(p.raya).overflow_hidden().mt(px(2.)).child(
            div().h_full().w(gpui::relative(ancho_nivel)).rounded(px(3.)).bg(if self.en_voz {
                p.acento
            } else {
                p.tenue
            }),
        );

        let campo = |etiqueta: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap(px(5.))
                .flex_1()
                .min_w_0()
                .text_size(px(13.))
                .text_color(p.tenue)
                .child(etiqueta)
        };
        let dos = div()
            .flex()
            .gap_3()
            .child(campo("Entrada").child(self.desplegable(Lista::Entrada, entrada, escuchando, p, cx)).child(nivel))
            .child(campo("Salida").child(self.desplegable(Lista::Salida, salida, escuchando, p, cx)));

        let boton_modo = |id: &'static str, texto: &'static str, activo: bool, traducir: bool| {
            div()
                .id(id)
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
                    if this.sesion.is_some() {
                        this.aviso = Some("Para de escuchar antes de cambiar de modo.".into());
                    } else {
                        this.traducir = traducir;
                    }
                    cx.notify();
                }))
        };
        let modo = div()
            .flex()
            .mt(px(14.))
            .p(px(3.))
            .bg(p.fondo)
            .border_1()
            .border_color(p.raya)
            .rounded(px(10.))
            .child(boton_modo("traducir", "Traducir al inglés", self.traducir, true))
            .child(boton_modo("transcribir", "Solo transcribir", !self.traducir, false));

        let cascos = self.cascos();
        let opciones = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(18.))
            .gap_y(px(8.))
            .mt(px(12.))
            .text_size(px(14.))
            .child(casilla(
                "hablar",
                "Leer en voz alta",
                self.hablar,
                escuchando,
                p,
                cx.listener(|this, _, _, cx| {
                    this.hablar = !this.hablar;
                    cx.notify();
                }),
            ))
            .when(self.hablar, |d| {
                let voz = self.voz.clone().unwrap_or_else(|| "Voz del sistema".into());
                d.child(div().w(px(190.)).child(self.desplegable(Lista::Voz, voz, escuchando, p, cx)))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .child(casilla(
                        "cascos",
                        "Uso cascos",
                        cascos,
                        escuchando,
                        p,
                        cx.listener(|this, _, _, cx| {
                            this.cascos_manual = Some(!this.cascos());
                            cx.notify();
                        }),
                    ))
                    .child(div().text_color(p.tenue).text_size(px(13.)).child(if cascos {
                        "· traduce mientras hablas"
                    } else {
                        "· frase a frase"
                    })),
            );

        let habilitado = escuchando || self.listo();
        let principal = div()
            .id("escuchar")
            .w_full()
            .mt(px(14.))
            .py(px(13.))
            .flex()
            .justify_center()
            .items_center()
            .gap(px(9.))
            .rounded(px(11.))
            .text_size(px(16.))
            .font_weight(FontWeight::SEMIBOLD)
            .bg(if escuchando { p.tinta } else { p.acento })
            .text_color(if escuchando { p.fondo } else { p.sobre_acento })
            .when(!habilitado, |d| d.opacity(0.45))
            .when(habilitado, |d| d.cursor_pointer())
            .when(escuchando, |d| d.child(div().size(px(9.)).rounded(px(2.)).bg(p.fondo)))
            .child(if escuchando { "Parar" } else { "Escuchar" })
            .on_click(cx.listener(move |this, _, _, cx| {
                if habilitado {
                    this.lista_abierta = None;
                    this.alternar_escucha(cx);
                }
            }));

        let seccion = || div().bg(p.papel).border_1().border_color(p.raya).rounded(px(14.)).p(px(16.));

        let escuchar = seccion().child(dos).child(modo).when(self.traducir, |d| d.child(opciones)).child(principal);

        // -- En vivo --
        let chip = |texto: &'static str, on: bool| {
            div()
                .px(px(9.))
                .py(px(3.))
                .rounded_full()
                .border_1()
                .border_color(if on { p.acento_suave } else { p.raya })
                .when(on, |d| d.bg(p.acento_suave).text_color(p.acento_texto))
                .child(texto)
        };
        let flecha = || div().opacity(0.5).child("→");
        let con_voz = self.traducir && self.hablar;
        let cadena = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(p.tenue)
            .mb(px(12.))
            .child(chip("Micro", self.en_voz))
            .child(flecha())
            .child(chip("Whisper", self.whisper_activo))
            .when(con_voz, |d| d.child(flecha()).child(chip("Voz", self.voz_activa)));

        let mut texto = div()
            .id("texto")
            .flex()
            .flex_col()
            .gap(px(10.))
            .min_h(px(132.))
            .max_h(px(300.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .text_size(px(19.))
            .line_height(px(29.));
        if self.parrafos.is_empty() {
            texto =
                texto.child(div().text_size(px(15.)).text_color(p.tenue).child("Pulsa Escuchar y habla en español."));
        }
        for parrafo in &self.parrafos {
            let firme = parrafo.firme.trim();
            let mut todo = firme.to_string();
            if !parrafo.provisional.is_empty() {
                if !todo.is_empty() {
                    todo.push(' ');
                }
                todo.push_str(&parrafo.provisional);
            }
            let inicio = todo.len() - parrafo.provisional.len();
            let gris = HighlightStyle { color: Some(p.tenue), ..Default::default() };
            texto =
                texto.child(div().child(StyledText::new(todo.clone()).with_highlights([(inicio..todo.len(), gris)])));
        }

        let mini = |id: &'static str, etiqueta: &'static str| {
            div()
                .id(id)
                .text_size(px(13.))
                .text_color(p.tinta)
                .border_1()
                .border_color(p.raya)
                .rounded(px(8.))
                .px(px(10.))
                .py(px(4.))
                .cursor_pointer()
                .hover(|d| d.bg(p.fondo))
                .child(etiqueta)
        };
        let pie = div()
            .flex()
            .justify_between()
            .items_center()
            .gap(px(10.))
            .mt(px(10.))
            .pt(px(10.))
            .border_t_1()
            .border_color(p.raya)
            .text_size(px(12.))
            .text_color(p.tenue)
            .child(div().child(SharedString::from(self.datos.clone())))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(mini("copiar", "Copiar").on_click(cx.listener(|this, _, _, cx| {
                        let t = this.texto_en_vivo();
                        if !t.is_empty() {
                            cx.write_to_clipboard(ClipboardItem::new_string(t));
                        }
                    })))
                    .child(mini("limpiar", "Limpiar").on_click(cx.listener(|this, _, _, cx| {
                        this.cerrar_parrafo();
                        this.parrafos.clear();
                        this.datos.clear();
                        cx.notify();
                    }))),
            );

        let en_vivo = seccion().child(cadena).child(texto).child(pie);

        div()
            .id("raiz")
            .size_full()
            .bg(p.fondo)
            .text_color(p.tinta)
            .text_size(px(15.))
            .font_family(".SystemUIFont")
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .max_w(px(660.))
                    .mx_auto()
                    .px(px(16.))
                    .pt(px(28.))
                    .pb(px(40.))
                    .child(cabecera)
                    .children(aviso)
                    .child(escuchar)
                    .child(en_vivo),
            )
    }
}

impl Trujaman {
    /// Un desplegable sencillo: el valor elegido y, al pulsar, la lista encima de todo.
    fn desplegable(
        &self,
        cual: Lista,
        valor: String,
        bloqueado: bool,
        p: Paleta,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = match cual {
            Lista::Entrada => "lista-entrada",
            Lista::Salida => "lista-salida",
            Lista::Voz => "lista-voz",
        };
        let abierta = self.lista_abierta == Some(cual) && !bloqueado;
        let opciones: Vec<(String, String)> = match cual {
            Lista::Entrada => self.dispositivos.entradas.iter().map(etiqueta_dispositivo).collect(),
            Lista::Salida => self.dispositivos.salidas.iter().map(etiqueta_dispositivo).collect(),
            Lista::Voz => self.voces.iter().map(|v| (v.clone(), v.clone())).collect(),
        };

        let caja = div()
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
                    this.lista_abierta = if this.lista_abierta == Some(cual) { None } else { Some(cual) };
                    cx.notify();
                }
            }));

        let mut contenedor = div().relative().w_full().child(caja);
        if abierta {
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
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.lista_abierta = None;
                    cx.notify();
                }));
            if opciones.is_empty() {
                lista = lista.child(div().px(px(10.)).py(px(6.)).text_color(p.tenue).child("Nada disponible"));
            }
            for (i, (nombre, texto)) in opciones.into_iter().enumerate() {
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
                                match cual {
                                    Lista::Entrada => this.entrada = Some(nombre.clone()),
                                    Lista::Salida => {
                                        this.salida = Some(nombre.clone());
                                        this.cascos_manual = None;
                                    }
                                    Lista::Voz => this.voz = Some(nombre.clone()),
                                }
                                this.lista_abierta = None;
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        ),
                );
            }
            contenedor = contenedor.child(deferred(anchored().snap_to_window().child(lista)).with_priority(1));
        }
        contenedor
    }
}

fn etiqueta_dispositivo(d: &Dispositivo) -> (String, String) {
    let texto = if d.predeterminado { format!("{}  · predeterminado", d.nombre) } else { d.nombre.clone() };
    (d.nombre.clone(), texto)
}

fn casilla(
    id: &'static str,
    texto: &'static str,
    marcada: bool,
    bloqueada: bool,
    p: Paleta,
    al_pulsar: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
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
        .child(texto)
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(700.), px(780.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions { title: Some("Trujamán".into()), ..Default::default() }),
                ..Default::default()
            },
            |_, cx| cx.new(Trujaman::new),
        )
        .expect("no se puede abrir la ventana");
        cx.on_window_closed(|cx| cx.quit()).detach();
        cx.activate(true);
    });
}
