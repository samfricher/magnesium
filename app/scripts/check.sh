#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

PART=${1:-}
case "$PART" in
  ""|--window|--pages|--rust) ;;
  *) echo "check.sh: unknown argument $PART" >&2; exit 64 ;;
esac

if [ "$PART" != "--rust" ]; then
  [ -d node_modules ] || npm ci
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
