# Contributing to Magnesium

Start with the [architecture](docs/architecture/learner-model.md) for the main components and the [API reference](docs/architecture/learner-model-contracts.md) for tool behavior.

## Build and test

Rust is pinned in `rust-toolchain.toml`. Repository checks also need Python 3.11+
and Node.js. The OpenCode plugin test in `tests/opencode_adapter.rs` runs the
dependency-free `opencode-plugin/tests.mjs`. Optional hosted CI uses Node.js 24.

From the repository root, run the ordinary local checks:

```sh
python3 ci/check.py
```

This runs Rust formatting, Clippy, the release build and tests, native source
checks, Python tests, and documentation/instruction checks. Subprocess tests use
the release executable built by the same command. Missing prerequisites and
failed checks stop the run.

For a smaller loop, select `core`, `python`, or `docs`, for example
`python3 ci/check.py core docs`. Use `--list` to inspect the commands or `--offline`
to require Cargo's cached dependencies. Individual `cargo test` commands remain
useful while working on a particular behavior.

Native-host installation tests that need a real Codex CLI are explicit opt-in
tests. An ignored test is not a pass. Live tutoring and
conversion-quality checks need their own environments. Never use personal
learning history as test data.

GitHub Actions is manually dispatched and uses the same local check entrypoint.

## Make a change

1. Keep the change focused and add regression tests for the behavior it affects.
2. Update the relevant documentation when commands, data formats, or behavior change.
3. Include a short summary and test results in the PR; get an independent review before merging.

Keep reusable tests in the repository. Put one-off investigation logs, review reports, and test-run attachments in the PR. Use whichever editor or coding tools you prefer; keep personal configuration out of version control.

## Commits

Use a short imperative subject:

```text
fix(storage): preserve raw input when an update fails

Ticket: LEARN-123
```

Choose a conventional type such as `feat`, `fix`, `refactor`, `test`, `docs`, or `chore`. Keep the subject under 72 characters and include a ticket footer when the work has a ticket. Keep unrelated changes in separate commits. Correct published commit-message mistakes in the PR rather than rewriting shared history.
