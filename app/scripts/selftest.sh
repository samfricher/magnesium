#!/bin/sh
# Check the real window end to end, without starting a model.
#
# The window opens for a few seconds, drives itself, writes a report and
# quits. It talks to the stand-in agent in crates/core/tests, and it works in
# a scratch study folder with a scratch home, so none of your study folders,
# settings or agent sessions are touched.
#
#   scripts/selftest.sh          build first, then check
#   scripts/selftest.sh --quick  check the window that is already built
#
# Set MAGNESIUM_APP_BIN to check another build, such as the one inside
# target/release/bundle/macos/Magnesium.app/Contents/MacOS/.

set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

if [ "${1:-}" != "--quick" ]; then
  npx tauri build --debug --no-bundle >/dev/null 2>&1 || {
    echo "the window did not build; run: npx tauri build --debug --no-bundle" >&2
    exit 1
  }
fi

WORK=$(mktemp -d "${TMPDIR:-/tmp}/magnesium-window-check.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/Demo-Course" "$WORK/home" "$WORK/window" "$WORK/agent"

# The stand-in agent, under the name the window looks for.
cat > "$WORK/claude" <<EOF
#!/bin/sh
export STAND_IN_LOG='$WORK/agent.log'
export STAND_IN_STATE='$WORK/agent'
exec python3 '$ROOT/crates/core/tests/stand_in_agent.py' "\$@"
EOF
chmod +x "$WORK/claude"

# The real runtime, with a scratch home: the one built at the root of this
# checkout, which is the one this window's readers are written for, else the
# one installed. Setting a folder up writes user settings, and those must land
# in the scratch home.
RUNTIME=""
if [ -x "$ROOT/../target/release/magnesium" ]; then
  RUNTIME="$ROOT/../target/release/magnesium"
else
  RUNTIME=$(command -v magnesium || true)
fi
if [ -n "$RUNTIME" ]; then
  cat > "$WORK/magnesium" <<EOF
#!/bin/sh
HOME='$WORK/home' exec '$RUNTIME' "\$@"
EOF
  chmod +x "$WORK/magnesium"
  (cd "$WORK/Demo-Course" && "$WORK/magnesium" init --json >/dev/null 2>&1) || true
  export MAGNESIUM_APP_RUNTIME="$WORK/magnesium"
fi

# A PDF in the course material, with real text on two pages, for the check to
# open beside the workspace's page.
mkdir -p "$WORK/Demo-Course/Study/materials"
python3 -I - "$WORK/Demo-Course/Study/materials/Week 1 slides.pdf" <<'PY'
import sys
objs = []
def add(body):
    objs.append(body)
    return len(objs)
font = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
words = ["Selectable words on page one", "Selectable words on page two"]
parent = len(objs) + 1 + 2 * len(words)
kids = []
for line in words:
    stream = "BT /F1 24 Tf 72 700 Td (%s) Tj ET" % line
    content = add("<< /Length %d >>\nstream\n%s\nendstream" % (len(stream), stream))
    kids.append(add("<< /Type /Page /Parent %d 0 R /MediaBox [0 0 612 792] /Contents %d 0 R /Resources << /Font << /F1 %d 0 R >> >> >>" % (parent, content, font)))
add("<< /Type /Pages /Kids [%s] /Count %d >>" % (" ".join("%d 0 R" % k for k in kids), len(kids)))
catalog = add("<< /Type /Catalog /Pages %d 0 R >>" % parent)
body = "%PDF-1.4\n"
offsets = []
for n, obj in enumerate(objs, 1):
    offsets.append(len(body))
    body += "%d 0 obj\n%s\nendobj\n" % (n, obj)
xref = len(body)
body += "xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1) + "".join("%010d 00000 n \n" % o for o in offsets)
body += "trailer\n<< /Size %d /Root %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, catalog, xref)
open(sys.argv[1], "wb").write(body.encode("latin-1"))
PY

MAGNESIUM_APP_SELFTEST="$WORK/report.json" \
MAGNESIUM_APP_AGENT="$WORK/claude" \
MAGNESIUM_APP_HOME="$WORK/window" \
MAGNESIUM_APP_FOLDER="$WORK/Demo-Course" \
  "${MAGNESIUM_APP_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/debug/magnesium-window}" >"$WORK/window.log" 2>&1 || true

if [ ! -f "$WORK/report.json" ]; then
  echo "the window wrote no report. Its output:" >&2
  tail -20 "$WORK/window.log" >&2
  exit 1
fi
cat "$WORK/report.json"
echo
grep -q '"ok": true' "$WORK/report.json"
