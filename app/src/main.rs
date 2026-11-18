//! Trujamán como app de escritorio: la interfaz de `scripts/ui.html` hecha con GPUI.
//!
//! El núcleo (crate `nucleo`) escucha en su propio hilo y cuenta lo que pasa por un
//! canal; aquí solo se pinta.

mod comun;
mod preferencias;
mod ventanas;

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    actions, div, prelude::*, px, App, Application, ClipboardItem, Context, FontWeight, HighlightStyle, KeyBinding,
    Menu, MenuItem, ScrollHandle, SharedString, StyledText, Subscription, SystemMenuType, Window,
};
use nucleo::audio::{self, Dispositivo, Dispositivos};
use nucleo::voz::Locutor;
use nucleo::whisper::{carpeta_modelos, Modelos};
use nucleo::{Ajustes, Evento, Fuente, Sesion};

use comun::{casilla, desplegable, etiqueta_dispositivo, logotipo, paleta, segmentado, Paleta, ALTO_BARRA, WEB};
use preferencias::Preferencias;

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
    /// Solo el modelo del modo en que está: al cambiar de modo se suelta y se carga el otro.
    modelos: Option<Arc<Modelos>>,
    estado: EstadoModelos,
    /// Cuenta las cargas, para descartar una que acaba cuando ya se ha cambiado de modo.
    carga_id: u64,
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
    /// Las preferencias ya aplicadas, para ver qué cambia cuando se tocan en Ajustes.
    prefs: Preferencias,
    _al_cambiar_prefs: Subscription,
}

