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
# "id x y width height owner", one line per on-screen window; the largest is the window.
WINDOWS=$("$HELPER" "$PID")
set -- $(printf '%s\n' "$WINDOWS" | awk '{ area = $4 * $5; if (area > best) { best = area; line = $1 " " $2 " " $3 " " $4 " " $5 } } END { if (line) print line }')
if [ $# -lt 5 ]; then
  echo "capture.sh: process $PID has no window on screen" >&2
  exit 1
fi
ID=$1 X=$2 Y=$3 W=$4 H=$5
screencapture -o -x -l "$ID" "$OUT"
# The Mac draws its input-source bubble beside a focused field as a child
# window, and a capture of the window takes in any child that reaches past its
# edge. Cut the photograph back to the window: the capture spans every
# on-screen window of the process that overlaps it.
PW=$(sips -g pixelWidth "$OUT" | awk '/pixelWidth/ { print $2 }')
PH=$(sips -g pixelHeight "$OUT" | awk '/pixelHeight/ { print $2 }')
CROP=$(printf '%s\n' "$WINDOWS" | awk -v x="$X" -v y="$Y" -v w="$W" -v h="$H" -v pw="$PW" -v ph="$PH" '
  BEGIN { l = x; t = y; r = x + w; b = y + h }
  $2 < x + w && $2 + $4 > x && $3 < y + h && $3 + $5 > y {
    if ($2 < l) l = $2; if ($3 < t) t = $3
    if ($2 + $4 > r) r = $2 + $4; if ($3 + $5 > b) b = $3 + $5
  }
  END {
    s = pw / (r - l)
    ch = int(h * s + 0.5); cw = int(w * s + 0.5)
    if (ch != ph || cw != pw) printf "%d %d %d %d", ch, cw, int((y - t) * s + 0.5), int((x - l) * s + 0.5)
  }')
if [ -n "$CROP" ]; then
  set -- $CROP
  sips -c "$1" "$2" --cropOffset "$3" "$4" "$OUT" >/dev/null
fi
