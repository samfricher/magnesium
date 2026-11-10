# Magnesium's window

One window, made for studying. It sits over [Magnesium](https://github.com/samfricher/magnesium) and the coding agent you already use. It adds no teaching rule of its own: a lesson behaves the same here and in a terminal.

macOS on Apple Silicon. Claude Code is the agent.

## Download

1. From the [releases page](https://github.com/samfricher/magnesium/releases), download the newest `Magnesium-<version>-arm64.dmg` and open it.
2. Drag Magnesium to Applications, then open it from Applications.
3. macOS says it could not check the app for malware. Choose **Done**.
4. Open **System Settings → Privacy & Security**. Under Security, choose **Open Anyway** next to Magnesium, and confirm.

Step 4 is needed once for each new version. The app is signed ad hoc, which is free and needs no Apple account, but it is not signed by Apple. If macOS ever says the app is damaged, the copy did not come from the release page: download it again.

You need [Claude Code](https://code.claude.com/docs/en/setup), and a paid Claude plan or an Anthropic Console account. No terminal step is needed.

### What the app sets up

The first screen shows each step, and moves on when they are done.

- **The `magnesium` program.** It is inside the app, built from the same commit. On first open the app puts it in `~/.local/bin`, where Claude Code's own installer puts `claude`. The window, Claude Code's hooks and a terminal then all run that one copy. If another version is already installed, the app asks before it replaces it, because a newer program upgrades each study folder's record when it opens the folder. If you keep the other version, lessons run on it, and the app says what that version cannot do.
- **Claude Code.** The app looks for `claude` and asks it whether you are signed in (`claude auth status`). Signed out, a button starts Claude Code's own sign-in (`claude auth login`), which opens Anthropic's page in your browser. Without Claude Code, the app links to Anthropic's install page and offers Check again. The app never installs Claude Code, and never reads or stores a credential.
- **How lessons are paid for.** Choose your Claude plan, the default, or an API key, paid per use. The first screen and ⌘K show which one Claude Code would use. With the plan chosen, the app starts Claude Code without the `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` variables, because Claude Code started by another program uses such a key over the plan without asking. A key that Claude Code keeps itself, such as an API key helper in its settings, cannot be held back: the app then says the lesson is not paid with the plan.
- **A study folder.** Open a folder, or make a new one with **New folder…**. A folder Magnesium has never been set up in is set up when you press Start. A folder an earlier version set up asks once, then is upgraded: Magnesium keeps a copy of its record in `.magnesium/backups` before it changes it. Say "Not now" and the folder stays as it was, and the window starts no lesson in it.

Setting a folder up also writes two things outside it: Magnesium's plugin for Claude Code in `~/Library/Application Support/magnesium`, and one entry in Claude Code's settings that names it. The [install guide](../docs/install.md#5-set-up-a-study-folder) lists every file.

The window shows its version and the commit it was built from at the foot of the first screen and of ⌘K. The program inside reports the same commit, as `magnesium --version` prints it: `magnesium 0.3.0+<commit>`.

## Run from source

From the `app` folder of the Magnesium repository:

```sh
./run ~/path/to/your/study-folder
```

The first run builds the app, which takes a few minutes. After that, `./run` alone opens the folder you had open last time. The built app is at `target/release/bundle/macos/Magnesium.app` and also opens with a double click.

You need Claude Code, `magnesium` on your PATH (see the [install guide](../docs/install.md)), Rust and Node. A build from source carries no `magnesium` of its own: it uses the one installed.

To click through every screen with made-up data and no agent, run `npm run dev` and open http://127.0.0.1:1430 in a browser. `#welcome:replace`, `#welcome:signed-out`, `#welcome:no-claude`, `#welcome:key` and `#entrance:upgrade` at the end of the address show the setup screens.

## What is on screen

| Screen | What it holds | What it leaves out |
|---|---|---|
| **Entrance** | The course, the next exam and days left, where you stopped, how many quick checks are due, and one Start button. | Statistics, a session list, a graph, a streak, any count above three. |
| **Session** | The goal and its steps, the tutor's current reply, one answer box, "I don't know" and "I'm lost", the clock, Stop. | Tool calls, save messages, token counts, the model's name, other sessions. |
| **Progress** | One row per lost mark or missed check, as three dots: unaided passes on separate days. Drawn as rows, a map or levels. | Minutes, streaks, sessions, percentages. Answers given with help are listed apart and never counted. |
| **Paper** | A countdown and Hand in. The tutor is hidden until you hand in. The window never opens an answer key. | Everything else. |

Everything that is not the lesson is behind one key, **⌘K**: progress, papers, a new lesson, the block length, another folder.

Other keys: **Enter** sends, **Shift+Enter** makes a new line, **⌘.** stops a reply, **⌘N** starts a new lesson.

## What must stay true

- **The window is a layer.** Every teaching rule stays in the Magnesium runtime and its plugin. A button sends plain learner words, such as "I'm lost". A lesson started here continues in a terminal with `claude --continue`.
- **Your own agent and subscription.** The window starts the `claude` program you installed, as published, in your study folder. Signing in is Claude Code's own (`claude auth login`), in your browser. The window reads no credential, and never passes `--bare`, which would skip Magnesium's hooks.
- **Local files.** No cloud service, no account, no telemetry. The window makes no network request of its own, and an image from the web inside a reply is not loaded.
- **The runtime stays small.** The window links none of the runtime's code. It runs `magnesium digest` and reads your plan files.

## What was checked

| Check | How to run it | Result on 2026-11-10 |
|---|---|---|
| Everything below but the live check | `scripts/check.sh`, and `scripts/check.sh --window` for the self-check too | passes |
| Core: readers, state, files, which folder opens first | `cargo test -p mg-core --lib` | 35 pass |
| The pipe: several turns in one process, permission allowed once, always and refused, a question answered, a reply interrupted, a lesson closed and resumed, a missing plugin reported, a helper's text hidden, a usage limit shown, a conversation the agent no longer has, a reply interrupted before its first word, a stopped or replaced conversation that says nothing more | `cargo test -p mg-core --test pipe` | 14 pass, against a stand-in agent |
| Paying: with the plan chosen, an API key in the environment does not reach the agent; with a key chosen, it does; a key the agent keeps itself is named before and during a lesson; signed out, the agent's own sign-in runs and the window sees the result; without the agent, its install page | `cargo test -p mg-core --test payment` | 3 pass, against the stand-in agent |
| The program inside the app: installed once without a question, another version kept or replaced only on a yes, asked again for a new version | `cargo test -p mg-core --test install` | passes, against stand-in programs |
| A folder an earlier version set up: the window asks, starts no lesson, changes nothing, then upgrades with a copy of the record | `cargo test -p mg-core --test upgrade` | passes, against the runtime built from this checkout |
| A paper opens from a folder inside the study folder, and an answer key does not | `cargo test -p mg-core --test papers` | passes, against a stand-in runtime |
| The readers against the runtime built from this checkout: the gap log it writes, its digest, `attempts --json`, the goal's steps, and a gap log edited by hand | `cargo test -p mg-core --test runtime` | passes |
| The real window, end to end: page-to-shell calls, the setup calls and the choice of payment, the content policy, mathematics, a diagram, a permission card, stop and resume | `scripts/selftest.sh` | passes, against the stand-in |
| **The pipe against the real Claude Code**: a new lesson, a second turn, two interrupts, Stop pressed during a reply, and a resume | `scripts/live-check.sh` | Stop during a reply is new and not yet run. The rest passed on 2026-11-10, with Claude Code 2.1.322 and the runtime of 2026-10-23 |
| The download: both programs report the commit, the signature verifies, no file holds a home folder path, and the disk image verifies | `scripts/package.sh <version>` | passes on a Mac. Only a copy downloaded with a browser shows what first open does |

`npm run shots` photographs every screen with made-up data into `shots/`, to set beside the mock. `cargo run -p mg-core --example look -- <folder>` prints what the entrance, rail and progress page would show for a folder.

The live check uses your subscription for six short turns in a scratch folder. It keeps every line between the window and the real program in a trace file, whose path it prints. Each line starts with its direction and the milliseconds since the conversation started. The trace of 2026-11-10 showed:

- In a folder Claude Code has never opened, the session-start hook runs and the plugin starts to install. Then the program answers `initialize`, finishes the install, and sends `system/init`. So the window judges the plugin from `system/init`, never from the `initialize` reply.
- An interrupt before the tutor's first word ends the turn with a `[Request interrupted by user]` line and a `result` of `error_during_execution` whose only error is the program's own diagnostic. The window shows it as interrupted, not as an error.
- An interrupt during a reply ends the turn within about 30 ms. The program sends a reply's text in bursts, so text that had already arrived stays on screen.
- `resetsAt` on a usage-limit line is in seconds.

## Which runtime it runs

The download installs its own copy, as above. Run from source, the window runs whichever `magnesium` is on the path.

A runtime from this repository after 2026-11-10 never upgrades a folder just because `magnesium digest` looked at it. For a folder an earlier version set up, the digest says `upgrade_needed` and holds no record, and the window asks before it runs `magnesium init --upgrade-instructions`.

- **The runtime of 2026-10-23.** It keeps no quick checks, has no `attempts` command and stores no goal steps. The entrance shows no checks, the rail shows the goal and the next action, and the progress page lists no answers given with help. The tutor writes the gap log, and the window reads it as written.
- **The runtime on `main` from 2026-11-10** (Magnesium issues 29, 30 and 31). The runtime writes the Gaps table of each gap log from the learner record, and the window shows that table. If the file differs from the record, as after a hand edit, the progress page says so and shows no rows until the runtime writes the table again. Goal steps show in the rail, and answers given with help are listed apart. Quick checks are kept where only the runtime can work out what is due, so the entrance and the progress page do not count them. The tutor still asks them as each sitting opens.

Upgrading the runtime upgrades the learner record in each study folder it opens, once, after keeping a copy.

## How it talks to the agent

`crates/core/src/agent.rs` is the one interface. Nothing outside `claude.rs` knows which agent is running.

For Claude Code the window runs:

```
claude -p --input-format stream-json --output-format stream-json --verbose \
  --include-partial-messages --permission-prompt-tool stdio [--resume=<id>]
```

It keeps that one process open for the sitting and writes each of your messages to it as one line of JSON. Permission requests and the tutor's questions arrive as control requests on the same pipe. This is the wire format the Agent SDKs use. Closing the window ends the process, and the conversation stays on disk under its id.

**Another agent** would need a second file beside `claude.rs` that implements the same two traits: start a conversation in a folder, send words, stream the reply, surface approvals, interrupt, resume by id. None is written. Magnesium itself is built and tested for Claude Code only.

## Where things are

```
crates/core/        everything that is not drawing, with its tests
  src/agent.rs      the interface to a coding agent
  src/claude.rs     Claude Code behind that interface
  src/runtime.rs    runs the magnesium program and reads what it prints
  src/install.rs    puts the app's own magnesium on this Mac, asking first
                    before it replaces another copy
  src/study.rs      what the entrance and progress page show
  src/gaplog.rs     reads plan.md, gap-log.md and graph.yaml
  src/peek.rs       two facts read straight from the runtime's files (see below)
  src/manager.rs    one open folder and its one conversation
  tests/            the pipe tests, the stand-in agent, the readers against
                    the runtime, paying and signing in, installing, upgrading
                    a folder, the live check
src-tauri/          the window's shell: passes messages, decides nothing
src/                the pages (React, plain CSS); Setup.tsx is the first screen
scripts/            check, selftest, live check, screenshots, package (the
                    download, which .github/workflows/mac-download.yml runs
                    on a GitHub-hosted Mac); make_icon.py draws
                    the app icon (three proof dots on a dark tile) as a 1024-pixel
                    PNG, written to the path given as its argument
```

The window's own state is in `~/Library/Application Support/dev.magnesium.window/`, apart from the runtime's folder: the folders you opened, each folder's conversation id and stop line, a copy of the turns it showed, how you chose to pay for lessons, and a `magnesium` version you chose to keep. None of it is learning state, and none of it is a credential.

## What is not there yet

- **A check with its key on the entrance.** The mock shows the first due check in full, with the key locked until you answer. The runtime on `main` now stores the next question and its key for each due item (Magnesium issue 30), but no command prints them yet.
- **Two facts come from the runtime's files, not from a command**: the quick checks, and the steps of the current goal. `peek.rs` reads them read-only and gives nothing when the layout changes. Against the runtime on `main` it cannot count the quick checks at all, because that runtime works out what is due each time and stores no row that says it. Each fact should become a runtime command.
- **Goal steps need a newer runtime.** The rail shows steps when the checkpoint holds them. The runtime of 2026-10-23 stores none, and the rail then shows the goal and the next action.
- **A step's tick is the tutor's judgment.** The mock says a tick means "you produced it". The stored field is the tutor's own `done`, so the window shows the tick without that claim.
- **A photo is sent as a path.** The window saves a shrunk JPEG in `Study/handed-in/` and tells the tutor where it is. The tutor reads the file.
- **No reader for sources, no Apple signing or notarization, no update from inside the app.** Apple's signing needs a paid Apple membership. Until then each new version needs the one-time approval again.
- **Claude Code is checked, not installed.** Anthropic's terms set conditions for a product that installs or runs Claude Code. Whether they apply to an app that starts the user's own copy is an open question (Magnesium issue 38).
