#!/bin/sh
# Build the Mac download: Magnesium.app with the magnesium program inside it,
# both built from one commit, signed ad hoc, in a disk image that opens to the
# app and a shortcut to Applications.
#
#   scripts/package.sh 0.3.0-rc.1
#
# The workflow "Mac download" runs this on a GitHub-hosted Mac and uploads the
# disk image to a draft pre-release. A build made on your own Mac is for
# looking at: macOS sets no download flag on it, so it shows nothing of what
# a downloaded copy does on first open.
#
# Both programs report the commit they were built from. It is
# MAGNESIUM_BUILD_COMMIT when set, else the commit checked out, and then the
# working tree must be clean. The disk image is written to
# target/download/Magnesium-<version>-arm64.dmg, under app/.
#
# Ad hoc signing needs no Apple account. macOS then offers "Open Anyway" in
# System Settings > Privacy & Security on first open. An unsigned app would be
# reported as damaged instead.

set -eu
VERSION=${1:-}
if ! printf '%s\n' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$'; then
  echo "give the version, such as 0.3.0-rc.1" >&2
  exit 1
fi
if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
  echo "the download is for Apple Silicon Macs; build it on one" >&2
  exit 1
fi

APP_DIR=$(cd "$(dirname "$0")/.." && pwd)
ROOT=$(cd "$APP_DIR/.." && pwd)
OUT="$APP_DIR/target/download"
mkdir -p "$OUT"

if [ -z "${MAGNESIUM_BUILD_COMMIT:-}" ]; then
  if [ -n "$(git -C "$ROOT" status --porcelain)" ]; then
    echo "the working tree has changes that no commit holds; commit them first" >&2
    exit 1
  fi
  MAGNESIUM_BUILD_COMMIT=$(git -C "$ROOT" rev-parse HEAD)
fi
export MAGNESIUM_BUILD_COMMIT

# A build keeps the paths of its source files, for messages such as a panic's.
# A path under a home folder names the account that built it, so each is
# written relative to the home folder or the checkout instead. When several
# match, the compiler applies the last, so the checkout comes after the home.
export RUSTFLAGS="--remap-path-prefix=$HOME=~ --remap-path-prefix=$ROOT=."
export CFLAGS="-ffile-prefix-map=$HOME=~ -ffile-prefix-map=$ROOT=."
export CXXFLAGS="$CFLAGS"

echo "== the magnesium program, from $MAGNESIUM_BUILD_COMMIT"
(cd "$ROOT" && CARGO_TARGET_DIR="$ROOT/target/download" cargo build --locked --release --bin magnesium)
RUNTIME="$ROOT/target/download/release/magnesium"

echo "== the window, version $VERSION"
cd "$APP_DIR"
[ -d node_modules ] || npm ci
CARGO_TARGET_DIR="$APP_DIR/target/download" npx tauri build --bundles app --config "{\"version\":\"$VERSION\"}"
BUILT="$APP_DIR/target/download/release/bundle/macos/Magnesium.app"

echo "== the program inside the app, and the signature"
STAGE="$OUT/stage"
rm -rf "$STAGE"
mkdir -p "$STAGE"
ditto "$BUILT" "$STAGE/Magnesium.app"
APP="$STAGE/Magnesium.app"
install -m 755 "$RUNTIME" "$APP/Contents/MacOS/magnesium"
# Inside out: the program first, then the app, whose signature covers it.
codesign --force --sign - "$APP/Contents/MacOS/magnesium"
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"

echo "== checks"
REPORTED=$("$APP/Contents/MacOS/magnesium" --version)
if [ "$REPORTED" != "magnesium $(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -1)+$MAGNESIUM_BUILD_COMMIT" ]; then
  echo "the program inside reports \"$REPORTED\", not the commit $MAGNESIUM_BUILD_COMMIT" >&2
  exit 1
fi
if ! strings "$APP/Contents/MacOS/magnesium-window" | grep -q "$MAGNESIUM_BUILD_COMMIT"; then
  echo "the window does not hold the commit $MAGNESIUM_BUILD_COMMIT" >&2
  exit 1
fi
# No home folder path of whoever built it, in any file of the app.
FOUND=$(find "$APP" -type f -exec sh -c 'strings "$1" | grep -F -e "$2" -e "/Users/" | head -3 | sed "s|^|$1: |"' _ {} "$HOME" \;)
if [ -n "$FOUND" ]; then
  echo "the app holds a home folder path:" >&2
  printf '%s\n' "$FOUND" >&2
  exit 1
fi
echo "$REPORTED, signed ad hoc, no home folder path"

echo "== the disk image"
ln -s /Applications "$STAGE/Applications"
DMG="$OUT/Magnesium-$VERSION-arm64.dmg"
rm -f "$DMG"
hdiutil create -quiet -volname "Magnesium $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO "$DMG"
hdiutil verify -quiet "$DMG"
rm -rf "$STAGE"
echo "$DMG"
