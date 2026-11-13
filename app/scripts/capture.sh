#!/bin/sh
# Photograph one process's window, without its shadow.
#
#   scripts/capture.sh <pid> <out.png>
#
# Finds the largest on-screen window of that process (layer 0) with a small
# Swift helper, compiled once into target/winid from scripts/winid.swift,
# then captures it with `screencapture -o -l <id>`. The Mac asks once for
# screen recording permission for the terminal that runs this.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PID=${1:?usage: capture.sh <pid> <out.png>}
OUT=${2:?usage: capture.sh <pid> <out.png>}
HELPER="$ROOT/target/winid"
if [ ! -x "$HELPER" ] || [ "$ROOT/scripts/winid.swift" -nt "$HELPER" ]; then
  mkdir -p "$ROOT/target"
  swiftc -O "$ROOT/scripts/winid.swift" -o "$HELPER" 2>/dev/null || {
    echo "capture.sh: could not compile scripts/winid.swift (is Xcode's swiftc installed?)" >&2
    exit 1
  }
fi
# "id width height owner", one line per window; the largest is the window.
ID=$("$HELPER" "$PID" | awk '{ area = $2 * $3; if (area > best) { best = area; id = $1 } } END { if (id) print id }')
if [ -z "$ID" ]; then
  echo "capture.sh: process $PID has no window on screen" >&2
  exit 1
fi
screencapture -o -x -l "$ID" "$OUT"
