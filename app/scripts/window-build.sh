#!/bin/sh
# Build the window, or the runtime it runs lessons through, to look at or to
# check, and print the program's path.
#
#   scripts/window-build.sh                        this checkout's window as it stands
#   scripts/window-build.sh --runtime              this checkout's runtime (magnesium)
#   scripts/window-build.sh --ref <ref>            the window as of a commit, such as origin/main,
#   scripts/window-build.sh --runtime --ref <ref>  or the runtime, to photograph "before" beside your change
#
# The window is the build scripts/selftest.sh makes (npx tauri build --debug
# --no-bundle), which carries this checkout's own pages inside the program.
# A plain `cargo build` does not: it loads its pages from port 1430, from
# whichever checkout's Vite server holds it. The runtime is the build the
# window's core tests make (cargo build --release --bin magnesium at the root).
#
# This checkout's window: nothing is built when no file the program is made
# from has changed since the last build here and the program is still the one
# that build made, so asking twice costs nothing. Set WINDOW_PAGES_BUILT=1
# when `npm run build` has just run, and the pages are not built a second time.
#
# A commit: built once in a checkout of its own under the cache folder
# (~/Library/Caches/magnesium-dev, or MAGNESIUM_DEV_CACHE) and kept there by
# its commit, so every later "before" of that commit, from any checkout, is
# already made. The first build there is from nothing, a few minutes; after
# that a new commit rebuilds only what changed.
#
# What the cache holds: the newest eight windows (28 MB each) and runtimes
# (12 MB each), which are what make "before" quick; npm's downloads; and the
# checkout with its build folders, which only make the next commit's build
# short. Those build folders are cleared when they pass 6 GB
# (MAGNESIUM_DEV_CACHE_MAX_MB) or when no build has used them for a week.
# They are built there and not copied from this checkout: a copy that shares
# its blocks with a checkout's build folder (an APFS clone) costs little
# while that checkout exists and all of its size once it is deleted.
#
# The checkout is a git worktree, so `git worktree list` shows it. To remove
# the cache: delete the folder, then `git worktree prune`.
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

# Each takes the checkout's folder (app/ for the window, the root for the runtime). Their output goes to standard
# error, so the path is all this script prints.
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
  # Every file the program is built from, as it stands: the files git holds, what differs from them, and the files
  # git does not know yet. The scripts here drive the window and are not part of it.
  SOURCES=$( (git ls-files -s -- . ':(exclude)scripts'; git diff -- . ':(exclude)scripts'; git ls-files -o --exclude-standard -z -- . ':(exclude)scripts' | xargs -0 shasum 2>/dev/null) | shasum | cut -d' ' -f1)
  KEPT="$TARGET/debug/.window-build"
  # With the program's own time beside it: `npx tauri dev` or a plain `cargo build` writes another program to the
  # same path and changes no source.
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
# The build folders of a cache nothing has built in for a week are not worth their room. Asked on every call,
# a kept program's too, since that is the only time this script runs.
if [ -n "$(find "$CACHE/last-build" -mtime +7 2>/dev/null)" ]; then
  rm -rf "$SRC/app/target" "$SRC/target" "$CACHE/last-build"
fi
if [ -x "$BIN" ]; then
  echo "$BIN"
  exit 0
fi
mkdir -p "$KEEP"

# One build in the cache's checkout at a time: a second asker waits, then finds the program made. The folder holds
# the builder's process number, so a build that was killed (which leaves the folder) does not hold the next one up.
LOCK="$CACHE/building"
WAITED=0
until mkdir "$LOCK" 2>/dev/null; do
  HOLDER=$(cat "$LOCK/pid" 2>/dev/null || true)
  if [ -n "$HOLDER" ] && ! kill -0 "$HOLDER" 2>/dev/null; then
    # Gone. Renamed away first, so of two askers that both see it gone only one takes it over.
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
# No incremental data: it grows with every commit built, and each commit is built once.
export CARGO_INCREMENTAL=0
if [ "$WHAT" = runtime ]; then
  (unset CARGO_TARGET_DIR; build_runtime "$SRC") || { echo "window-build.sh: the runtime as of $REF did not build" >&2; exit 1; }
  MADE="$SRC/target/release/magnesium"
else
  # The packages the commit asks for, whenever they are not the ones last installed here.
  if ! cmp -s "$SRC/app/package-lock.json" "$SRC/app/node_modules/.lock-installed" 2>/dev/null; then
    # With a cache of its own: npm's usual one, in the home folder, may not be writable where an agent runs.
    (cd "$SRC/app" && npm ci --cache "$CACHE/npm" --prefer-offline >&2)
    cp "$SRC/app/package-lock.json" "$SRC/app/node_modules/.lock-installed"
  fi
  (unset CARGO_TARGET_DIR WINDOW_PAGES_BUILT; build_window "$SRC/app") || { echo "window-build.sh: the window as of $REF did not build" >&2; exit 1; }
  MADE="$SRC/app/target/debug/magnesium-window"
fi
cp -c "$MADE" "$BIN.partial" 2>/dev/null || cp "$MADE" "$BIN.partial"
mv "$BIN.partial" "$BIN"
# The build folders, held to their limit: past it they go, and the next commit builds from nothing.
MAX_MB=${MAGNESIUM_DEV_CACHE_MAX_MB:-6000}
USED_MB=$(du -sk "$SRC/app/target" "$SRC/target" 2>/dev/null | awk '{ kb += $1 } END { print int(kb / 1024) }')
if [ "${USED_MB:-0}" -gt "$MAX_MB" ]; then
  echo "window-build.sh: the cache's build folders reached ${USED_MB} MB (limit ${MAX_MB}); cleared" >&2
  rm -rf "$SRC/app/target" "$SRC/target"
fi
# Keep the newest eight of the programs kept by commit; a run's own copy beside them is not one.
ls -t "$KEEP" | grep -E '^magnesium(-window)?-[0-9a-f]{12}$' | awk 'NR > 8' | while read -r old; do rm -f "$KEEP/$old"; done
echo "$BIN"
