#!/bin/sh
# Wraps the awb-tray binary into AWB.app (menu bar agent, no Dock icon).
# Usage: scripts/bundle-app.sh [tray-binary] [output-dir]
set -eu

TRAY_BIN="${1:-target/release/awb-tray}"
OUT_DIR="${2:-target/bundle}"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"

if [ ! -x "$TRAY_BIN" ]; then
  echo "tray binary not found at ${TRAY_BIN}; build it with: cargo build --release -p awb-tray" >&2
  exit 1
fi

APP="${OUT_DIR}/AWB.app"
rm -rf "$APP"
mkdir -p "${APP}/Contents/MacOS" "${APP}/Contents/Resources"

cp "$TRAY_BIN" "${APP}/Contents/MacOS/awb-tray"

iconset="$(mktemp -d)/awb.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  "$TRAY_BIN" --render-icon "${iconset}/icon_${size}x${size}.png" "$size" >/dev/null
  "$TRAY_BIN" --render-icon "${iconset}/icon_${size}x${size}@2x.png" "$((size * 2))" >/dev/null
done
iconutil -c icns "$iconset" -o "${APP}/Contents/Resources/awb.icns"

cat > "${APP}/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>awb-tray</string>
  <key>CFBundleIconFile</key><string>awb</string>
  <key>CFBundleIdentifier</key><string>com.ovitrif.awb</string>
  <key>CFBundleName</key><string>AWB</string>
  <key>CFBundleDisplayName</key><string>AWB</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --sign - "$APP" 2>/dev/null || true

echo "Bundled ${APP} (v${VERSION})"
