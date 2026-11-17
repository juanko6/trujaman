//! Qué parte de la traducción ya es firme.
//!
//! Whisper, con el audio a medias, acierta el principio y se inventa el final. Así que
//! se comparan dos pasadas seguidas: las palabras iniciales en las que coinciden (menos
//! las `reserva` últimas, por prudencia) se dan por buenas y se dicen ya. Lo dicho no se
//! corrige; cuando callas, se traduce la frase entera y se dice lo que faltaba.
//!
//! Es la misma regla que `scripts/simultaneo.py`, sin audio ni Whisper, para poder
//! probarla sola.

/// Lo que Whisper suelta cuando solo hay ruido o silencio.
const INVENTADAS: &[&str] = &[
    "",
    ".",
    "you",
    "thank you.",
    "thanks for watching!",
    "thank you for watching.",
    "thank you very much.",
    "bye.",
    "subtitles by the amara.org community",
    "gracias.",
    "gracias por ver el video.",
    "gracias por ver el vídeo.",
    "¡gracias por ver el video!",
    "¡gracias por ver el vídeo!",
    "¡gracias!",
    "subtítulos realizados por la comunidad de amara.org",
];

/// Lo que sale de una pasada parcial.
#[derive(Debug, Default, PartialEq)]
pub struct Parcial {
    /// Trozo nuevo que ya es firme (se dice y se pinta en negro), si lo hay.
    pub firme: Option<String>,
    /// Lo que aún no es firme: se pinta en gris.
    pub provisional: String,
}

#[derive(Debug)]
pub struct Estabilizador {
    reserva: usize,
    min_trozo: usize,
    anterior: Vec<String>,
    dichas: usize,
}

impl Estabilizador {
    pub fn new(reserva: usize, min_trozo: usize) -> Self {
        Self { reserva, min_trozo, anterior: Vec::new(), dichas: 0 }
    }

    /// Palabras de la frase en curso que ya se han dado por firmes.
    pub fn dichas(&self) -> usize {
        self.dichas
    }

    /// Una pasada con el audio a medias.
    pub fn parcial(&mut self, texto: &str) -> Parcial {
        let actual: Vec<String> = texto.split_whitespace().map(str::to_owned).collect();
        let firmes = coinciden(&self.anterior, &actual).saturating_sub(self.reserva);
        let mut salida = Parcial::default();
        if firmes > self.dichas {
            let nuevas = &actual[self.dichas..firmes];
            // Cortar en la última puntuación si la hay; si no, esperar a un trozo decente.
            let mut corte =
                nuevas.iter().rposition(|p| p.ends_with([',', '.', ';', ':', '?', '!'])).map_or(0, |i| i + 1);
            if corte == 0 && nuevas.len() >= self.min_trozo {
                corte = nuevas.len();
            }
            if corte > 0 {
                salida.firme = Some(nuevas[..corte].join(" "));
                self.dichas += corte;
            }
        }
        salida.provisional = actual.get(self.dichas..).map(|r| r.join(" ")).unwrap_or_default();
        self.anterior = actual;
        salida
    }

    /// Al callar: la traducción de la frase entera. Devuelve lo que faltaba por decir y
    /// deja el estabilizador listo para la frase siguiente.
    pub fn cierre(&mut self, texto: &str) -> Option<String> {
        let final_: Vec<&str> = texto.split_whitespace().collect();
        let dichas = std::mem::take(&mut self.dichas);
        self.anterior.clear();
        if dichas == 0 && es_inventada(&final_.join(" ")) {
            return None;
        }
        let resto = final_.get(dichas..).unwrap_or_default();
        (!resto.is_empty()).then(|| resto.join(" "))
    }
}

/// Lo que Whisper devuelve con audio vacío o ruido.
pub fn es_inventada(texto: &str) -> bool {
    let t = texto.trim().to_lowercase();
    INVENTADAS.contains(&t.as_str())
}

/// Para comparar palabras entre pasadas sin que estorben mayúsculas ni comas.
fn clave(palabra: &str) -> String {
    palabra.chars().filter(|c| c.is_alphanumeric() || *c == '_').flat_map(char::to_lowercase).collect()
}

