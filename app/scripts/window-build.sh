#!/bin/sh
set -eu
APP=$(cd "$(dirname "$0")/.." && pwd)
REF=""
WHAT=window
while [ $# -gt 0 ]; do
  case "$1" in
    --ref) REF=${2:?--ref needs a commit}; shift 2 ;;
    --runtime) WHAT=runtime; shift ;;
    *) echo "window-build.sh: unknown argument $1" >&2; exit 64 ;;
  esac
done

build_window() {
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
build_runtime() {
  (cd "$1" && CARGO_TARGET_DIR="$1/target" cargo build --release --locked --bin magnesium >&2)
}

if [ -z "$REF" ] && [ "$WHAT" = runtime ]; then
  ROOT=$(cd "$APP/.." && pwd)
  build_runtime "$ROOT" || { echo "window-build.sh: the runtime did not build" >&2; exit 1; }
  echo "$ROOT/target/release/magnesium"
  exit 0
fi

if [ -z "$REF" ]; then
  cd "$APP"
  TARGET=${CARGO_TARGET_DIR:-$APP/target}
  BIN="$TARGET/debug/magnesium-window"
  SOURCES=$( (git ls-files -s -- . ':(exclude)scripts'; git diff -- . ':(exclude)scripts'; git ls-files -o --exclude-standard -z -- . ':(exclude)scripts' | xargs -0 shasum 2>/dev/null) | shasum | cut -d' ' -f1)
  KEPT="$TARGET/debug/.window-build"
  made() { echo "$SOURCES $(stat -f %Fm "$BIN" 2>/dev/null)"; }
  if [ -x "$BIN" ] && [ -f "$KEPT" ] && [ "$(cat "$KEPT")" = "$(made)" ]; then
    echo "$BIN"
    exit 0
  fi
  rm -f "$KEPT"
  build_window "$APP" || { echo "window-build.sh: the window did not build" >&2; exit 1; }
  made > "$KEPT"
  echo "$BIN"
  exit 0
fi

SHA=$(git -C "$APP" rev-parse --verify --quiet "$REF^{commit}") || { echo "window-build.sh: no commit named $REF" >&2; exit 1; }
CACHE=${MAGNESIUM_DEV_CACHE:-$HOME/Library/Caches/magnesium-dev}
SHORT=$(printf %s "$SHA" | cut -c1-12)
if [ "$WHAT" = runtime ]; then
  KEEP="$CACHE/runtimes"
  BIN="$KEEP/magnesium-$SHORT"
else
  KEEP="$CACHE/windows"
  BIN="$KEEP/magnesium-window-$SHORT"
fi
SRC="$CACHE/src"
if [ -n "$(find "$CACHE/last-build" -mtime +7 2>/dev/null)" ]; then
  rm -rf "$SRC/app/target" "$SRC/target" "$CACHE/last-build"
fi
if [ -x "$BIN" ]; then
  echo "$BIN"
  exit 0
fi
mkdir -p "$KEEP"

LOCK="$CACHE/building"
WAITED=0
until mkdir "$LOCK" 2>/dev/null; do
  HOLDER=$(cat "$LOCK/pid" 2>/dev/null || true)
  if [ -n "$HOLDER" ] && ! kill -0 "$HOLDER" 2>/dev/null; then
    [ "$(cat "$LOCK/pid" 2>/dev/null)" != "$HOLDER" ] || { mv "$LOCK" "$LOCK.gone-$$" 2>/dev/null && rm -rf "$LOCK.gone-$$"; } || true
    continue
  fi
  [ "$WAITED" -lt 900 ] || { echo "window-build.sh: process ${HOLDER:-unknown} has held $LOCK for 15 minutes" >&2; exit 1; }
  sleep 1
  WAITED=$((WAITED + 1))
done
echo $$ > "$LOCK/pid"
trap 'rm -rf "$LOCK"' EXIT
trap 'exit 130' INT TERM
if [ -x "$BIN" ]; then
  echo "$BIN"
  exit 0
fi

if [ -e "$SRC/.git" ]; then
  git -C "$SRC" checkout --quiet --detach "$SHA" >&2
else
  rm -rf "$SRC"
  git -C "$APP" worktree prune
  git -C "$APP" worktree add --quiet --detach "$SRC" "$SHA" >&2
fi
touch "$CACHE/last-build"
export CARGO_INCREMENTAL=0
if [ "$WHAT" = runtime ]; then
  (unset CARGO_TARGET_DIR; build_runtime "$SRC") || { echo "window-build.sh: the runtime as of $REF did not build" >&2; exit 1; }
  MADE="$SRC/target/release/magnesium"
else
  if ! cmp -s "$SRC/app/package-lock.json" "$SRC/app/node_modules/.lock-installed" 2>/dev/null; then
    (cd "$SRC/app" && npm ci --cache "$CACHE/npm" --prefer-offline >&2)
    cp "$SRC/app/package-lock.json" "$SRC/app/node_modules/.lock-installed"
  fi
  (unset CARGO_TARGET_DIR WINDOW_PAGES_BUILT; build_window "$SRC/app") || { echo "window-build.sh: the window as of $REF did not build" >&2; exit 1; }
  MADE="$SRC/app/target/debug/magnesium-window"
fi
cp -c "$MADE" "$BIN.partial" 2>/dev/null || cp "$MADE" "$BIN.partial"
mv "$BIN.partial" "$BIN"
MAX_MB=${MAGNESIUM_DEV_CACHE_MAX_MB:-6000}
USED_MB=$(du -sk "$SRC/app/target" "$SRC/target" 2>/dev/null | awk '{ kb += $1 } END { print int(kb / 1024) }')
if [ "${USED_MB:-0}" -gt "$MAX_MB" ]; then
  echo "window-build.sh: the cache's build folders reached ${USED_MB} MB (limit ${MAX_MB}); cleared" >&2
  rm -rf "$SRC/app/target" "$SRC/target"
fi
ls -t "$KEEP" | grep -E '^magnesium(-window)?-[0-9a-f]{12}$' | awk 'NR > 8' | while read -r old; do rm -f "$KEEP/$old"; done
echo "$BIN"
