#!/bin/sh
# Check a change before its pull request: the checks picked from the files
# that differ, leaving out those no such file can affect. Starts no model.
#
#   scripts/prove.sh                against origin/main
#   scripts/prove.sh --base <ref>   against another commit
#   scripts/prove.sh --all          every check, whatever changed
#   scripts/prove.sh --list         say which parts would run, and run none
#
# It reads which files differ from the base, committed or not, and runs:
#
#   always                     the Python tests and the documentation checks
#                              (python3 ci/check.py python docs)
#   a file under app/ changed  the pages and their checks (check.sh --pages);
#                              the runtime and the window built once; the real
#                              window against the stand-in agent (selftest.sh);
#                              and the window core's unit tests, one of which
#                              reads the pages (the words a button sends must
#                              be the words the engine knows)
#   Rust under app/ changed    in place of the unit tests, all of the window's
#                              Rust format, lints and core tests (check.sh --rust)
#   the runtime changed        the runtime's Rust checks (ci/check.py core), and
#   (a file outside app/)      all of the window's, which reads what it writes
#
# The runtime is everything outside app/ but the documents at the root and the
# checks' own tools in ci/: its build carries files from docs/, claude-plugin/
# and the tutor's instructions under src/. scripts/test-prove.mjs holds the
# rule to what the sources read.
#
# One part runs at a time. Run side by side, the window's Rust build took the
# processor from the runtime's tests, and one that gives a call a time limit
# (adapter::sources, "exact inspection remained deferred") failed; alone it
# passes. Each part's output is kept in target/prove/<run>/<part>.log, and
# only a failed part's is shown. A part that fails does not stop the parts
# that do not depend on it.
#
# The real window is checked on the runtime built from this checkout, never
# the installed one, from a copy of the program under a name of its own, kept
# in view and drawing while it runs (MAGNESIUM_APP_DRIVE). Windows started as
# target/debug/magnesium-window share one settings store on this Mac
# (~/Library/WebKit/magnesium-window), so two checks at once would change each
# other's settings; and macOS stops drawing a window that another covers,
# which fails the check's PDF step. So a change that stops a window started
# the plain way from drawing is not caught here: scripts/check.sh --window
# runs the same check without keeping it in view.
#
# For the photographs of a change, before and after: scripts/look.mjs.
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
# PROVE_CHANGED names the files that differ, one a line, in place of asking git: scripts/test-prove.mjs sets it.
if [ -n "${PROVE_CHANGED+set}" ]; then
  CHANGED=$PROVE_CHANGED
else
  FROM=$(git merge-base HEAD "$BASE" 2>/dev/null) || { echo "prove.sh: no commit named $BASE (try: git fetch origin)" >&2; exit 1; }
  # --no-renames: a file that moved counts under its old path too, which is where something may still read it.
  CHANGED=$( (git diff --name-only --no-renames "$FROM"; git ls-files -o --exclude-standard) | sort -u)
fi
has() { printf '%s\n' "$CHANGED" | grep -Eq "$1"; }
# Outside app/, everything is the runtime or is built into it (docs/ and claude-plugin/ among them), except the
# documents at the root and the checks' own tools in ci/.
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

# A folder to each run, so two runs in one checkout keep their own; a day-old run's is cleared.
mkdir -p "$APP/target/prove"
find "$APP/target/prove" -mindepth 1 -maxdepth 1 -mtime +1 -exec rm -rf {} + 2>/dev/null || true
LOGS="$APP/target/prove/$(date +%H%M%S)-$$"
mkdir -p "$LOGS"
STARTED=$(date +%s)
echo "prove: $(printf '%s\n' "$CHANGED" | grep -c . || true) file(s) differ from $BASE"

# run <part> <command...>: its output kept, its result and its seconds in <part>.code. Says whether it passed.
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
    # The runtime first: the real window is checked on this checkout's, as check.sh --window does by running the
    # core tests (which build it) before the self-test.
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
      # With the deployment target the window's build sets (Tauri's, 10.13 unless tauri.conf.json names another):
      # SQLite's build script runs again whenever that variable changes, so without it this and the window's
      # build would each recompile SQLite and the core after the other, about 17 s a time. The unit tests start
      # no cargo of their own, so the variable goes no further.
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
