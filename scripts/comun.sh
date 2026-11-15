# Variables compartidas por los scripts. Se cambian aquí o desde fuera:
#   MODELO=../modelos/otro.bin IDIOMA=en ./transcribir.sh audio.m4a
RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# INGLES=1 traduce al inglés en vez de transcribir. El modelo turbo no sabe
# traducir (devuelve el texto en el idioma original), así que se usa medium.
INGLES="${INGLES:-}"
if [[ -n "$INGLES" ]]; then
  MODELO="${MODELO:-$RAIZ/modelos/ggml-medium-q5_0.bin}"
else
  MODELO="${MODELO:-$RAIZ/modelos/ggml-large-v3-turbo-q5_0.bin}"
fi
IDIOMA="${IDIOMA:-es}"       # es, en, auto...
# Micrófono. ":default" = el que tengas elegido en macOS (Ajustes → Sonido → Entrada).
# También por nombre, MICRO=":MacBook Pro Microphone", o por número, MICRO=":2"
# (./grabar.sh --lista los enseña; los números cambian al conectar dispositivos).
MICRO="${MICRO:-:default}"
PUERTO="${PUERTO:-8178}"
VOCABULARIO="${VOCABULARIO:-}"   # nombres propios que suele fallar: "Whisper, Kubernetes, LinkedIn"
# Voz para leer en alto (say -v '?' las lista). Por defecto, según el idioma de salida.
if [[ -n "$INGLES" ]]; then VOZ="${VOZ:-Samantha}"; else VOZ="${VOZ:-Mónica}"; fi
VELOCIDAD="${VELOCIDAD:-}"   # palabras por minuto; vacío = la de la voz (~180)
