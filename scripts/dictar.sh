#!/usr/bin/env bash
# Graba y transcribe de una vez. El texto queda también en el portapapeles.
#   ./dictar.sh       hasta que pulses q
#   ./dictar.sh 15    15 segundos
#   INGLES=1 ./dictar.sh 15   y lo traduce al inglés
#   INGLES=1 HABLAR=1 ./dictar.sh 15   y además lo lee en alto
set -euo pipefail
DIR="$(dirname "$0")"
AUDIO="$("$DIR/grabar.sh" "${1:-}")"
echo "Transcribiendo…" >&2
TEXTO="$("$DIR/transcribir.sh" "$AUDIO")"
echo "$TEXTO"
printf '%s' "$TEXTO" | sed 's/^ *//' | pbcopy
echo "(copiado al portapapeles)" >&2
[[ -n "${HABLAR:-}" ]] && printf '%s' "$TEXTO" | "$DIR/hablar.sh"
true
