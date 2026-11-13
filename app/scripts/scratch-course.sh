#!/bin/sh
# Make a scratch study course for the window, with made-up material, that
# never touches your own home, study folders or window settings.
#
#   scripts/scratch-course.sh <dir>                 make it under <dir>
#   scripts/scratch-course.sh <dir> --days-ago <N>  then, or later, move every
#                                                   time it recorded back N
#                                                   local days ("a week away")
#
# Under <dir>:
#   magnesium            the runtime, with HOME=<dir>/home, for MAGNESIUM_APP_RUNTIME
#   home/                the runtime's scratch home (magnesium init writes there)
#   window/              a scratch window home, for MAGNESIUM_APP_HOME: the
#                        three folders below in its library, and lessons on
#                        Magnesium (the direct engine)
#   Intro-Statistics/    the course, as a learner's folder often is: two past
#                        papers (PDFs, one with a "#" in its name) and an answer
#                        key in Study/midterm-2/papers, no other material, and
#                        a plan for "Midterm 2", ten days from today, in
#                        Study/midterm-2/plan.md with five items
#   Linear-Algebra/      set up, one lecture note, no plan
#   Organic-Chemistry/   set up, nothing in it yet
#
# The plan is a file the tutor writes in any lesson; nothing is written to
# the runtime's record here except by `magnesium init`. Every name, number and
# note is made up.
#
# The runtime is the one built at the root of this checkout when there is
# one, else the installed `magnesium`. Prints the course folder.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DIR=${1:?usage: scratch-course.sh <dir> [--days-ago N]}
shift
DAYS=""
while [ $# -gt 0 ]; do
  case "$1" in
    --days-ago) DAYS=${2:?--days-ago needs a number of days}; shift 2 ;;
    *) echo "scratch-course.sh: unknown argument $1" >&2; exit 64 ;;
  esac
done
mkdir -p "$DIR"
DIR=$(cd "$DIR" && pwd)
COURSE="$DIR/Intro-Statistics"

