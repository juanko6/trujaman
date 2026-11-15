#!/usr/bin/env bash
# Levanta trujamán con su interfaz web y abre el navegador:
#   ./servidor.sh                 → http://127.0.0.1:8178
#   PUERTO=9000 ./servidor.sh     otro puerto
#   NO_ABRIR=1 ./servidor.sh      sin abrir el navegador (solo la API)
#   NO_CERRAR=1 ./servidor.sh     no se apaga al cerrar la pestaña
#
# Se apaga solo unos segundos después de cerrar la pestaña, o con el botón Cerrar.
#
# La misma dirección sigue dando servicio a otras apps:
#   curl -F file=@nota.m4a http://127.0.0.1:8178/inference              transcribe
#   curl -F file=@nota.m4a "http://127.0.0.1:8178/inference?ingles=1"   traduce al inglés
# Con INGLES=1, /inference traduce sin tener que pedirlo.
#
# El trabajo lo hace ui.py; esto prepara el entorno.
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
export PUERTO="${PUERTO:-8178}" IDIOMA="${IDIOMA:-es}"
export LOCUTOR="$DIR/locutor/locutor"

if [[ ! -x "$LOCUTOR" || "$DIR/locutor/locutor.swift" -nt "$LOCUTOR" ]]; then
  echo "Compilando el locutor (solo la primera vez)…" >&2
  swiftc -O "$DIR/locutor/locutor.swift" -o "$LOCUTOR" 2>/dev/null \
    || echo "No se pudo compilar el locutor: scripts/doctor.sh" >&2
fi

exec python3 "$DIR/ui.py"
