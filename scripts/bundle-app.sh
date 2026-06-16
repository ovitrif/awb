#!/bin/sh
# Wraps the awb-app binary into AWB.app (menu bar agent, no Dock icon).
# Usage: scripts/bundle-app.sh [app-binary] [output-dir] [icon-renderer]
#
# icon-renderer defaults to app-binary, but can be a host-architecture build so
# a cross-compiled release binary is never executed on the packaging runner.
set -eu

APP_BIN="${1:-target/release/awb-app}"
OUT_DIR="${2:-target/bundle}"
ICON_BIN="${3:-$APP_BIN}"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"

if [ ! -x "$APP_BIN" ]; then
  echo "app binary not found at ${APP_BIN}; build it with: cargo build --release -p awb-app" >&2
  exit 1
fi

if [ ! -x "$ICON_BIN" ]; then
  echo "icon renderer not found at ${ICON_BIN}" >&2
  exit 1
fi

APP="${OUT_DIR}/AWB.app"
rm -rf "$APP"
mkdir -p "${APP}/Contents/MacOS" "${APP}/Contents/Resources"

cp "$APP_BIN" "${APP}/Contents/MacOS/awb-app"

iconset="$(mktemp -d)/awb.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  "$ICON_BIN" --render-icon "${iconset}/icon_${size}x${size}.png" "$size" >/dev/null
  "$ICON_BIN" --render-icon "${iconset}/icon_${size}x${size}@2x.png" "$((size * 2))" >/dev/null
done
iconutil -c icns "$iconset" -o "${APP}/Contents/Resources/awb.icns"

cat > "${APP}/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>awb-app</string>
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