# Move every recorded time back N local days: the runtime's two stores, and
# the window's own notes of when the learner last wrote. Days are local
# calendar days at the same wall-clock time, as the runtime counts them
# (see tests/it/support/clock.rs at the root of the repository).
shift_days() {
  python3 -I - "$DIR" "$1" <<'PY'
import json, os, sqlite3, sys, time
from datetime import datetime, timedelta

root, days = sys.argv[1], int(sys.argv[2])

def earlier(ms):
    wall = datetime.fromtimestamp(ms / 1000) - timedelta(days=days)
    return int(time.mktime(wall.timetuple()) * 1000 + wall.microsecond // 1000)

moved = 0
for course in sorted(os.listdir(root)):
    for store in ("state.sqlite", "sittings.sqlite"):
        path = os.path.join(root, course, ".magnesium", store)
        if not os.path.isfile(path):
            continue
        conn = sqlite3.connect(path)
        tables = [r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")]
        for table in tables:
            for _, column, kind, *_ in conn.execute(f'PRAGMA table_info("{table}")'):
                name = column.lower()
                if name.endswith("_ms") or name.endswith("_at_ms"):
                    rows = conn.execute(f'SELECT rowid, "{column}" FROM "{table}" WHERE typeof("{column}")=\'integer\' AND "{column}" > 100000000000').fetchall()
                    for rowid, ms in rows:
                        conn.execute(f'UPDATE "{table}" SET "{column}"=? WHERE rowid=?', (earlier(ms), rowid))
                        moved += 1
                elif name.endswith("_day") or name.endswith("_on") or name in ("day", "date"):
                    moved += conn.execute(
                        f'UPDATE "{table}" SET "{column}"=date("{column}", ?) WHERE typeof("{column}")=\'text\' AND "{column}" GLOB \'[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]*\'',
                        (f"-{days} days",),
                    ).rowcount
        conn.commit()
        conn.close()

# The window's notes: when each folder's lesson stopped, when the direct
# engine last heard the learner, and each kept turn.
window = os.path.join(root, "window")
state_file = os.path.join(window, "state.json")
if os.path.isfile(state_file):
    with open(state_file) as f:
        state = json.load(f)
    for folder in state.get("folders", {}).values():
        if isinstance(folder.get("stopped_ms"), int):
            folder["stopped_ms"] = earlier(folder["stopped_ms"]); moved += 1
        direct = folder.get("direct")
        if isinstance(direct, dict) and isinstance(direct.get("last_message_ms"), int):
            direct["last_message_ms"] = earlier(direct["last_message_ms"]); moved += 1
        paper = folder.get("paper")
        if isinstance(paper, dict) and isinstance(paper.get("started_ms"), int):
            paper["started_ms"] = earlier(paper["started_ms"]); moved += 1
        # When the folder and each of its sessions was last opened.
        if isinstance(folder.get("opened_ms"), int):
            folder["opened_ms"] = earlier(folder["opened_ms"]); moved += 1
        for note in (folder.get("sessions") or {}).values():
            if isinstance(note, dict) and isinstance(note.get("opened_ms"), int):
                note["opened_ms"] = earlier(note["opened_ms"]); moved += 1
    # When the window last saw its tabs.
    layout = state.get("window")
    if isinstance(layout, dict) and isinstance(layout.get("seen_ms"), int):
        layout["seen_ms"] = earlier(layout["seen_ms"]); moved += 1
    days_seen = state.get("days")
    if isinstance(days_seen, dict):
        state["days"] = {
            (datetime.strptime(day, "%Y-%m-%d") - timedelta(days=days)).strftime("%Y-%m-%d"): n
            for day, n in days_seen.items()
        }
    with open(state_file + ".tmp", "w") as f:
        json.dump(state, f, indent=2)
    os.replace(state_file + ".tmp", state_file)
for dirpath, _, names in os.walk(window):
    for name in names:
        if not name.endswith(".jsonl"):
            continue
        path = os.path.join(dirpath, name)
        lines = []
        with open(path) as f:
            for line in f:
                try:
                    turn = json.loads(line)
                except ValueError:
                    lines.append(line.rstrip("\n")); continue
                if isinstance(turn.get("ms"), int):
                    turn["ms"] = earlier(turn["ms"]); moved += 1
                lines.append(json.dumps(turn))
        with open(path, "w") as f:
            f.write("\n".join(lines) + ("\n" if lines else ""))
print(f"moved {moved} recorded times back {days} days", file=sys.stderr)
PY
}

if [ -n "$DAYS" ] && [ -d "$COURSE/.magnesium" ]; then
  shift_days "$DAYS"
  echo "$COURSE"
  exit 0
fi

RUNTIME=""
if [ -x "$ROOT/../target/release/magnesium" ]; then
  RUNTIME="$ROOT/../target/release/magnesium"
else
  RUNTIME=$(command -v magnesium || true)
fi
if [ -z "$RUNTIME" ]; then
  echo "scratch-course.sh: no magnesium program: build it at the root (cargo build --release) or install it" >&2
  exit 1
fi
mkdir -p "$DIR/home" "$DIR/window"
cat > "$DIR/magnesium" <<EOF
#!/bin/sh
HOME='$DIR/home' exec '$RUNTIME' "\$@"
EOF
chmod +x "$DIR/magnesium"

day() { python3 -I -c "import datetime,sys; print((datetime.date.today()+datetime.timedelta(days=int(sys.argv[1]))).isoformat())" "$1"; }
weekday() { python3 -I -c "import datetime,sys; print((datetime.date.today()+datetime.timedelta(days=int(sys.argv[1]))).strftime('%a'))" "$1"; }
TODAY=$(day 0)
EXAM=$(day 10)

# --- The course ------------------------------------------------------------
# As a learner's folder often is: its past papers under the target's papers
# folder, and no other material. The papers are PDFs with real text (so a
# page's words can be selected), and one has its answer key beside it, which
# the window never opens or names.
mkdir -p "$COURSE/Study/materials" "$COURSE/Study/midterm-2/papers"
python3 -I - "$COURSE/Study/midterm-2/papers" <<'PY'
import os, sys
out = sys.argv[1]
def pdf(path, pages):
    objs = []
    def add(body):
        objs.append(body)
        return len(objs)
    font = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    bold = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>")
    kids = []
    pages_id = len(objs) + 1 + 2 * len(pages)
    esc = lambda t: t.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
    for lines in pages:
        text = ["BT", "/F2 18 Tf", "72 720 Td", "(%s) Tj" % esc(lines[0]), "/F1 12 Tf", "0 -34 Td"]
        for line in lines[1:]:
            text += ["(%s) Tj" % esc(line), "0 -20 Td"]
        text.append("ET")
        stream = "\n".join(text)
        content = add("<< /Length %d >>\nstream\n%s\nendstream" % (len(stream), stream))
        kids.append(add("<< /Type /Page /Parent %d 0 R /MediaBox [0 0 612 792] /Contents %d 0 R /Resources << /Font << /F1 %d 0 R /F2 %d 0 R >> >> >>" % (pages_id, content, font, bold)))
    add("<< /Type /Pages /Kids [%s] /Count %d >>" % (" ".join("%d 0 R" % k for k in kids), len(kids)))
    catalog = add("<< /Type /Catalog /Pages %d 0 R >>" % pages_id)
    body = "%PDF-1.4\n"
    offsets = []
    for n, obj in enumerate(objs, 1):
        offsets.append(len(body.encode("latin-1")))
        body += "%d 0 obj\n%s\nendobj\n" % (n, obj)
    xref = len(body.encode("latin-1"))
    body += "xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    body += "".join("%010d 00000 n \n" % o for o in offsets)
    body += "trailer\n<< /Size %d /Root %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, catalog, xref)
    with open(os.path.join(out, path), "wb") as f:
        f.write(body.encode("latin-1"))
pdf("Midterm 2023.pdf", [
    ["Midterm 2, 2023", "75 minutes. One page of notes and a calculator.", "", "Question 1 (10 marks)", "A sample of 36 bags has mean 1003 g and s = 12 g.", "(a) Find the standard error of the mean.", "(b) Is a mean of 1000 g plausible? Explain."],
    ["Question 2 (12 marks)", "A 95% interval for a mean study time is 37.7 to 46.3 minutes.", "(a) Give the sample mean and the margin of error.", "(b) Say what 95% means here, in one sentence."],
    ["Question 3 (14 marks)", "n = 25, x-bar = 53, s = 10. Test H0: mu = 50 against mu != 50.", "(a) Find t and its degrees of freedom.", "(b) The p-value is about 0.15. What do you conclude?", "(c) Name the error you could be making."],
])
pdf("Practice midterm #2.pdf", [
    ["Practice midterm #2", "Untimed. Work from a blank page.", "", "1. A sample of 49 has s = 14. Find the standard error.", "2. Why does a larger sample give a narrower interval?"],
    ["3. A test gives p = 0.03 at alpha = 0.05.", "What do you conclude, and what error could you be making?"],
])
pdf("Midterm 2023 key.pdf", [["Midterm 2, 2023: answer key", "1(a) 2 g. 1(b) Yes: 1000 is within 2 standard errors."]])
PY

cat > "$COURSE/Study/midterm-2/plan.md" <<EOF
# Midterm 2: study plan

Folder: \`Study/midterm-2/\`. Built $TODAY. Last re-plan $TODAY.

## 1. Facts

| Item | Value |
|---|---|
| Target | Midterm 2 |
| When | $EXAM ($(weekday 10)) 10:00 to 11:15 |
| Length | 75 |
| Aids | one page of notes, a calculator |
| Coverage | Weeks 5 to 7: sampling distributions, confidence intervals, hypothesis tests |
| Exclusions | two-way tables |
| Proficiency | "I follow the lectures but freeze on the tests"; read as mixed |
| Hours available | 9 |

## 6. Blocks

| # | Block | Purpose | Status | Day |
|---|---|---|---|---|
| 1 | Sampling distributions | the standard error of a mean, and when x̄ is normal | todo | $(day 0) |
| 2 | Confidence intervals | t intervals for one mean, and what 95% means | todo | $(day 2) |
| 3 | Hypothesis tests | t tests, p-values, the two kinds of error | todo | $(day 4) |
| 4 | Tests and intervals together | when a test and an interval agree | todo | $(day 6) |
| 5 | Practice midterm | 75 minutes, timed, from a blank page | todo | $(day 8) |

Never drop: the practice midterm.

Drop order if time runs short: tests and intervals together.

## 7. Sessions

- none planned

## 14. Changes

- $TODAY: plan built from the past papers.
EOF

# --- Two more folders for the library ----------------------------------------
mkdir -p "$DIR/Linear-Algebra/Study/materials" "$DIR/Organic-Chemistry"
cat > "$DIR/Linear-Algebra/Study/materials/week3-eigenvalues.md" <<'EOF'
# Week 3: Eigenvalues

A nonzero vector v is an **eigenvector** of A when Av = λv; λ is its
**eigenvalue**. The eigenvalues are the roots of det(A − λI) = 0.

For A = [[2, 1], [1, 2]], det(A − λI) = (2 − λ)² − 1, so λ = 1 or λ = 3.
EOF

for folder in "$COURSE" "$DIR/Linear-Algebra" "$DIR/Organic-Chemistry"; do
  (cd "$folder" && "$DIR/magnesium" init --json >/dev/null) || {
    echo "scratch-course.sh: magnesium init failed in $folder" >&2
    exit 1
  }
done

# --- The window's scratch home ----------------------------------------------
python3 -I - "$DIR" <<'PY'
import json, os, sys
root = sys.argv[1]
real = lambda name: os.path.realpath(os.path.join(root, name))
state = {
    "recent": [real("Intro-Statistics"), real("Linear-Algebra"), real("Organic-Chemistry")],
    "folders": {},
    "engine": "direct",
}
with open(os.path.join(root, "window", "state.json"), "w") as f:
    json.dump(state, f, indent=2)
PY

if [ -n "$DAYS" ]; then
  shift_days "$DAYS"
fi
echo "$COURSE"
