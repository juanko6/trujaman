#!/usr/bin/env bash
# Transcribe cualquier audio o vídeo (wav, mp3, m4a, mp4, ogg…).
# Deja el texto por pantalla y en un .txt junto al audio (.en.txt si traduce).
#   INGLES=1 ./transcribir.sh nota.m4a   traduce al inglés
#   ./transcribir.sh nota.m4a
set -euo pipefail
source "$(dirname "$0")/comun.sh"

ENTRADA="${1:?Uso: ./transcribir.sh <audio>}"
BASE="${ENTRADA%.*}${INGLES:+.en}"
TMP="$(mktemp -t whisper).wav"
trap 'rm -f "$TMP"' EXIT

# Whisper solo acepta WAV 16 kHz mono: ffmpeg convierte lo que sea.
ffmpeg -hide_banner -loglevel error -i "$ENTRADA" -ar 16000 -ac 1 -c:a pcm_s16le -y "$TMP"

whisper-cli -m "$MODELO" -l "$IDIOMA" ${VOCABULARIO:+--prompt "$VOCABULARIO"} ${INGLES:+-tr} -f "$TMP" -nt -np -otxt -of "$BASE" >/dev/null 2>&1
cat "$BASE.txt"
