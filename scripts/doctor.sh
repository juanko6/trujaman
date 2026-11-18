#!/usr/bin/env bash
# Revisa que el proyecto tiene todo lo que necesita: programas, modelos y locutor.
#   ./doctor.sh             dice qué falta y cómo arreglarlo; no toca nada
#   ./doctor.sh --arreglar  además descarga los modelos que falten y compila el locutor
#   ./doctor.sh --app       revisa lo que hace falta para compilar la app de Mac (app/)
#                           en vez de lo de la interfaz web; se combina con --arreglar
# Se puede lanzar las veces que haga falta: lo que ya está, no lo repite.
# Los programas (brew, Xcode, Rust) no los instala: enseña el comando.
set -uo pipefail
RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
SOLO_MIRAR=1; APP=""
for a in "$@"; do
  case "$a" in
    --arreglar) SOLO_MIRAR="" ;;
    --app) APP=1 ;;
    *) echo "Uso: scripts/doctor.sh [--arreglar] [--app]"; exit 2 ;;
  esac
done
FALTA=0

ok()   { echo "  ✓ $1"; }
mal()  { echo "  ✗ $1"; echo "      → $2"; FALTA=1; }

echo "Programas"
if [[ -n "$APP" ]]; then
  # whisper.cpp se compila dentro de la app: hace falta el compilador de C++ de Xcode y
  # cmake; el icono se hace con iconutil. La versión mínima de Rust es la de cpal.
  xcode-select -p >/dev/null 2>&1 && command -v c++ >/dev/null \
    && ok "herramientas de Xcode" || mal "herramientas de Xcode" "xcode-select --install"
  if command -v cargo >/dev/null; then
    V="$(rustc --version 2>/dev/null | awk '{print $2}')"
    if [[ "$(printf '%s\n1.85.0\n' "$V" | sort -V | head -1)" == "1.85.0" ]]; then
      ok "Rust $V"
    else
      mal "Rust $V (hace falta 1.85 o más)" "rustup update"
    fi
  else
    mal "Rust" "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  fi
  command -v cmake    >/dev/null && ok "cmake"    || mal "cmake" "brew install cmake"
  command -v iconutil >/dev/null && ok "iconutil" || mal "iconutil" "viene con macOS"
else
  for p in whisper-cli whisper-server; do
    command -v "$p" >/dev/null && ok "$p" || mal "$p" "brew install whisper-cpp"
  done
  command -v ffmpeg  >/dev/null && ok "ffmpeg"  || mal "ffmpeg"  "brew install ffmpeg"
  command -v python3 >/dev/null && ok "python3 ($(python3 -c 'import platform;print(platform.python_version())'))" \
                                || mal "python3" "brew install python"
  command -v swiftc  >/dev/null && ok "swiftc (para el locutor)" \
                                || mal "swiftc" "xcode-select --install"
  command -v say     >/dev/null && ok "say (voces de macOS)" || mal "say" "viene con macOS"
fi

echo "Modelos"
BASE="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"
mkdir -p "$RAIZ/modelos" "$RAIZ/audio"
for m in ggml-large-v3-turbo-q5_0.bin ggml-medium-q5_0.bin; do
  if [[ -s "$RAIZ/modelos/$m" ]]; then
    ok "$m"
  elif [[ -n "$SOLO_MIRAR" ]]; then
    mal "$m" "scripts/doctor.sh --arreglar${APP:+ --app} lo descarga (unos 550 MB)"
  else
    echo "  ↓ $m (unos 550 MB)…"
    if curl -L --fail --progress-bar -o "$RAIZ/modelos/$m.parcial" "$BASE/$m"; then
      mv "$RAIZ/modelos/$m.parcial" "$RAIZ/modelos/$m"; ok "$m"
    else
      rm -f "$RAIZ/modelos/$m.parcial"; mal "$m" "falló la descarga; vuelve a lanzar scripts/doctor.sh --arreglar${APP:+ --app}"
    fi
  fi
done

if [[ -z "$APP" ]]; then
  echo "Locutor"
  LOC="$RAIZ/scripts/locutor/locutor"
  if [[ -x "$LOC" && ! "$RAIZ/scripts/locutor/locutor.swift" -nt "$LOC" ]]; then
    ok "compilado"
  elif [[ -n "$SOLO_MIRAR" ]]; then
    mal "sin compilar" "scripts/doctor.sh --arreglar lo compila"
  elif command -v swiftc >/dev/null && swiftc -O "$RAIZ/scripts/locutor/locutor.swift" -o "$LOC" 2>/dev/null; then
    ok "compilado"
  else
    mal "no se pudo compilar" "xcode-select --install"
  fi

  echo "Interfaz"
  for f in ui.py ui.html simultaneo.py; do
    [[ -s "$RAIZ/scripts/$f" ]] && ok "$f" || mal "$f" "falta el fichero: git checkout scripts/$f"
  done
fi

echo
if [[ $FALTA -eq 0 ]]; then
  if [[ -n "$APP" ]]; then
    echo "Todo listo. Compila y abre la app: app/empaquetar.sh --abrir"
  else
    echo "Todo listo. Abre la interfaz: scripts/servidor.sh (o doble clic en Trujaman.command)"
  fi
else
  echo "Falta algo: mira las líneas con ✗."
fi
exit $FALTA