impl Trujaman {
    fn new(cx: &mut Context<Self>) -> Self {
        let prefs = cx.global::<Preferencias>().clone();
        let mut yo = Self {
            modelos: None,
            estado: EstadoModelos::Cargando,
            carga_id: 0,
            dispositivos: audio::dispositivos(),
            voces: Vec::new(),
            entrada: prefs.entrada.clone(),
            salida: prefs.salida.clone(),
            voz: prefs.voz.clone(),
            traducir: prefs.traducir,
            hablar: prefs.hablar,
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
            prefs,
            _al_cambiar_prefs: cx.observe_global::<Preferencias>(|this, cx| this.aplicar_prefs(cx)),
        };

        yo.cargar_modelo(depurar(), cx);

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
                        this.voz = this.voz_por_defecto();
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

    /// La voz guardada en Ajustes si sigue instalada; si no, Samantha o la primera.
    fn voz_por_defecto(&self) -> Option<String> {
        let guardada = self.prefs.voz.clone().filter(|v| self.voces.contains(v));
        let samantha = self.voces.iter().find(|n| n.starts_with("Samantha")).cloned();
        guardada.or(samantha).or_else(|| self.voces.first().cloned())
    }

    /// Lo que se cambia en Ajustes pasa a la ventana (si no está escuchando: lo que
    /// suena no cambia a media sesión).
    fn aplicar_prefs(&mut self, cx: &mut Context<Self>) {
        let nuevas = cx.global::<Preferencias>().clone();
        if self.sesion.is_none() {
            let antes = &self.prefs;
            if nuevas.entrada != antes.entrada {
                self.entrada = nuevas.entrada.clone();
            }
            if nuevas.salida != antes.salida {
                self.salida = nuevas.salida.clone();
                self.cascos_manual = None;
            }
            if nuevas.traducir != antes.traducir {
                self.traducir = nuevas.traducir;
            }
            if nuevas.hablar != antes.hablar {
                self.hablar = nuevas.hablar;
            }
            let cambia_voz = nuevas.voz != antes.voz;
            self.prefs = nuevas;
            if cambia_voz {
                self.voz = self.voz_por_defecto();
            }
            // Si ha cambiado el modo, el modelo que toca (no hace nada si ya está).
            self.cargar_modelo(false, cx);
        } else {
            self.prefs = nuevas;
        }
        cx.notify();
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

    /// Carga, fuera del hilo de la interfaz, el modelo del modo actual si no es el que
    /// hay. El otro se suelta antes, para no tener los dos en memoria a la vez.
    /// `empezar`: con `TRUJAMAN_DEPURAR`, se pone a escuchar en cuanto está.
    fn cargar_modelo(&mut self, empezar: bool, cx: &mut Context<Self>) {
        if self.listo() {
            return;
        }
        self.modelos = None;
        self.estado = EstadoModelos::Cargando;
        self.carga_id += 1;
        let (id, traducir) = (self.carga_id, self.traducir);
        cx.spawn(async move |this, cx| {
            let carpeta = carpeta_modelos();
            let cargado = cx.background_executor().spawn(async move { Modelos::cargar_para(&carpeta, traducir) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.carga_id != id {
                    return;
                }
                match cargado {
                    Ok(modelos) => {
                        this.modelos = Some(Arc::new(modelos));
                        this.estado = EstadoModelos::Listo;
                        if empezar {
                            this.entrada = std::env::var("TRUJAMAN_MICRO").ok();
                            if let Ok(c) = std::env::var("TRUJAMAN_CASCOS") {
                                this.cascos_manual = Some(c != "0");
                            }
                            eprintln!(
                                "[trujamán] modelo listo; empiezo a escuchar ({:?}, cascos {})",
                                this.entrada,
                                this.cascos()
                            );
                            this.alternar_escucha(cx);
                        }
                    }
                    Err(e) => this.estado = EstadoModelos::Mal(e),
                }
                cx.notify();
            });
        })
        .detach();
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
        let p = paleta(window, cx);
        let escuchando = self.sesion.is_some();

        let (clase, texto_estado) = match &self.estado {
            EstadoModelos::Cargando => (
                p.tenue,
                if self.traducir {
                    "Cargando el modelo de traducción…"
                } else {
                    "Cargando el modelo de transcripción…"
                }
                .to_string(),
            ),
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
            .child(logotipo(34., 30., p))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(13.))
                    .text_color(p.tenue)
                    .child(div().size(px(8.)).rounded_full().bg(clase))
                    .child(texto_estado)
                    .child(
                        div()
                            .id("abrir-ajustes")
                            .ml(px(6.))
                            .px(px(8.))
                            .py(px(3.))
                            .rounded(px(7.))
                            .border_1()
                            .border_color(p.raya)
                            .text_color(p.tinta)
                            .cursor_pointer()
                            .hover(|d| d.bg(p.papel))
                            .child("Ajustes")
                            .on_click(|_, _, cx| ventanas::abrir_ajustes(cx)),
                    ),
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

        let modo = div().mt(px(14.)).child(segmentado(
            "modo",
            &[(true, "Traducir al inglés"), (false, "Solo transcribir")],
            self.traducir,
            p,
            cx,
            |this: &mut Self, traducir, cx| {
                if this.sesion.is_some() {
                    this.aviso = Some("Para de escuchar antes de cambiar de modo.".into());
                } else {
                    this.traducir = traducir;
                    this.cargar_modelo(false, cx);
                }
            },
        ));

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
            .flex_1()
            .min_h(px(80.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .font_family(self.prefs.letra.familia())
            .text_size(px(self.prefs.tamano.puntos()))
            .line_height(px(self.prefs.tamano.puntos() * 1.5));
        if self.parrafos.is_empty() {
            texto = texto.child(
                div()
                    .text_size(px(15.))
                    .font_family(".SystemUIFont")
                    .text_color(p.tenue)
                    .child("Pulsa Escuchar y habla en español."),
            );
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
            .flex_none()
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

        // La tarjeta en vivo ocupa el alto que queda: el texto se desplaza dentro de ella y
        // el pie (con su borde) queda siempre a la vista.
        let en_vivo = seccion().flex_1().min_h_0().flex().flex_col().child(cadena).child(texto).child(pie);

        div()
            .relative()
            .size_full()
            .bg(p.fondo)
            .text_color(p.tinta)
            .text_size(px(15.))
            .font_family(".SystemUIFont")
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .px(px(16.))
                    .pt(px(ALTO_BARRA + 8.))
                    .pb(px(16.))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(660.))
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .child(cabecera)
                            .children(aviso)
                            .child(escuchar.flex_none())
                            .child(en_vivo),
                    ),
            )
            // La franja de los botones de la ventana, del color del fondo.
            .child(div().absolute().top_0().left_0().w_full().child(comun::barra(None, false, p)))
    }
}

impl Trujaman {
    fn desplegable(
        &self,
        cual: Lista,
        valor: String,
        bloqueado: bool,
        p: Paleta,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = match cual {
            Lista::Entrada => "lista-entrada",
            Lista::Salida => "lista-salida",
            Lista::Voz => "lista-voz",
        };
        let opciones = match cual {
            Lista::Entrada => self.dispositivos.entradas.iter().map(etiqueta_dispositivo).collect(),
            Lista::Salida => self.dispositivos.salidas.iter().map(etiqueta_dispositivo).collect(),
            Lista::Voz => self.voces.iter().map(|v| (Some(v.clone()), v.clone())).collect(),
        };
        desplegable(
            id,
            valor,
            opciones,
            self.lista_abierta == Some(cual),
            bloqueado,
            p,
            cx,
            move |this: &mut Self, abrir| this.lista_abierta = abrir.then_some(cual),
            move |this: &mut Self, valor, _| match cual {
                Lista::Entrada => this.entrada = valor,
                Lista::Salida => {
                    this.salida = valor;
                    this.cascos_manual = None;
                }
                Lista::Voz => this.voz = valor,
            },
        )
    }
}

actions!(trujaman, [AcercaDe, AbrirAjustes, Ocultar, OcultarOtras, MostrarTodas, Salir, Minimizar, Cerrar, Web]);

fn menus(cx: &mut App) {
    cx.on_action(|_: &AcercaDe, cx| ventanas::abrir_acerca(cx));
    cx.on_action(|_: &AbrirAjustes, cx| ventanas::abrir_ajustes(cx));
    cx.on_action(|_: &Ocultar, cx| cx.hide());
    cx.on_action(|_: &OcultarOtras, cx| cx.hide_other_apps());
    cx.on_action(|_: &MostrarTodas, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Salir, cx| cx.quit());
    cx.on_action(|_: &Web, cx| cx.open_url(WEB));
    cx.on_action(|_: &Minimizar, cx| {
        if let Some(v) = cx.active_window() {
            let _ = v.update(cx, |_, window, _| window.minimize_window());
        }
    });
    cx.on_action(|_: &Cerrar, cx| {
        if let Some(v) = cx.active_window() {
            let _ = v.update(cx, |_, window, _| window.remove_window());
        }
    });
    cx.bind_keys([
        KeyBinding::new("cmd-,", AbrirAjustes, None),
        KeyBinding::new("cmd-h", Ocultar, None),
        KeyBinding::new("alt-cmd-h", OcultarOtras, None),
        KeyBinding::new("cmd-q", Salir, None),
        KeyBinding::new("cmd-m", Minimizar, None),
        KeyBinding::new("cmd-w", Cerrar, None),
    ]);
    cx.set_menus(vec![
        Menu {
            name: "Trujamán".into(),
            items: vec![
                MenuItem::action("Acerca de Trujamán", AcercaDe),
                MenuItem::separator(),
                MenuItem::action("Ajustes…", AbrirAjustes),
                MenuItem::separator(),
                MenuItem::os_submenu("Servicios", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Ocultar Trujamán", Ocultar),
                MenuItem::action("Ocultar otras", OcultarOtras),
                MenuItem::action("Mostrar todas", MostrarTodas),
                MenuItem::separator(),
                MenuItem::action("Salir de Trujamán", Salir),
            ],
        },
        Menu {
            name: "Ventana".into(),
            items: vec![MenuItem::action("Minimizar", Minimizar), MenuItem::action("Cerrar", Cerrar)],
        },
        Menu { name: "Ayuda".into(), items: vec![MenuItem::action("Trujamán en GitHub", Web)] },
    ]);
}

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.set_global(Preferencias::cargar());
        menus(cx);
        let principal = cx
            .open_window(ventanas::opciones_ventana("Trujamán", 700., 800., true, cx), |_, cx| cx.new(Trujaman::new))
            .expect("no se puede abrir la ventana");
        // Cerrar la ventana principal cierra la app; Ajustes y Acerca de, no.
        cx.on_window_closed(move |cx| {
            if !cx.windows().iter().any(|w| w.window_id() == principal.window_id()) {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
