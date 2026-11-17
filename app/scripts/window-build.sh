#!/bin/sh
# Build the window to look at or to check, and print the program's path.
#
#   scripts/window-build.sh              this checkout as it stands
#   scripts/window-build.sh --ref <ref>  the window as of a commit, such as origin/main,
#                                        to photograph "before" beside your change
#
# Both are the build scripts/selftest.sh makes (npx tauri build --debug
# --no-bundle), which carries this checkout's own pages inside the program.
# A plain `cargo build` does not: it loads its pages from port 1430, from
# whichever checkout's Vite server holds it.
#
# This checkout: nothing is built when no file the program is made from has
# changed since the last build here, so asking twice costs nothing. Set WINDOW_PAGES_BUILT=1
# when `npm run build` has just run, and the pages are not built a second time.
#
# A commit: built once in a checkout of its own under the cache folder
# (~/Library/Caches/magnesium-dev, or MAGNESIUM_DEV_CACHE) and kept there by
# its commit, so every later "before" of that commit, from any checkout, is
# already made. The first one of all copies this checkout's build folder to
# start from; after that a new commit rebuilds only what changed.
set -eu
APP=$(cd "$(dirname "$0")/.." && pwd)
REF=""
while [ $# -gt 0 ]; do
  case "$1" in
    --ref) REF=${2:?--ref needs a commit}; shift 2 ;;
    *) echo "window-build.sh: unknown argument $1" >&2; exit 64 ;;
  esac
done

build() {
  # $1: the app folder to build in. Its output goes to standard error, so the path is all this script prints.
  (
    cd "$1"
    [ -d node_modules ] || npm ci >&2
    if [ "${WINDOW_PAGES_BUILT:-}" = 1 ] && [ -d dist ]; then
      npx tauri build --debug --no-bundle --config '{"build":{"beforeBuildCommand":""}}' >&2
    else
      npx tauri build --debug --no-bundle >&2
    fi
  )
}

if [ -z "$REF" ]; then
  cd "$APP"
  TARGET=${CARGO_TARGET_DIR:-$APP/target}
  BIN="$TARGET/debug/magnesium-window"
  # Every file the program is built from, as it stands: the files git holds, what differs from them, and the files
  # git does not know yet. The scripts here drive the window and are not part of it.
  STAMP=$( (git ls-files -s -- . ':(exclude)scripts'; git diff -- . ':(exclude)scripts'; git ls-files -o --exclude-standard -z -- . ':(exclude)scripts' | xargs -0 shasum 2>/dev/null) | shasum | cut -d' ' -f1)
  KEPT="$TARGET/debug/.window-build"
  if [ -x "$BIN" ] && [ -f "$KEPT" ] && [ "$(cat "$KEPT")" = "$STAMP" ]; then
    echo "$BIN"
    exit 0
  fi
  rm -f "$KEPT"
  build "$APP" || { echo "window-build.sh: the window did not build" >&2; exit 1; }
  echo "$STAMP" > "$KEPT"
  echo "$BIN"
  exit 0
fi

SHA=$(git -C "$APP" rev-parse --verify --quiet "$REF^{commit}") || { echo "window-build.sh: no commit named $REF" >&2; exit 1; }
CACHE=${MAGNESIUM_DEV_CACHE:-$HOME/Library/Caches/magnesium-dev}
BIN="$CACHE/windows/magnesium-window-$(printf %s "$SHA" | cut -c1-12)"
if [ -x "$BIN" ]; then
  echo "$BIN"
  exit 0
fi
mkdir -p "$CACHE/windows"
# One build in the cache's checkout at a time: a second asker waits, then finds the program made.
LOCK="$CACHE/building"
WAITED=0
until mkdir "$LOCK" 2>/dev/null; do
  [ "$WAITED" -lt 900 ] || { echo "window-build.sh: another build has held $LOCK for 15 minutes; remove it if none is running" >&2; exit 1; }
  sleep 2
  WAITED=$((WAITED + 2))
done
trap 'rmdir "$LOCK" 2>/dev/null || true' EXIT
if [ -x "$BIN" ]; then
  echo "$BIN"
  exit 0
fi
SRC="$CACHE/src"
if [ -e "$SRC/.git" ]; then
  git -C "$SRC" checkout --quiet --detach "$SHA" >&2
else
  rm -rf "$SRC"
  git -C "$APP" worktree prune
  git -C "$APP" worktree add --quiet --detach "$SRC" "$SHA" >&2
  # Start from this checkout's build, as copies that take no room (APFS clones), so the first build is not from nothing.
  [ -d "$APP/node_modules" ] && cp -cR "$APP/node_modules" "$SRC/app/node_modules" 2>/dev/null || true
  if [ -d "$APP/target/debug" ]; then
    mkdir -p "$SRC/app/target"
    cp -cR "$APP/target/debug" "$SRC/app/target/debug" 2>/dev/null || true
    rm -f "$SRC/app/target/debug/.window-build"
  fi
fi
# The packages the commit asks for, when they differ from the ones copied.
if ! cmp -s "$SRC/app/package-lock.json" "$SRC/app/node_modules/.package-lock.json.built" 2>/dev/null; then
  if ! cmp -s "$SRC/app/package-lock.json" "$APP/package-lock.json" || [ ! -d "$SRC/app/node_modules" ]; then
    (cd "$SRC/app" && npm ci >&2)
  fi
  cp "$SRC/app/package-lock.json" "$SRC/app/node_modules/.package-lock.json.built"
fi
(unset CARGO_TARGET_DIR WINDOW_PAGES_BUILT; build "$SRC/app") || { echo "window-build.sh: the window as of $REF did not build" >&2; exit 1; }
cp -c "$SRC/app/target/debug/magnesium-window" "$BIN.partial" 2>/dev/null || cp "$SRC/app/target/debug/magnesium-window" "$BIN.partial"
mv "$BIN.partial" "$BIN"
# Keep the newest eight.
ls -t "$CACHE/windows" | awk 'NR > 8' | while read -r old; do rm -f "$CACHE/windows/$old"; done
echo "$BIN"
