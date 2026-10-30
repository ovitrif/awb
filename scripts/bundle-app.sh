#!/bin/sh
# Wraps the awb-app binary into Android Wifi Bridge.app (menu bar agent, no Dock icon).
# Usage: scripts/bundle-app.sh [app-binary] [output-dir] [icon-renderer]
#
# Bundles the patched scrcpy from scripts/build-scrcpy.sh when it has been
# built (target/scrcpy, or $SCRCPY_DIR); otherwise awb uses scrcpy from PATH.
#
# icon-renderer defaults to app-binary, but can be a host-architecture build so
# a cross-compiled release binary is never executed on the packaging runner.
set -eu

APP_BIN="${1:-target/release/awb-app}"
OUT_DIR="${2:-target/bundle}"
ICON_BIN="${3:-$APP_BIN}"
SCRCPY_DIR="${SCRCPY_DIR:-target/scrcpy}"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"

if [ ! -x "$APP_BIN" ]; then
  echo "app binary not found at ${APP_BIN}; build it with: cargo build --release -p awb-app" >&2
  exit 1
fi

if [ ! -x "$ICON_BIN" ]; then
  echo "icon renderer not found at ${ICON_BIN}" >&2
  exit 1
fi

APP="${OUT_DIR}/Android Wifi Bridge.app"
LEGACY_APP="${OUT_DIR}/AWB.app"
rm -rf "$APP" "$LEGACY_APP"
mkdir -p "${APP}/Contents/MacOS" "${APP}/Contents/Resources"

cp "$APP_BIN" "${APP}/Contents/MacOS/awb-app"

# Liquid Glass icon: actool compiles the Icon Composer bundle into Assets.car
# (light, dark, clear and tinted variants) plus AppIcon.icns for older macOS.
# Without an actool that understands .icon files, fall back to the flat icon.
ICON_SOURCE="$(dirname "$0")/../crates/awb-app/assets/AppIcon.icon"
ICON_NAME="AppIcon"
ICON_PLIST_KEYS="<key>CFBundleIconName</key><string>AppIcon</string>"
if ! xcrun actool "$ICON_SOURCE" --compile "${APP}/Contents/Resources" \
  --platform macosx --minimum-deployment-target 11.0 --app-icon AppIcon \
  --output-partial-info-plist "$(mktemp -d)/icon.plist" >/dev/null 2>&1 \
  || [ ! -f "${APP}/Contents/Resources/Assets.car" ] \
  || [ ! -f "${APP}/Contents/Resources/AppIcon.icns" ]; then
  echo "actool could not compile ${ICON_SOURCE}; using the flat icon" >&2
  rm -f "${APP}/Contents/Resources/Assets.car" "${APP}/Contents/Resources/AppIcon.icns"
  ICON_NAME="awb"
  ICON_PLIST_KEYS=""
  iconset="$(mktemp -d)/awb.iconset"
  mkdir -p "$iconset"
  for size in 16 32 128 256 512; do
    "$ICON_BIN" --render-icon "${iconset}/icon_${size}x${size}.png" "$size" >/dev/null
    "$ICON_BIN" --render-icon "${iconset}/icon_${size}x${size}@2x.png" "$((size * 2))" >/dev/null
  done
  iconutil -c icns "$iconset" -o "${APP}/Contents/Resources/awb.icns"
fi

# The patched scrcpy runs as its own app, so the mirror window belongs to a
# process with a bundle id that window managers (Raycast layouts) can target
if [ -x "${SCRCPY_DIR}/scrcpy" ] \
  && [ "$(lipo -archs "${SCRCPY_DIR}/scrcpy")" != "$(lipo -archs "$APP_BIN")" ]; then
  echo "skipping ${SCRCPY_DIR}/scrcpy: not built for the app's architecture" >&2
elif [ -x "${SCRCPY_DIR}/scrcpy" ]; then
  MIRROR="${APP}/Contents/Helpers/Android Mirror.app"
  mkdir -p "${MIRROR}/Contents/MacOS" "${MIRROR}/Contents/Resources"
  cp "${SCRCPY_DIR}/scrcpy" "${MIRROR}/Contents/MacOS/scrcpy"
  cp "${SCRCPY_DIR}/scrcpy-server" "${SCRCPY_DIR}/scrcpy.png" \
    "${SCRCPY_DIR}/disconnected.png" "${MIRROR}/Contents/Resources/"
  cp "${APP}/Contents/Resources/${ICON_NAME}.icns" "${MIRROR}/Contents/Resources/AppIcon.icns"

  # awb starts mirrors through this launcher; opened with no arguments (Finder,
  # Raycast, Spotlight) it quits, since only awb knows which phone to mirror.
  # The patched scrcpy links Homebrew's ffmpeg, sdl3 and libusb; when it cannot
  # load them, the launcher falls back to the scrcpy on PATH.
  cat > "${MIRROR}/Contents/MacOS/Android Mirror" <<'LAUNCHER'
#!/bin/sh
[ "$#" -eq 0 ] && exit 0
dir="$(cd "$(dirname "$0")" && pwd)"
if ! "${dir}/scrcpy" --version >/dev/null 2>&1; then
  if ! command -v scrcpy >/dev/null 2>&1; then
    echo "Android Mirror: the bundled scrcpy cannot start and no scrcpy is on PATH" >&2
    exit 127
  fi
  echo "Android Mirror: the bundled scrcpy cannot start; using scrcpy from PATH" >&2
  exec scrcpy "$@"
fi
resources="$(cd "${dir}/../Resources" && pwd)"
export SCRCPY_SERVER_PATH="${resources}/scrcpy-server"
export SCRCPY_ICON_DIR="$resources"
exec "${dir}/scrcpy" "$@"
LAUNCHER
  chmod 755 "${MIRROR}/Contents/MacOS/Android Mirror"

  cat > "${MIRROR}/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>Android Mirror</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleIdentifier</key><string>com.ovitrif.awb.mirror</string>
  <key>CFBundleName</key><string>Android Mirror</string>
  <key>CFBundleDisplayName</key><string>Android Mirror</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
  codesign --force --sign - "${MIRROR}/Contents/MacOS/scrcpy" 2>/dev/null || true
  codesign --force --sign - "$MIRROR" 2>/dev/null || true
fi

cat > "${APP}/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>awb-app</string>
  <key>CFBundleIconFile</key><string>${ICON_NAME}</string>
  ${ICON_PLIST_KEYS}
  <key>CFBundleIdentifier</key><string>com.ovitrif.awb</string>
  <key>CFBundleName</key><string>Android Wifi Bridge</string>
  <key>CFBundleDisplayName</key><string>Android Wifi Bridge</string>
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
