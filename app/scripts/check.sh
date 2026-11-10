#!/bin/sh
# The window's own checks. The runtime's check at the root of the repository
# (python3 ci/check.py) neither builds nor tests the window, so run this after
# a change under app/, or after a change to what the runtime writes.
#
#   scripts/check.sh            the pages, Rust format and lints, the core
#                               tests, and the readers against the runtime
#                               built from this checkout
#   scripts/check.sh --window   all of that, then the real window against
#                               the stand-in agent (scripts/selftest.sh)
#
# It starts no model. The check against the real agent, on your own
# subscription, is scripts/live-check.sh.
#
# The first run builds the runtime at the root and the window's shell, which
# takes a few minutes.
set -eu
cd "$(dirname "$0")/.."

[ -d node_modules ] || npm ci
# The shell embeds the built pages, so they are built before any Rust check.
npm run build
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked -p mg-core

if [ "${1:-}" = "--window" ]; then
  scripts/selftest.sh
fi
echo "the window's checks passed"
