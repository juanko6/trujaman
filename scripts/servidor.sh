#!/usr/bin/env bash
# Levanta Whisper como servicio HTTP local para que lo use cualquier app.
#   curl -F file=@nota.m4a http://127.0.0.1:8178/inference
# Con INGLES=1 arranca con el modelo que traduce; traduce todas las
# peticiones sin pedir nada más. Puede convivir con el otro en otro puerto:
#   INGLES=1 PUERTO=8179 ./servidor.sh
set -euo pipefail
source "$(dirname "$0")/comun.sh"
echo "Whisper escuchando en http://127.0.0.1:$PUERTO/inference" >&2
exec whisper-server -m "$MODELO" -l "$IDIOMA" --host 127.0.0.1 --port "$PUERTO" --convert -bs 5 ${INGLES:+--translate}
