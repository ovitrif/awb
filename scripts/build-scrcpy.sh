#!/bin/sh
# Builds the scrcpy that Android Wifi Bridge.app bundles: the pinned release
# plus scrcpy/macos-window.patch (QuickTime-style mirror window).
# Usage: scripts/build-scrcpy.sh [output-dir]
#
# Needs meson, ninja, pkg-config and Homebrew's ffmpeg, sdl3 and libusb:
#   brew install meson ninja pkgconf ffmpeg sdl3 libusb
# The binary links those Homebrew libraries, so it runs where they are
# installed. scripts/bundle-app.sh copies the output into the app when present.
# The build is scrcpy (Apache-2.0) modified by scrcpy/macos-window.patch.
#
# To move to a new scrcpy release: bump the version and both sha256 values,
# run this script and fix any rejected hunk in the extracted source, then
# refresh the patch with `scripts/build-scrcpy.sh --refresh-patch`.
set -eu

VERSION="5.0.1"
SOURCE_SHA256="a24b996ac23d0f674d3237c00b39a97829d8acbe9e1657a6c216fd51ea488ee5"
SERVER_SHA256="764eb6f79811d5211fe9df341120882ba9994c7a61b897d7bf3fb662e53bc536"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PATCH="${ROOT}/scrcpy/macos-window.patch"
WORK="${ROOT}/target/scrcpy-build"
SRC="${WORK}/scrcpy-${VERSION}"
PRISTINE="${WORK}/pristine/scrcpy-${VERSION}"

fetch() {
  url="$1" out="$2" sha="$3"
  if [ ! -f "$out" ] || ! echo "${sha}  ${out}" | shasum -a 256 -c - >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$out"
  fi
  echo "${sha}  ${out}" | shasum -a 256 -c - >/dev/null || {
    echo "checksum mismatch for ${url}" >&2
    exit 1
  }
}

extract() {
  rm -rf "$1"
  mkdir -p "$(dirname "$1")"
  tar xzf "${WORK}/scrcpy-${VERSION}.tar.gz" -C "$(dirname "$1")"
}

mkdir -p "$WORK"
fetch "https://github.com/Genymobile/scrcpy/archive/refs/tags/v${VERSION}.tar.gz" \
  "${WORK}/scrcpy-${VERSION}.tar.gz" "$SOURCE_SHA256"

if [ "${1:-}" = "--refresh-patch" ]; then
  # Rewrites the patch from the edited tree in $SRC
  extract "$PRISTINE"
  tab="$(printf '\t')"
  (cd "${WORK}/pristine" && diff -ruN "scrcpy-${VERSION}" "$SRC" || true) \
    | sed -e "s#^diff -ruN scrcpy-${VERSION}/\([^ ]*\) .*#diff -ruN a/\\1 b/\\1#" \
          -e "s#^--- scrcpy-${VERSION}/\([^${tab}]*\).*#--- a/\\1#" \
          -e "s#^+++ ${SRC}/\([^${tab}]*\).*#+++ b/\\1#" \
    > "$PATCH"
  echo "Refreshed ${PATCH}"
  exit 0
fi

OUT_DIR="${1:-${ROOT}/target/scrcpy}"

fetch "https://github.com/Genymobile/scrcpy/releases/download/v${VERSION}/scrcpy-server-v${VERSION}" \
  "${WORK}/scrcpy-server-v${VERSION}" "$SERVER_SHA256"

extract "$SRC"
patch -d "$SRC" -p1 --forward --quiet < "$PATCH"

# Portable: scrcpy finds scrcpy-server and its icons next to its executable
# and takes adb from $ADB (awb always sets it)
meson setup "${WORK}/build" "$SRC" --wipe --buildtype=release --strip \
  -Db_lto=true -Dcompile_server=false -Dportable=true >/dev/null
ninja -C "${WORK}/build" >/dev/null

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
cp "${WORK}/build/app/scrcpy" "${OUT_DIR}/scrcpy"
cp "${WORK}/scrcpy-server-v${VERSION}" "${OUT_DIR}/scrcpy-server"
cp "${SRC}/app/data/scrcpy.png" "${SRC}/app/data/disconnected.png" "$OUT_DIR"
# Apache-2.0: ship scrcpy's license with the modified build
cp "${SRC}/LICENSE" "${OUT_DIR}/LICENSE"
codesign --force --sign - "${OUT_DIR}/scrcpy" 2>/dev/null || true

echo "Built patched scrcpy ${VERSION} in ${OUT_DIR}"
