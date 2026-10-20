# Install Magnesium

This document is for a coding agent. The user handed you this repository and asked you to install Magnesium. Follow the steps in order. Run the check after each step. If a check fails, stop and tell the user what failed.

## 1. What you are installing

You are installing a command-line runtime, `magnesium`, and a plugin for the user's assistant (Claude Code, Codex or OpenCode). It runs with the user's privileges and writes only to the folders that step 5 names.

## 2. Ask before you install anything

The build in step 4 needs no permission beyond this repository. Ask the user before every other install, including prerequisites and the optional tools in step 6. Say in one sentence what the tool is for. If the user says no, accept it, say what the fallback is, and continue.

Also ask before you change the user's shell profile or copy a file into a folder on their PATH.

## 3. Prerequisites

| Tool | What it is for | Check | If it is missing, ask |
| --- | --- | --- | --- |
| Rust, through rustup | Builds Magnesium. The repository pins its toolchain in `rust-toolchain.toml`, and rustup fetches that version on the first build. | `cargo --version` | "May I install Rust with rustup? Magnesium is built from source with it." |
| Git | Checks out the newest release now and at each update. | `git --version` | "May I install Git? I use it to get the newest release of Magnesium, now and when you update." |
| Git Bash (Windows only) | The assistant runs Magnesium's hooks through it. | `bash --version`, from a Git Bash prompt | "May I install Git for Windows? Its Git Bash runs Magnesium's hooks." |

## 4. Build the newest release and place the binary

Build the newest release, not the `main` branch. A release is a Git tag, such as `v0.2.0`. The `main` branch can hold changes that are not released yet.

Find the newest release tag with either command:

- With the GitHub CLI: `gh release view --repo samfricher/magnesium --json tagName -q .tagName`
- With Git alone: `git tag --sort=-v:refname | head -1`. Run it in a copy of the repository, after `git fetch --tags`.

If the GitHub CLI is missing or fails, use the Git command. Do not install the GitHub CLI for this step.

From the repository root, check out the newest release:

```sh
git fetch --tags
git checkout <newest-tag>
```

Git then reports a detached HEAD. A detached HEAD is expected here.

If you have no copy of the repository yet, clone the newest release instead:

```sh
git clone --branch <newest-tag> https://github.com/samfricher/magnesium.git
```

To find the tag with Git alone, clone without `--branch` first. Then check out the tag as above.

Check: `git describe --tags --exact-match` prints the newest tag.

Then, from the repository root, run:

```sh
cargo build --locked --release
```

The binary is `target/release/magnesium` on macOS and Linux, and `target\release\magnesium.exe` on Windows.

Put the binary on the user's PATH. Ask first, then do one of these:

- Copy it into a folder that is already on PATH, such as `~/.local/bin` on macOS or Linux.
- Add the repository's `target/release` folder to PATH in the user's shell profile.

The assistant starts `magnesium` by name, so the command must resolve in a new terminal.

Check: `magnesium --version` prints `magnesium` and a version number.

## 5. Set up a study folder

Ask the user which folder holds their course. Then run, in that folder:

```sh
magnesium init
```

`magnesium init` writes these things:

- `.magnesium/` in the study folder. It holds the learning history (`state.sqlite`), the teaching instructions, the source search cache and `config.toml`.
- A per-user plugin folder: `~/Library/Application Support/magnesium` on macOS, `$XDG_DATA_HOME/magnesium` or `~/.local/share/magnesium` on Linux, and `%LOCALAPPDATA%\magnesium` on Windows. It holds the Claude Code plugin copy and the Codex marketplace.
- For Claude Code, one `extraKnownMarketplaces` entry in `~/.claude/settings.json` (or in `$CLAUDE_CONFIG_DIR/settings.json`). That entry names the plugin folder and enables nothing.
- For Claude Code, `.claude/settings.local.json` in the study folder. It turns the plugin on for this folder only. It also allows Magnesium's tools, reads of the plugin folder, the three Magnesium skills and `magnesium digest`.
- In a Git repository, entries for `.magnesium/` and `.claude/settings.local.json` in `.git/info/exclude`.
- When the `codex` command exists, the Magnesium marketplace and plugin in Codex.
- When the `opencode` command exists, the OpenCode plugin, its skill and an MCP entry in OpenCode's config folder.
- With `--adapter agents-md` only, a bounded Magnesium block in the folder's `AGENTS.md`.

`magnesium init` prints one result line for each assistant. Tell the user what it wrote.

Checks:

- The study folder has a `.magnesium/` folder.
- Claude Code: `.claude/settings.local.json` lists `magnesium@magnesium` under `enabledPlugins`. In a new Claude Code conversation in the study folder, `/magnesium:help` answers with a short guide.
- Codex: run `codex` from the study folder. Approve the Magnesium hooks if Codex asks.
- OpenCode: do any manual registration step that `magnesium init` printed. Then run `opencode` from the study folder.

## 6. Optional tools

| Tool | What it enables | Ask | If the user says no |
| --- | --- | --- | --- |
| tectonic | PDF practice papers with real math notation | "May I install tectonic? It lets me build your practice papers as PDFs with proper math notation. Without it I write them in Markdown or HTML, which also works." | The tutor writes papers in Markdown or HTML. |
| HEIC conversion | Reading photos of handwritten answers from an iPhone | "May I install a HEIC converter? It lets me read the photos your iPhone takes of your answers. Without it, send your answers as PDF or JPEG." | The learner sends PDF or JPEG files. |

After a yes, install the tool the way the host operating system installs software. Then check that its command runs.

## 7. Record the choices

Write each answer into the study folder's `.magnesium/config.toml`, in a `[tools]` table at the end of the file. Keep the other keys above it unchanged. Keys below a table header belong to that table, so a table placed higher would capture `schema_version` and `workspace_id`:

```toml
[tools]
tectonic = "installed"
heic = "declined"
```

Use `"installed"` or `"declined"`. The papers skill reads this table, so a later conversation does not ask again. Each study folder keeps its own answers. When a new folder holds a math course and tectonic is still absent, the tutor may ask once more.

## 8. Update

Run these in order:

1. In the repository, run `git fetch --tags`, then `git checkout <newest-tag>`. Step 4 shows how to find `<newest-tag>`.
2. `cargo build --locked --release`.
3. Put the new binary in place as in step 4.
4. In each study folder, run `magnesium init --upgrade-instructions`. This command does everything that `magnesium init` does. It also switches the folder to the new teaching instructions, and the plugin copy then matches the binary.

Checks: `magnesium --version` prints the new version, and `/magnesium:help` answers in a new conversation.

`magnesium update` prints these steps. It also refreshes the plugin copies that `magnesium init` installed. It never downloads anything.

## 9. Windows

Git Bash is required. The hooks and the permission rules assume it. PowerShell alone does not work. The tests pass on Windows, but no one has run the full learning loop there yet. Report problems as issues in this repository.

## 10. Done

Report these to the user:

- The output of `magnesium --version`.
- Each study folder that you set up.
- Each optional tool, and whether it was installed or declined.
- Anything that you could not do, and why.
