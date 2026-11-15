#!/usr/bin/env bash
# Graba del micrófono en WAV 16 kHz mono, el formato que pide Whisper.
#   ./grabar.sh            graba hasta que pulses q
#   ./grabar.sh 10         graba 10 segundos
#   ./grabar.sh 10 x.wav   y lo guarda en x.wav
#   ./grabar.sh --lista    enseña los micrófonos disponibles
set -euo pipefail
source "$(dirname "$0")/comun.sh"

if [[ "${1:-}" == "--lista" ]]; then
  ffmpeg -hide_banner -f avfoundation -list_devices true -i "" 2>&1 | grep -A20 "audio devices" || true
  exit 0
fi

DURACION="${1:-}"
SALIDA="${2:-$RAIZ/audio/grabacion-$(date +%Y%m%d-%H%M%S).wav}"
LIMITE=()
[[ -n "$DURACION" ]] && LIMITE=(-t "$DURACION")

echo "Grabando${DURACION:+ $DURACION s}… (q para parar)" >&2
ffmpeg -hide_banner -loglevel error -f avfoundation -i "$MICRO" ${LIMITE[@]+"${LIMITE[@]}"} \
  -ar 16000 -ac 1 -c:a pcm_s16le -y "$SALIDA"

# Si macOS no ha dado permiso de micrófono, ffmpeg graba ceros sin quejarse
# y Whisper se inventa frases ("Gracias por ver el video").
PICO="$(ffmpeg -hide_banner -i "$SALIDA" -af volumedetect -f null - 2>&1 | sed -n 's/.*max_volume: \(-\{0,1\}[0-9]*\).*/\1/p')"
if [[ -n "$PICO" && "$PICO" -le -80 ]]; then
  echo "La grabación está en silencio ($PICO dB). Revisa el permiso de micrófono de la app" >&2
  echo "desde la que lanzas esto (Ajustes del Sistema → Privacidad y seguridad → Micrófono)," >&2
  echo "o elige otro micro con MICRO=\":N\" (./grabar.sh --lista)." >&2
  exit 1
fi
echo "$SALIDA"
