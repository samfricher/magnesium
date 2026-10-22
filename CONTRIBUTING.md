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
failed checks stop the run. The builds, lint, Python tests, and documentation
checks overlap and print each command's output when it finishes; the Rust tests
and native source check then run alone because the tests are sensitive to CPU
load, except that the `opencode_adapter` test, which mostly waits, runs beside the
other Rust tests. The Rust tests keep their temporary files on a RAM disk
(macOS) or in `/dev/shm` (Linux) that the command removes afterwards; set
`MAGNESIUM_CI_DISK_TMP=1` to use the default temporary directory instead.

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

Each change starts from a public issue that explains it. Work on one issue per pull request, and keep the change to that issue.

1. Create a branch named `<type>/<issue-number>-<short-description>`, for example `feat/2-spaced-retries`. `<type>` is the conventional type of the commit subject, such as `feat`, `fix`, or `docs`.
2. Add regression tests for the behavior that the change affects.
3. Update the relevant documentation when commands, data formats, or behavior change.
4. Open a pull request for the issue. Write the pull request title like a commit subject, for example `feat(papers): require three spaced retries before a mistake is fixed`. In the description, include a short summary and the test results. End the description with `Closes #N`, or with `Refs #N` when the pull request only contributes to the issue.
5. Get an independent review before merging.

Merge each pull request into `main` as one squash commit. The commit subject is the pull request title, and the commit body is the pull request description.

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

A release is a Git tag on `main`, such as `v0.3.0`, with a GitHub release page. The tag is `v` followed by the `version` in `Cargo.toml`, with no suffix. The install guide ignores any other tag, such as `v0.3.0-rc.1`. If you publish a page for such a tag, create it with `gh release create --prerelease`, so that `gh release view` skips it too. Users install and update from the newest release, as the [install guide](docs/install.md) describes, so a change merged into `main` reaches them only in the next release.

Version numbers are `MAJOR.MINOR.PATCH`. Below 1.0, a new feature or a change in behavior raises the minor number, for example from `0.2.0` to `0.3.0`. A fix alone raises the patch number, for example from `0.3.0` to `0.3.1`.

To make a release:

1. Open an issue for the release, such as `Release v0.3.0`. Create a branch for it, such as `chore/<issue-number>-release-v0.3.0`.
2. Raise `version` in `Cargo.toml` to the new version. Replace the old version with the new one in each file that `git grep -lF <old-version> -- claude-plugin codex-plugin 'tests/*.rs'` lists. Check that each match is Magnesium's version and not a dependency's. Then run `cargo build` without `--locked`. It updates the `magnesium` entry in `Cargo.lock`. `magnesium --version` prints this version, and the runtime refuses a plugin that names a different one.
3. Open a pull request titled `chore(release): raise the version to 0.3.0`, and merge it as described in Make a change. End its description with `Refs #N`, not `Closes #N`, because the release does not exist until step 4.
4. Create the tag and its release page on the merged version bump, not on whatever is newest on `main`, which may include a pull request merged after it. Find the bump's squash commit with `gh pr view <pull-request-number> --json mergeCommit -q .mergeCommit.oid`, then run:

   ```sh
   gh release create v0.3.0 --target <squash-commit-sha> --generate-notes
   ```

   `--generate-notes` writes release notes that list the pull requests merged since the previous release. Then close the release issue.

The first release tags the current `main` commit as `v0.2.0`, the version that `Cargo.toml` already declares. It needs no version change, so it starts at step 4. With no version bump, take the commit from `git rev-parse origin/main` after `git fetch origin`.
