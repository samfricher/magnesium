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
#   Intro-Statistics/    the course: three lecture notes in Study/materials,
#                        and a plan for "Midterm 2", ten days from today, in
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
mkdir -p "$COURSE/Study/materials" "$COURSE/Study/midterm-2"
cat > "$COURSE/Study/materials/week5-sampling-distributions.md" <<'EOF'
# Week 5: Sampling distributions

A statistic, such as the sample mean x̄, changes from sample to sample. Its
**sampling distribution** is the distribution of its values over all the
samples of size n we could have drawn.

## The sample mean

If the population has mean μ and standard deviation σ, then for random
samples of size n:

- the mean of x̄ is μ (x̄ is unbiased);
- the standard deviation of x̄ is σ / √n, called the **standard error**.

When σ is unknown we estimate the standard error with s / √n.

## The central limit theorem

For large n (a common rule: n ≥ 30), x̄ is approximately normal whatever the
shape of the population. For a normal population it is normal for every n.

## Worked example

Bags of flour have mean weight 1000 g and standard deviation 12 g. For
samples of 36 bags, the standard error is 12 / √36 = 2 g, so about 95% of
sample means fall within 1000 ± 4 g.

## Check yourself

1. A sample of 25 has s = 10. What is the estimated standard error of x̄?
2. Why does the standard error shrink as n grows?
EOF
cat > "$COURSE/Study/materials/week6-confidence-intervals.md" <<'EOF'
# Week 6: Confidence intervals for a mean

A **confidence interval** gives a range of plausible values for a parameter.
For a population mean with σ unknown:

    x̄ ± t* · s / √n,   with n − 1 degrees of freedom.

The **margin of error** is t* · s / √n.

## Reading the confidence level

A 95% interval comes from a method that captures μ in 95% of samples. It
does not say there is a 95% probability that μ lies in this one interval:
μ is fixed, and the interval either holds it or does not.

## What changes the width

- A higher confidence level widens the interval (larger t*).
- A larger sample narrows it (smaller s / √n).
- More spread in the data widens it.

## Worked example

A sample of 16 study sessions has x̄ = 42 min and s = 8 min. With 15
degrees of freedom, t* = 2.131 for 95%. The margin is 2.131 · 8 / 4 = 4.26,
so the interval is 37.7 to 46.3 minutes.

## Check yourself

1. Find a 90% interval for the example above (t* = 1.753).
2. To halve the margin of error, by what factor must n grow?
EOF
cat > "$COURSE/Study/materials/week7-hypothesis-tests.md" <<'EOF'
# Week 7: Hypothesis tests for a mean

A test weighs the evidence against a **null hypothesis** H₀, such as
μ = 50, in favour of an **alternative** Hₐ, such as μ ≠ 50.

## The one-sample t test

    t = (x̄ − μ₀) / (s / √n),   with n − 1 degrees of freedom.

The **p-value** is the probability, if H₀ were true, of a statistic at least
as extreme as the one observed. A small p-value is evidence against H₀.

## Errors

- Type I: rejecting a true H₀. Its probability is the significance level α.
- Type II: keeping a false H₀.

## Tests and intervals

A two-sided test at level α rejects μ = μ₀ exactly when μ₀ lies outside the
(1 − α) confidence interval.

## Worked example

n = 25, x̄ = 53, s = 10, H₀: μ = 50. The standard error is 2, so t = 1.5
with 24 degrees of freedom; the two-sided p-value is about 0.15. The data
give little evidence against μ = 50.

## Check yourself

1. Does a p-value of 0.15 show that H₀ is true?
2. What happens to t if n grows and x̄ and s stay the same?
EOF

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

- $TODAY: plan built from the course notes for weeks 5 to 7.
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
