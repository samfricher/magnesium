#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
TRACE="${TMPDIR:-/tmp}/magnesium-window-live-trace.jsonl"
: > "$TRACE"
MAGNESIUM_APP_TRACE="$TRACE" exec cargo test -p mg-core --test live -- --ignored --nocapture
