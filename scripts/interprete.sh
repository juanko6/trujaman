#!/usr/bin/env bash
# Intérprete en tiempo real: hablas en español y oyes el inglés.
#   AURICULARES=1 ./interprete.sh   con cascos: traduce MIENTRAS hablas (simultáneo)
#   ./interprete.sh                 con altavoces: frase a frase, en cada pausa
#   SIN_VOZ=1 ./interprete.sh       solo subtítulos en pantalla (simultáneo)
# Ctrl+C para salir.
#
# Con altavoces no puede ser simultáneo: el micro oiría la traducción, así que se
# silencia mientras el locutor habla y lo que digas encima se pierde.
#
# El trabajo lo hace simultaneo.py (ahí está explicado); esto prepara el entorno.
# Ajustes: PAUSA, UMBRAL, INTERVALO, RESERVA, MIN_TROZO, MICRO, VOZ, VOCABULARIO.
set -euo pipefail
INGLES=1
source "$(dirname "$0")/comun.sh"
DIR="$(cd "$(dirname "$0")" && pwd)"

export MODELO IDIOMA MICRO VOZ VOCABULARIO
export LOCUTOR="$DIR/locutor/locutor"

if [[ -z "${SIN_VOZ:-}" && ( ! -x "$LOCUTOR" || "$DIR/locutor/locutor.swift" -nt "$LOCUTOR" ) ]]; then
  echo "Compilando el locutor (solo la primera vez)…" >&2
  swiftc -O "$DIR/locutor/locutor.swift" -o "$LOCUTOR" 2>/dev/null
fi

exec python3 "$DIR/simultaneo.py"
