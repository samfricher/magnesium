#!/bin/sh
set -eu
APP=$(cd "$(dirname "$0")/.." && pwd)
ROOT=$(cd "$APP/.." && pwd)
BASE=origin/main
ALL=""
LIST=""
while [ $# -gt 0 ]; do
  case "$1" in
    --base) BASE=${2:?--base needs a commit}; shift 2 ;;
    --all) ALL=1; shift ;;
    --list) LIST=1; shift ;;
    *) echo "prove.sh: unknown argument $1" >&2; exit 64 ;;
  esac
done

cd "$ROOT"
if [ -n "${PROVE_CHANGED+set}" ]; then
  CHANGED=$PROVE_CHANGED
else
  FROM=$(git merge-base HEAD "$BASE" 2>/dev/null) || { echo "prove.sh: no commit named $BASE (try: git fetch origin)" >&2; exit 1; }
  CHANGED=$( (git diff --name-only --no-renames "$FROM"; git ls-files -o --exclude-standard) | sort -u)
fi
has() { printf '%s\n' "$CHANGED" | grep -Eq "$1"; }
RUNTIME=""; WINDOW=""; WINDOW_RUST=""
if [ -n "$ALL" ] || printf '%s\n' "$CHANGED" | grep -Ev '^app/|^[^/]*\.md$|^ci/|^\.github/|^LICENSE' | grep -q .; then RUNTIME=1; fi
if [ -n "$RUNTIME" ] || has '^app/'; then WINDOW=1; fi
if [ -n "$RUNTIME" ] || has '^app/(crates/|src-tauri/|Cargo\.)'; then WINDOW_RUST=1; fi

if [ -n "$LIST" ]; then
  echo words
  [ -z "$WINDOW" ] || printf 'pages\nbuild\nwindow\n'
  if [ -n "$WINDOW_RUST" ]; then echo rust; elif [ -n "$WINDOW" ]; then echo unit; fi
  [ -z "$RUNTIME" ] || echo runtime
  exit 0
fi

mkdir -p "$APP/target/prove"
find "$APP/target/prove" -mindepth 1 -maxdepth 1 -mtime +1 -exec rm -rf {} + 2>/dev/null || true
LOGS="$APP/target/prove/$(date +%H%M%S)-$$"
mkdir -p "$LOGS"
STARTED=$(date +%s)
echo "prove: $(printf '%s\n' "$CHANGED" | grep -c . || true) file(s) differ from $BASE"

run() {
  part=$1; shift
  s=$(date +%s); code=0
  "$@" > "$LOGS/$part.log" 2>&1 || code=$?
  echo "$code $(( $(date +%s) - s ))" > "$LOGS/$part.code"
  [ "$code" = 0 ]
}

[ -n "$RUNTIME" ] || echo "prove: no file of the runtime differs, so its Rust checks are left out"
[ -n "$WINDOW" ] || echo "prove: no file under app/ differs, so the window's checks are left out"
[ -z "$WINDOW" ] || [ -n "$WINDOW_RUST" ] || echo "prove: no Rust under app/ differs, so of the window's Rust checks only the core's unit tests run"

run words python3 ci/check.py python docs || true
if [ -n "$WINDOW" ]; then
  cd "$APP"
  if run pages sh scripts/check.sh --pages; then
    s=$(date +%s); code=0
    MAGNESIUM=$(sh scripts/window-build.sh --runtime 2> "$LOGS/build.log") || code=$?
    [ "$code" != 0 ] || BIN=$(WINDOW_PAGES_BUILT=1 sh scripts/window-build.sh 2>> "$LOGS/build.log") || code=$?
    echo "$code $(( $(date +%s) - s ))" > "$LOGS/build.code"
    if [ "$code" = 0 ]; then
      echo "prove: the real window runs on the runtime built here, $MAGNESIUM"
      NAME="magnesium-window-prove-$$"
      OWN="$(dirname "$BIN")/$NAME"
      cp -c "$BIN" "$OWN" 2>/dev/null || cp "$BIN" "$OWN"
      mkdir -p "$LOGS/drive"
      run window env MAGNESIUM_TEST_BINARY="$MAGNESIUM" MAGNESIUM_APP_BIN="$OWN" MAGNESIUM_APP_DRIVE="$LOGS/drive" sh scripts/selftest.sh --quick || true
      rm -rf "$OWN" "$HOME/Library/WebKit/$NAME" "$HOME/Library/Caches/$NAME"
    fi
    if [ -n "$WINDOW_RUST" ]; then
      run rust sh scripts/check.sh --rust || true
    else
      run unit env MACOSX_DEPLOYMENT_TARGET=10.13 cargo test --locked -p mg-core --lib || true
    fi
  fi
  cd "$ROOT"
fi
[ -z "$RUNTIME" ] || run runtime python3 ci/check.py core || true

FAILED=""
PASSED=""
for part in words pages build window unit rust runtime; do
  [ -f "$LOGS/$part.code" ] || continue
  read -r code took < "$LOGS/$part.code"
  if [ "$code" = 0 ]; then
    echo "  passed  $part (${took}s)"
    PASSED="$PASSED $part"
  else
    echo "  FAILED  $part (${took}s): $LOGS/$part.log"
    FAILED="$FAILED $part"
  fi
done
for part in $FAILED; do
  echo
  echo "---- the end of $part.log ----"
  tail -40 "$LOGS/$part.log"
done
echo
if [ -n "$FAILED" ]; then
  echo "prove: failed:$FAILED, after $(( $(date +%s) - STARTED ))s"
  exit 1
fi
echo "prove: passed:$PASSED, in $(( $(date +%s) - STARTED ))s"
