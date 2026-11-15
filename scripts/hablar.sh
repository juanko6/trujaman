#!/usr/bin/env bash
# Lee un texto en alto por los altavoces con las voces de macOS.
#   ./hablar.sh "Hello there"
#   ./hablar.sh nota.en.txt
#   echo "Hola" | ./hablar.sh
#   VOZ=Daniel VELOCIDAD=160 ./hablar.sh "Hello"
set -euo pipefail
source "$(dirname "$0")/comun.sh"

if [[ $# -eq 0 ]]; then TEXTO="$(cat)"
elif [[ -f "$1" ]]; then TEXTO="$(cat "$1")"
else TEXTO="$*"
fi

say -v "$VOZ" ${VELOCIDAD:+-r "$VELOCIDAD"} -- "$TEXTO"
