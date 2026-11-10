#!/bin/sh
# Check the pipe against the real Claude Code, in a scratch study folder.
#
# This starts a model on your own subscription: five short turns, a few
# minutes.
#
# Every line that passes between the window and the agent is kept in a trace
# file, whose path is printed. If a part fails, the trace shows what the real
# program sent.
set -eu
cd "$(dirname "$0")/.."
TRACE="${TMPDIR:-/tmp}/magnesium-window-live-trace.jsonl"
: > "$TRACE"
MAGNESIUM_APP_TRACE="$TRACE" exec cargo test -p mg-core --test live -- --ignored --nocapture
