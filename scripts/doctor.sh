#!/usr/bin/env bash
# Revisa que el proyecto tiene todo lo que necesita: programas, modelos y locutor.
#   ./doctor.sh             dice qué falta y cómo arreglarlo; no toca nada
#   ./doctor.sh --arreglar  además descarga los modelos que falten y compila el locutor
# Se puede lanzar las veces que haga falta: lo que ya está, no lo repite.
# Los programas (brew, Xcode) no los instala: enseña el comando.
set -uo pipefail
RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
SOLO_MIRAR=1; [[ "${1:-}" == "--arreglar" ]] && SOLO_MIRAR=""
FALTA=0

ok()   { echo "  ✓ $1"; }
mal()  { echo "  ✗ $1"; echo "      → $2"; FALTA=1; }

echo "Programas"
for p in whisper-cli whisper-server; do
  command -v "$p" >/dev/null && ok "$p" || mal "$p" "brew install whisper-cpp"
done
command -v ffmpeg  >/dev/null && ok "ffmpeg"  || mal "ffmpeg"  "brew install ffmpeg"
command -v python3 >/dev/null && ok "python3 ($(python3 -c 'import platform;print(platform.python_version())'))" \
                              || mal "python3" "brew install python"
command -v swiftc  >/dev/null && ok "swiftc (para el locutor)" \
                              || mal "swiftc" "xcode-select --install"
command -v say     >/dev/null && ok "say (voces de macOS)" || mal "say" "viene con macOS"

echo "Modelos"
BASE="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"
mkdir -p "$RAIZ/modelos" "$RAIZ/audio"
for m in ggml-large-v3-turbo-q5_0.bin ggml-medium-q5_0.bin; do
  if [[ -s "$RAIZ/modelos/$m" ]]; then
    ok "$m"
  elif [[ -n "$SOLO_MIRAR" ]]; then
    mal "$m" "./doctor.sh --arreglar lo descarga (unos 550 MB)"
  else
    echo "  ↓ $m (unos 550 MB)…"
    if curl -L --fail --progress-bar -o "$RAIZ/modelos/$m.parcial" "$BASE/$m"; then
      mv "$RAIZ/modelos/$m.parcial" "$RAIZ/modelos/$m"; ok "$m"
    else
      rm -f "$RAIZ/modelos/$m.parcial"; mal "$m" "falló la descarga; vuelve a lanzar ./doctor.sh --arreglar"
    fi
  fi
done

echo "Locutor"
LOC="$RAIZ/scripts/locutor/locutor"
if [[ -x "$LOC" && ! "$RAIZ/scripts/locutor/locutor.swift" -nt "$LOC" ]]; then
  ok "compilado"
elif [[ -n "$SOLO_MIRAR" ]]; then
  mal "sin compilar" "./doctor.sh --arreglar lo compila"
elif command -v swiftc >/dev/null && swiftc -O "$RAIZ/scripts/locutor/locutor.swift" -o "$LOC" 2>/dev/null; then
  ok "compilado"
else
  mal "no se pudo compilar" "xcode-select --install"
fi

echo
if [[ $FALTA -eq 0 ]]; then
  echo "Todo listo. Prueba: scripts/dictar.sh 10"
else
  echo "Falta algo: mira las líneas con ✗."
fi
exit $FALTA
