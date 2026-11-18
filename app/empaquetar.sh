#!/usr/bin/env bash
# Compila Trujamán y lo deja como app de Mac en target/Trujaman.app, con su propio
# permiso de micrófono (la primera vez que escucha, macOS pregunta).
#
#   app/empaquetar.sh            compila y empaqueta
#   app/empaquetar.sh --abrir    además, la abre
#
# Los modelos no van dentro: la app los busca en la carpeta modelos/ del repo (o en
# TRUJAMAN_MODELOS). Firma ad hoc: vale para este Mac, no para repartirla.
set -euo pipefail
RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
cd "$RAIZ"

cargo build --release -p trujaman
# El icono sale de app/recursos/icono.svg.
rm -rf target/Trujaman.iconset
cargo run --release -q -p trujaman --example icono -- target/Trujaman.iconset >/dev/null

APP="$RAIZ/target/Trujaman.app"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
iconutil -c icns target/Trujaman.iconset -o "$APP/Contents/Resources/Trujaman.icns"
cp target/release/trujaman "$APP/Contents/MacOS/trujaman"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Trujamán</string>
  <key>CFBundleDisplayName</key><string>Trujamán</string>
  <key>CFBundleIdentifier</key><string>dev.trujaman.app</string>
  <key>CFBundleExecutable</key><string>trujaman</string>
  <key>CFBundleIconFile</key><string>Trujaman</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key>
  <string>Trujamán escucha el micrófono para transcribir o traducir lo que dices. El audio no sale de este Mac.</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$APP" >/dev/null
echo "Lista: $APP"

if [[ "${1:-}" == "--abrir" ]]; then
  open "$APP"
fi
