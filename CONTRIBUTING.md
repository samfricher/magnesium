# Contributing to Magnesium

## Build and test

Rust is pinned in `rust-toolchain.toml`. Repository checks also need Python 3.11+.
The study window under `app/` is a separate Cargo workspace. `python3 ci/check.py`
does not build or test it. Its own check is `app/scripts/check.sh`.

From the repository root, run the local checks:

```sh
python3 ci/check.py
```

This runs Rust formatting, Clippy, the Rust tests, the Python tests, and the
documentation checks, one after another, and stops at the first failure. With
a warm build it takes about half a minute. Select `core`, `python`, or `docs`
to run a part, for example `python3 ci/check.py core docs`. Use `--list` to
print the commands or `--offline` to require Cargo's cached dependencies.

While you work on one behavior, run only its tests. Each of these takes a few
seconds:

```sh
cargo test --lib learner::practice    # the unit tests of one module
cargo test --test it sittings         # one file of integration tests
```

Unit tests sit beside the code. A module with more than a screen of tests
keeps them in a file of their own, such as `src/digest/tests.rs` for
`src/digest.rs`, so the source file holds only product code. Integration tests
start the built `magnesium` program the way Claude Code or a learner does.
They are one test program, `tests/it/`: a new file there needs a `mod` line in
`tests/it/main.rs`, and its helpers are in `tests/it/support/`.

The tests use the debug build that Cargo makes for them, so no release build
is needed. They do not wait for the disk either. A flush to disk can take a
Mac tens of milliseconds and the tests would make thousands, so the test helpers set
`MAGNESIUM_UNSAFE_NO_SYNC=1` for each `magnesium` they start, and the unit
tests never flush. Never set that variable to study: a crash or a power cut
can then lose what was written last.

The tutor's instruction files are in `src/workspace/learning/`. The build
works out each file's SHA-256, so an edit there needs no other change. A new
file needs an entry in `manifest.json` beside them.

An ignored test is not a pass. Live tutoring and
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
2. Raise `version` in `Cargo.toml` to the new version. Replace the old version with the new one in each file that `git grep -lF <old-version> -- claude-plugin tests app ':!*.lock' ':!app/package-lock.json'` lists. Check that each match is Magnesium's version and not a dependency's. Then run `cargo build` without `--locked`. It updates the `magnesium` entry in `Cargo.lock`. `magnesium --version` prints this version, and the runtime refuses a plugin that names a different one.
3. Open a pull request titled `chore(release): raise the version to 0.3.0`, and merge it as described in Make a change. End its description with `Refs #N`, not `Closes #N`, because the release does not exist until step 4.
4. Create the tag and its release page on the merged version bump, not on whatever is newest on `main`, which may include a pull request merged after it. Find the bump's squash commit with `gh pr view <pull-request-number> --json mergeCommit -q .mergeCommit.oid`, then run:

   ```sh
   gh release create v0.3.0 --target <squash-commit-sha> --generate-notes
   ```

   `--generate-notes` writes release notes that list the pull requests merged since the previous release. Then close the release issue.

### The Mac download

The study window ships as a disk image for Apple Silicon Macs, with the `magnesium` program inside it. The workflow **Mac download** (`.github/workflows/mac-download.yml`) builds it on a GitHub-hosted Mac, so that no file in it holds a builder's home folder path. It never runs on a push or a pull request.

1. On GitHub, open Actions, then Mac download, then Run workflow, on `main`. Give a release-candidate tag such as `v0.3.0-rc.1`. Its version must be the `version` in `Cargo.toml`.
2. The workflow builds the window and the program from that commit, passes the commit to both builds, signs the app ad hoc, checks that the program reports the commit and that no file holds a home folder path, and uploads `Magnesium-0.3.0-rc.1-arm64.dmg` to a draft pre-release.
3. Check a copy downloaded with a browser, as [Download](README.md#download) in the README describes. A copy built on your own Mac carries no download flag, so it shows nothing of what first open does.
4. Publish the draft yourself. The workflow never does. As a pre-release, it does not change what `gh release view` prints, so the install guide still finds the newest release.

`app/scripts/package.sh` is the build the workflow runs.
