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

Each change starts from a public issue that explains it.

1. Create a branch named `<type>/<issue-number>-<short-description>`, for example `feat/2-spaced-retries`. `<type>` is the conventional type of the commit subject, such as `feat`, `fix`, or `docs`.
2. Keep the change to that one issue. Add regression tests for the behavior it affects.
3. Update the relevant documentation when commands, data formats, or behavior change.
4. Open one pull request for the issue. Write the pull request title like a commit subject, for example `feat(papers): require three spaced retries before a mistake is fixed`. Include a short summary and the test results.
5. Get an independent review before merging.

Each pull request lands on `main` as one squash commit, with the pull request title as the commit subject.

`main` always works. Every merge builds and passes `python3 ci/check.py`.

Keep reusable tests in the repository. Put one-off investigation logs, review reports, and test-run attachments in the PR. Use whichever editor or coding tools you prefer; keep personal configuration out of version control.

## Commits

Use a short imperative subject. End the message with a footer that names the issue:

```text
fix(storage): preserve raw input when an update fails

Closes #123
```

Write `Closes #N` when the commit completes issue N, and `Refs #N` when it contributes to issue N. Choose a conventional type such as `feat`, `fix`, `refactor`, `test`, `docs`, or `chore`. Keep the subject under 72 characters. Correct published commit-message mistakes in the PR rather than rewriting shared history.

## Releases

A release is a Git tag on `main`, such as `v0.3.0`, with a GitHub release page. The tag is `v` followed by the `version` in `Cargo.toml`. Users install and update from the newest release, as the [install guide](docs/install.md) describes, so a change merged into `main` reaches them only in the next release.

Version numbers are `MAJOR.MINOR.PATCH`. Below 1.0, a new feature or a change in behavior raises the minor number, for example from `0.2.0` to `0.3.0`. A fix alone raises the patch number, for example from `0.3.0` to `0.3.1`.

To make a release:

1. In a pull request, raise `version` in `Cargo.toml` to the new version. Change every other file that repeats Magnesium's version to match, such as `Cargo.lock` and the plugin files under `claude-plugin/` and `codex-plugin/`. `magnesium --version` prints this version, and the runtime refuses a plugin that names a different one.
2. After that pull request merges, create the tag and its release page:

   ```sh
   gh release create v0.3.0 --target main --generate-notes
   ```

   `--generate-notes` writes release notes that list the pull requests merged since the previous release.

The first release tags the current `main` as `v0.2.0`, the version that `Cargo.toml` already declares. It needs no version change, so it starts at step 2.