/// Cuántas palabras iniciales comparten dos traducciones.
fn coinciden(a: &[String], b: &[String]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| clave(x) == clave(y)).count()
}

/// Limpia lo que devuelve Whisper: fuera `[BLANK_AUDIO]`, `(music)` y los "..." de
/// frase a medias, y los espacios sobrantes.
pub fn limpiar(texto: &str) -> String {
    let mut sin_marcas = String::with_capacity(texto.len());
    let mut cierre: Option<char> = None;
    for c in texto.chars() {
        match (cierre, c) {
            (None, '[') => cierre = Some(']'),
            (None, '(') => cierre = Some(')'),
            (Some(fin), c) if c == fin => {
                cierre = None;
                sin_marcas.push(' ');
            }
            (Some(_), _) => {}
            (None, c) => sin_marcas.push(c),
        }
    }
    let mut limpio = String::with_capacity(sin_marcas.len());
    let mut puntos = 0;
    for c in sin_marcas.chars().chain(std::iter::once(' ')) {
        if c == '.' {
            puntos += 1;
            continue;
        }
        if puntos > 0 && puntos < 3 {
            limpio.extend(std::iter::repeat_n('.', puntos));
        }
        puntos = 0;
        limpio.push(c);
    }
    limpio.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn da_por_firme_lo_que_coincide_menos_la_reserva() {
        let mut e = Estabilizador::new(2, 3);
        let p = e.parcial("Tomorrow morning I have");
        assert_eq!(p.firme, None);
        assert_eq!(p.provisional, "Tomorrow morning I have");

        let p = e.parcial("Tomorrow morning I have to go to");
        // Coinciden 4; menos 2 de reserva quedan 2, menos que MIN_TROZO y sin coma.
        assert_eq!(p.firme, None);

        let p = e.parcial("Tomorrow morning I have to go to the bank");
        // Coinciden 7, firmes 5: "Tomorrow morning I have to".
        assert_eq!(p.firme.as_deref(), Some("Tomorrow morning I have to"));
        assert_eq!(p.provisional, "go to the bank");
        assert_eq!(e.dichas(), 5);
    }

    #[test]
    fn corta_en_la_ultima_puntuacion() {
        let mut e = Estabilizador::new(2, 3);
        e.parcial("Hello, good morning I need to");
        let p = e.parcial("Hello, good morning I need to check");
        // Firmes 4: "Hello, good morning I" → se corta tras "Hello,".
        assert_eq!(p.firme.as_deref(), Some("Hello,"));
        assert_eq!(p.provisional, "good morning I need to check");
    }

    #[test]
    fn la_comparacion_no_mira_mayusculas_ni_comas() {
        let mut e = Estabilizador::new(0, 1);
        e.parcial("hello world again");
        let p = e.parcial("Hello, world again");
        assert_eq!(p.firme.as_deref(), Some("Hello,"));
        assert_eq!(p.provisional, "world again");
    }

    #[test]
    fn el_cierre_dice_lo_que_faltaba() {
        let mut e = Estabilizador::new(2, 3);
        e.parcial("Tomorrow morning I have to go");
        e.parcial("Tomorrow morning I have to go to the");
        assert_eq!(e.dichas(), 4);
        let resto = e.cierre("Tomorrow morning I have to go to the bank.");
        assert_eq!(resto.as_deref(), Some("to go to the bank."));
        assert_eq!(e.dichas(), 0);
    }

    #[test]
    fn el_cierre_descarta_lo_inventado() {
        let mut e = Estabilizador::new(2, 3);
        assert_eq!(e.cierre("Thank you."), None);
        assert_eq!(e.cierre("Gracias por ver el video."), None);
    }

    #[test]
    fn limpia_marcas_y_puntos_suspensivos() {
        assert_eq!(limpiar(" [BLANK_AUDIO] Hola (música)  qué tal... "), "Hola qué tal");
        assert_eq!(limpiar("Hola. Adiós."), "Hola. Adiós.");
        assert_eq!(limpiar("y luego..."), "y luego");
    }
}
