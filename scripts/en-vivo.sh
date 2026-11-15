#!/usr/bin/env bash
# Transcripción en directo mientras hablas (Ctrl+C para salir).
# Va por trozos: menos preciso que grabar y transcribir el audio entero.
set -euo pipefail
source "$(dirname "$0")/comun.sh"
whisper-stream -m "$MODELO" -l "$IDIOMA" --step 3000 --length 8000 -t 8 -c "${CAPTURA:--1}"
