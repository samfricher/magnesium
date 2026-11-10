# Magnesium's window

One window, made for studying. It sits over [Magnesium](https://github.com/samfricher/magnesium) and the coding agent you already use. It adds no teaching rule of its own: a lesson behaves the same here and in a terminal.

Built 2026-11-10. macOS, run from source. Claude Code is the first agent.

## Open it

From the `app` folder of the Magnesium repository:

```sh
./run ~/path/to/your/study-folder
```

The first run builds the app, which takes a few minutes. After that, `./run` alone opens the folder you had open last time. The built app is at `target/release/bundle/macos/Magnesium.app` and also opens with a double click.

You need `claude` (signed in once in a terminal), `magnesium`, Rust and Node.

To click through every screen with made-up data and no agent, run `npm run dev` and open http://127.0.0.1:1430 in a browser.

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
- **Your own agent and subscription.** The window starts the `claude` program you installed, as published, in your study folder. It shows no sign-in screen, reads no credential, and never passes `--bare`, which would skip Magnesium's hooks.
- **Local files.** No cloud service, no account, no telemetry. The window makes no network request of its own, and an image from the web inside a reply is not loaded.
- **The runtime stays small.** The window links none of the runtime's code. It runs `magnesium digest` and reads your plan files.

## What was checked

| Check | How to run it | Result on 2026-11-10 |
|---|---|---|
| Everything below but the live check | `scripts/check.sh`, and `scripts/check.sh --window` for the self-check too | passes |
| Core: readers, state, files, which folder opens first | `cargo test -p mg-core --lib` | 35 pass |
| The pipe: several turns in one process, permission allowed once, always and refused, a question answered, a reply interrupted, a lesson closed and resumed, a missing plugin reported, a helper's text hidden, a usage limit shown, a conversation the agent no longer has, a reply interrupted before its first word, a stopped or replaced conversation that says nothing more | `cargo test -p mg-core --test pipe` | 14 pass, against a stand-in agent |
| A paper opens from a folder inside the study folder, and an answer key does not | `cargo test -p mg-core --test papers` | passes, against a stand-in runtime |
| The readers against the runtime built from this checkout: the gap log it writes, its digest, `attempts --json`, the goal's steps, and a gap log edited by hand | `cargo test -p mg-core --test runtime` | passes |
| The real window, end to end: page-to-shell calls, the content policy, mathematics, a diagram, a permission card, stop and resume | `scripts/selftest.sh` | passes, against the stand-in |
| **The pipe against the real Claude Code**: a new lesson, a second turn, two interrupts, a stop and a resume | `scripts/live-check.sh` | **passes**, with Claude Code 2.1.322 and the runtime of 2026-10-23 |

`npm run shots` photographs every screen with made-up data into `shots/`, to set beside the mock. `cargo run -p mg-core --example look -- <folder>` prints what the entrance, rail and progress page would show for a folder.

The live check uses your subscription for five short turns in a scratch folder. It keeps every line between the window and the real program in a trace file, whose path it prints. Each line starts with its direction and the milliseconds since the conversation started. The trace of 2026-11-10 showed:

- In a folder Claude Code has never opened, the session-start hook runs and the plugin starts to install. Then the program answers `initialize`, finishes the install, and sends `system/init`. So the window judges the plugin from `system/init`, never from the `initialize` reply.
- An interrupt before the tutor's first word ends the turn with a `[Request interrupted by user]` line and a `result` of `error_during_execution` whose only error is the program's own diagnostic. The window shows it as interrupted, not as an error.
- An interrupt during a reply ends the turn within about 30 ms. The program sends a reply's text in bursts, so text that had already arrived stays on screen.
- `resetsAt` on a usage-limit line is in seconds.

## Which runtime it runs

The window runs whichever `magnesium` is on the path.

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

**Codex and OpenCode** each need a second file beside `claude.rs` that implements the same two traits: start a conversation in a folder, send words, stream the reply, surface approvals, interrupt, resume by id. Codex has an app-server protocol that fits. OpenCode has a local server with sessions and events. Neither is written.

## Where things are

```
crates/core/        everything that is not drawing, with its tests
  src/agent.rs      the interface to a coding agent
  src/claude.rs     Claude Code behind that interface
  src/runtime.rs    runs the magnesium program and reads what it prints
  src/study.rs      what the entrance and progress page show
  src/gaplog.rs     reads plan.md, gap-log.md and graph.yaml
  src/peek.rs       two facts read straight from the runtime's files (see below)
  src/manager.rs    one open folder and its one conversation
  tests/            the pipe tests, the stand-in agent, the readers against
                    the runtime, the live check
src-tauri/          the window's shell: passes messages, decides nothing
src/                the pages (React, plain CSS)
scripts/            check, selftest, live check, screenshots; make_icon.py draws
                    the app icon (three proof dots on a dark tile) as a 1024-pixel
                    PNG, written to the path given as its argument
```

The window's own state is in `~/Library/Application Support/dev.magnesium.window/`, apart from the runtime's folder: the folders you opened, each folder's conversation id and stop line, and a copy of the turns it showed. None of it is learning state.

## What is not there yet

- **A check with its key on the entrance.** The mock shows the first due check in full, with the key locked until you answer. The runtime on `main` now stores the next question and its key for each due item (Magnesium issue 30), but no command prints them yet.
- **Two facts come from the runtime's files, not from a command**: the quick checks, and the steps of the current goal. `peek.rs` reads them read-only and gives nothing when the layout changes. Against the runtime on `main` it cannot count the quick checks at all, because that runtime works out what is due each time and stores no row that says it. Each fact should become a runtime command.
- **Goal steps need a newer runtime.** The rail shows steps when the checkpoint holds them. The runtime of 2026-10-23 stores none, and the rail then shows the goal and the next action.
- **A step's tick is the tutor's judgment.** The mock says a tick means "you produced it". The stored field is the tutor's own `done`, so the window shows the tick without that claim.
- **A photo is sent as a path.** The window saves a shrunk JPEG in `Study/handed-in/` and tells the tutor where it is. The tutor reads the file.
- **No reader for sources, no installer, no signing, no auto-update.**
