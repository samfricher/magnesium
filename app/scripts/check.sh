#!/bin/sh
# The window's own checks. The runtime's check at the root of the repository
# (python3 ci/check.py) neither builds nor tests the window, so run this after
# a change under app/, or after a change to what the runtime writes.
#
#   scripts/check.sh            the pages and their checks, Rust format and
#                               lints, the core tests, and the readers against
#                               the runtime built from this checkout
#   scripts/check.sh --window   all of that, then the real window against
#                               the stand-in agent (scripts/selftest.sh)
#   scripts/check.sh --pages    only the pages and their checks
#   scripts/check.sh --rust     only the Rust format, lints and core tests
#
# scripts/prove.sh runs the parts a change can affect, side by side.
#
# It starts no model. The check against the real agent, on your own
# subscription, is scripts/live-check.sh.
#
# The first run builds the runtime at the root and the window's shell, which
# takes a few minutes.
set -eu
cd "$(dirname "$0")/.."

PART=${1:-}
case "$PART" in
  ""|--window|--pages|--rust) ;;
  *) echo "check.sh: unknown argument $PART" >&2; exit 64 ;;
esac

if [ "$PART" != "--rust" ]; then
  [ -d node_modules ] || npm ci
  # The shell embeds the built pages, so they are built before any Rust check.
  npm run build
  node scripts/test-task-line.mjs
  node scripts/test-setup.mjs
  node scripts/test-next.mjs
  node scripts/test-record-sign.mjs
  node scripts/test-paper-time.mjs
  node scripts/test-prefs.mjs
  node scripts/test-step-head.mjs
  node scripts/test-words-batch.mjs
  node scripts/test-reveal.mjs
  node scripts/test-prove.mjs
fi
if [ "$PART" != "--pages" ]; then
  # The shell does not compile without built pages to embed.
  [ -d dist ] || npm run build
  cargo fmt --all --check
  cargo clippy --locked --workspace --all-targets -- -D warnings
  cargo test --locked -p mg-core
fi

if [ "$PART" = "--window" ]; then
  scripts/selftest.sh
fi
case "$PART" in
  --pages) echo "the pages' checks passed" ;;
  --rust) echo "the window's Rust checks passed" ;;
  *) echo "the window's checks passed" ;;
esac
