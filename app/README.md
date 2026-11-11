# Magnesium's window

One window, made for studying. It sits over [Magnesium](https://github.com/samfricher/magnesium) and the coding agent you already use. It adds no teaching rule of its own: a lesson behaves the same here and in a terminal.

The window opens on your library: every course, with the nearest exam first. A course shows one card that says what to do next. A lesson fills the window with one column: what the tutor said, and one box to answer in.

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
- **Your study folders.** The library lists every study folder the window knows. **Open a folder** adds one, and **New folder** makes one. **Find folders on this Mac** looks through your home folder for folders that Magnesium was set up in from a terminal, and adds them. It runs only when you press it, it reads folder names only, and macOS may ask whether the app may read Documents or Desktop. A folder Magnesium has never been set up in is set up when you press Start. A folder an earlier version set up asks once, then is upgraded: Magnesium keeps a copy of its record in `.magnesium/backups` before it changes it. Say "Not now" and the folder stays as it was, and the window starts no lesson in it. **Remove** on a course takes it out of the library and leaves the folder as it is.

Setting a folder up also writes two things outside it: Magnesium's plugin for Claude Code in `~/Library/Application Support/magnesium`, and one entry in Claude Code's settings that names it. The [install guide](../docs/install.md#5-set-up-a-study-folder) lists every file.

The window shows its version and the commit it was built from at the foot of the setup screen and of ⌘K. The program inside reports the same commit, as `magnesium --version` prints it: `magnesium 0.3.0+<commit>`.

## Run from source

From the `app` folder of the Magnesium repository:

```sh
./run ~/path/to/your/study-folder
```

The first run builds the app, which takes a few minutes. After that, `./run` alone opens the library, and `./run` with a folder opens that course. The built app is at `target/release/bundle/macos/Magnesium.app` and also opens with a double click.

You need Claude Code, `magnesium` on your PATH (see the [install guide](../docs/install.md)), Rust and Node. A build from source carries no `magnesium` of its own: it uses the one installed.

To click through every screen with made-up data and no agent, run `npm run dev` and open http://127.0.0.1:1430 in a browser. The end of the address chooses the screen: `#library`, `#course`, `#session:answer`, `#summary`, `#progress`, `#paper:running`. `#welcome:replace`, `#welcome:signed-out`, `#welcome:no-claude`, `#welcome:key` and `#course:upgrade` show the setup states. `scripts/shots.mjs` lists them all.

## What is on screen

| Screen | What it holds | What it leaves out |
|---|---|---|
| **Library** | One card for each course: its next exam and the days left, what you last worked on, its level (or, for a course with no plan, how many skills are proven and in progress), and how much is due today. The most urgent course is the large card, and Enter opens it. This week's study days. | Statistics, a list of sessions, anything about a course that does not help you choose it. |
| **Course** | One card: what comes next, where you stopped, and one button. Under it, three tiles: your proofs, a timed paper, a new lesson. | Everything else about the course. |
| **Lesson** | The tutor's reply, appearing as it is written, one answer box, "Hint", "I don't know" and "I'm lost". While the tutor works, one line that names the work in hand, such as "Writing paper 2 · 12 min". In the top strip: the goal's steps as marks, and the minutes left in the block. A click on the steps shows the goal. When the tutor asks you to choose, the answer box gives way to the choices, so there is one thing to answer. | Tool calls, save messages, token counts, the model's name, other sessions, a sidebar. The working line never shows a tool's name, a command, or a path outside the study folder. The line the window sends to open a sitting is shown as one quiet line, not as something you said. |
| **Sitting done** | The proofs earned in the sitting, when there are any. Where you stopped. How many skills can earn a proof tomorrow. Today's place in the week. | Any proof the runtime's record does not show. The window compares the record before and after the sitting, and never takes the tutor's word for a proof. |
| **Proofs** | One row for each skill, with three dots: unaided answers on separate days. The level, the papers' scores, and five weeks of study days. | Minutes and sessions. Answers given with help are listed apart and never counted. |
| **Paper** | A countdown and Hand in. The tutor is hidden until you hand in. The window never opens an answer key. | Everything else. |

Everything that is not the lesson is behind one key, **⌘K**: the library, your proofs, a paper, a new lesson, the block length, this Mac's setup.

Other keys: **Enter** sends, **Shift+Enter** makes a new line, **⌘.** stops a reply, **⌘N** starts a new lesson, **⌘O** opens a folder, **Esc** goes back.

### How it looks, and why

- **One accent colour.** Green means two things only: what you have proved, and what moves you on (the task in hand, and the one button). Amber means due. Red means a fault. Everything else is ink: a study day, a step done, a clock, a chosen option.
- **Two voices.** What is read is set in a serif: a title, and the tutor's words, at book size, in a narrow column. What is pressed or glanced at is small, in the system's own face.
- **The task is set apart.** When the tutor opens a paragraph with a bold "Task." or "Check 1 of 2.", that paragraph and everything after it go in one box, so the thing to answer is never lost under the explanation.
- **Mathematics everywhere.** A reply, a goal, a step, a skill's name, a question's options and your own answer all draw `$…$`, `$$…$$`, `\(…\)`, `\[…\]` and bare environments such as `\begin{aligned}`. While the tutor is still writing, a formula appears only when it is whole, so its source is never on screen. The answer box shows your own mathematics drawn as you type it. The **Σ** button puts the dollar signs in.
- **Nothing fades in from nothing.** macOS pauses a page's animations while its window is covered. A screen that started invisible would stay blank in a capture of the window, so things arrive by moving.

### Progress, and what counts

The window counts one thing as progress: a **proof**, which is one answer you gave right, alone and cold, on its own day. Three proofs prove a skill. The runtime decides every proof and the level, and prints them with `magnesium progress`. The window only draws them.

- **Every skill can earn proofs.** A skill you got wrong, on a paper or in a quick check, earns them by the redos the tutor schedules. A skill you never got wrong earns one for each day you answer it right, alone and cold, for example on a timed paper. So a learner who loses no marks still moves up.
- **Level.** A course with a plan has a level from 1 to 10. It is the share of the plan's proofs you have earned, in tenths, over the skills of the plan's course graph, so it moves only when a proof is earned, it can reach 10, and starting something new never lowers it. A course with no plan has no level: the window shows how many skills are proven and how many are in progress. The ring shows the same share.
- **Quick checks count.** A quick check from a chat lesson is a skill like any other. It shows in the list, in "due today", and at the end of a sitting when you pass it.
- **The run.** A study day is a day on which the tutor marked an answer of yours, in any course in the library, in this window or in a terminal. A message alone does not make one, and neither does a question you declined. The run counts study days in a row. It is forgiving on purpose, because a run that one bad day can end becomes a reason to stop. Today does not end a run before the day is over. One missed day in a week is a rest day: it is not counted, and the run goes on through it. Two missed days in a row, or a second missed day within a week, end the run. The window shows no longest run, and a missed day is never marked in red. No proof depends on the run.
- **Never counted.** Minutes, opening the app, and answers given with help.
- **Skipping the checks.** A sitting opens with the quick checks and paper questions that are due, because answering them again is what earns a proof. "Continue without the quick checks" is in ⌘K, not beside the Continue button.

## What must stay true

- **The window is a layer.** Every teaching rule stays in the Magnesium runtime and its plugin. A button sends plain learner words, such as "I'm lost" or "Can I have a hint?". A lesson started here continues in a terminal with `claude --continue`. The one difference is how a reply reaches the screen: in the window the tutor sends it through Magnesium's `reply` tool, whose words stream as they are written, where text after a tool call would arrive in one piece.
- **Your own agent and subscription.** The window starts the `claude` program you installed, as published, in your study folder. Signing in is Claude Code's own (`claude auth login`), in your browser. The window reads no credential, and never passes `--bare`, which would skip Magnesium's hooks.
- **A lesson loads what it needs and nothing else.** The window starts Claude Code with Magnesium's plugin, from where `magnesium init` put it, and nine of Claude Code's own tools: Read, Write, Edit, Glob, Grep, AskUserQuestion, WebSearch, WebFetch, and Bash inside Claude Code's sandbox. Write and Edit never touch `.magnesium/`. Settings come from the study folder only, so your other plugins, hooks, skills and tool servers stay out, and so do your account's connectors, such as mail and calendar. The model is named (`claude-fable-5-2`, Claude Code's default on 2026-11-11), so a later change of that default cannot change the tutor unseen. A lesson in a terminal is unchanged.
- **Local files.** No cloud service, no account, no telemetry. The window makes no network request of its own, and an image from the web inside a reply is not loaded. It looks for study folders only when you press **Find folders on this Mac**.
- **The runtime stays small.** The window links none of the runtime's code. It runs `magnesium digest` and `magnesium progress`, and reads your plan files.

## What was checked

| Check | How to run it | Result on 2026-11-11 |
|---|---|---|
| Everything below but the live check | `scripts/check.sh`, and `scripts/check.sh --window` for the self-check too | passes |
| Core: readers, state, files, the rows and the level from the runtime's record, the run of study days and its rest day, the look for study folders | `cargo test -p mg-core --lib` | 58 pass |
| The library: every folder is listed, the nearest exam first, and a folder that was never set up is shown as new | `cargo test -p mg-core --test folders` | passes, against a stand-in runtime |
| The pipe: several turns in one process, permission allowed once, always and refused, a question answered, a reply interrupted, a lesson closed and resumed, a missing plugin reported, a helper's text hidden, a usage limit shown, a conversation the agent no longer has, a reply interrupted before its first word, a stopped or replaced conversation that says nothing more | `cargo test -p mg-core --test pipe` | 14 pass, against a stand-in agent |
| Paying: with the plan chosen, an API key in the environment does not reach the agent; with a key chosen, it does; a key the agent keeps itself is named before and during a lesson; signed out, the agent's own sign-in runs and the window sees the result; without the agent, its install page | `cargo test -p mg-core --test payment` | 3 pass, against the stand-in agent |
| The program inside the app: installed once without a question, another version kept or replaced only on a yes, asked again for a new version | `cargo test -p mg-core --test install` | passes, against stand-in programs |
| A folder an earlier version set up: the window asks, starts no lesson, changes nothing, then upgrades with a copy of the record | `cargo test -p mg-core --test upgrade` | passes, against the runtime built from this checkout |
| A paper opens from a folder inside the study folder, and an answer key does not | `cargo test -p mg-core --test papers` | passes, against a stand-in runtime |
| The readers against the runtime built from this checkout: its digest, `progress --json`, `attempts --json`, the gap log it writes, the goal's steps, the proofs and the level on the progress page and on the library's card, a quick check from a chat lesson, the study days, and a gap log edited by hand | `cargo test -p mg-core --test runtime` | passes |
| The real window, end to end: page-to-shell calls, the setup calls and the choice of payment, the library and the study days, the content policy, mathematics in a reply, in one line of text and in the learner's own answer, a half-written formula held back, a task set apart, a diagram, a permission card, the end of a sitting, stop and resume | `scripts/selftest.sh` | passes, against the stand-in, on the debug build and on the release build |
| The release build's window, looked at: the library, a course, a lesson with formulas, matrices and a table, the steps, the end of a sitting and the proofs | by eye, on made-up courses and the stand-in agent | drawn as in the browser, on 2026-11-11 |
| **The pipe against the real Claude Code**: a new lesson, a second turn, two interrupts, Stop pressed during a reply, and a resume | `scripts/live-check.sh` | Stop during a reply is new and not yet run. The rest passed on 2026-11-10, with Claude Code 2.1.322 and the runtime of 2026-10-23 |
| The download: both programs report the commit, the signature verifies, no file holds a home folder path, and the disk image verifies | `scripts/package.sh <version>` | passes on a Mac. Only a copy downloaded with a browser shows what first open does |

`npm run shots` photographs every screen with made-up data into `shots/`, in the dark scheme and, for the main screens, the light one. `cargo run -p mg-core --example look -- <folder>` prints what the library, the course page, the lesson's steps and the progress page would show for a folder.

The live check uses your subscription for six short turns in a scratch folder. It keeps every line between the window and the real program in a trace file, whose path it prints. Each line starts with its direction and the milliseconds since the conversation started. The trace of 2026-11-10 showed:

- In a folder Claude Code has never opened, the session-start hook runs and the plugin starts to install. Then the program answers `initialize`, finishes the install, and sends `system/init`. So the window judges the plugin from `system/init`, never from the `initialize` reply.
- An interrupt before the tutor's first word ends the turn with a `[Request interrupted by user]` line and a `result` of `error_during_execution` whose only error is the program's own diagnostic. The window shows it as interrupted, not as an error.
- An interrupt during a reply ends the turn within about 30 ms. The program sends a reply's text in bursts, so text that had already arrived stays on screen.
- `resetsAt` on a usage-limit line is in seconds.

## Which runtime it runs

The download installs its own copy, as above. Run from source, the window runs whichever `magnesium` is on the path.

A runtime from this repository after 2026-11-10 never upgrades a folder just because `magnesium digest` looked at it. For a folder an earlier version set up, the digest says `upgrade_needed` and holds no record, and the window asks before it runs `magnesium init --upgrade-instructions`.

- **The runtime of 2026-10-23.** It keeps no quick checks, has no `attempts` command and stores no goal steps. The course page shows no checks, the lesson's top strip shows the goal and the next action, and the progress page lists no answers given with help. The tutor writes the gap log, and the window reads it as written.
- **The runtime on `main` from 2026-11-10** (Magnesium issues 29, 30 and 31). It has no `progress` command, so the window reads the Gaps table that the runtime writes into each gap log. It shows no level, because only gap-log rows could earn proofs there and a level over them would have a low ceiling. Quick checks are kept where the window cannot read them, so they are not shown. The run falls back on the days an answer was sent from the window. If the file differs from the record, as after a hand edit, the progress page says so and shows no rows until the runtime writes the table again.
- **The runtime of this pull request** (Magnesium issue 46). `magnesium progress --json` prints each skill's proofs, the level, what is due and the days with a marked answer. The window takes all four from it, so a hand edit to a gap log changes nothing on screen. For a plan whose gap log the runtime has not read into the record yet, the command says so, and the window reads that plan's file and shows no level until the tutor next marks an answer.

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
  src/study.rs      what the library, the course page and the progress page
                    show: the cards, the proofs and the level
  src/activity.rs   the days studied in the window, and the run they make
  src/find.rs       looks for study folders set up in a terminal
  src/gaplog.rs     reads plan.md, gap-log.md and graph.yaml
  src/peek.rs       two facts read straight from the runtime's files (see below)
  src/manager.rs    one open folder and its one conversation
  tests/            the pipe tests, the stand-in agent, the readers against
                    the runtime, paying and signing in, installing, upgrading
                    a folder, the live check
src-tauri/          the window's shell: passes messages, decides nothing
src/                the pages (React, plain CSS)
  App.tsx           which screen is up, and what moves between them
  screens/          one file for each screen
  ui/               the shared parts: text with mathematics, icons, the
                    ring, the proof dots, the week
  render.ts         Markdown and mathematics to safe HTML
  styles/           base.css (colours, type, controls), prose.css (the
                    tutor's words), screens.css
scripts/            check, selftest, live check, screenshots, package (the
                    download, which .github/workflows/mac-download.yml runs
                    on a GitHub-hosted Mac); make_icon.py draws
                    the app icon (three proof dots on a dark tile) as a 1024-pixel
                    PNG, written to the path given as its argument
```

The window's own state is in `~/Library/Application Support/dev.magnesium.window/`, apart from the runtime's folder: the folders in your library, each folder's conversation id and stop line, a copy of the turns it showed, how many messages you sent on each day, how you chose to pay for lessons, and a `magnesium` version you chose to keep. None of it is learning state, and none of it is a credential.

## What is not there yet

- **A check on the course page.** The course page says how many checks are due, and the tutor asks them. It could show the first one in full, with its key locked until you answer. The runtime on `main` stores the next question and its key for each due item (Magnesium issue 30), but no command prints them yet.
- **One fact comes from the runtime's files, not from a command**: the steps of the current goal. `peek.rs` reads them read-only and gives nothing when the layout changes. It should become a runtime command. `peek.rs` also still reads the quick checks of the runtime of 2026-10-23, which has no `progress` command.
- **Goal steps need a newer runtime.** The top strip shows steps when the checkpoint holds them. The runtime of 2026-10-23 stores none, and the strip then shows the goal and the next action.
- **A step's tick is the tutor's judgment.** The stored field is the tutor's own `done`, so the window shows the tick and does not say that you produced the step alone.
- **A proof needs a marked answer.** The end of a sitting shows the proofs the record gained. If the tutor did not mark an answer, the record did not change, and the screen shows none.
- **A skill never got wrong has no redo date.** The tutor schedules redos only for what you got wrong. A skill you never got wrong earns its next proof when a later paper or lesson asks it again, so its row shows no "next" day.
- **A sound skill can show fewer than three dots.** The tutor reads a skill as sound after one right, cold answer. The dots still count days, so that skill shows one dot until it is answered right on two more days.
- **A task is found by its first words.** The box round a task appears when the tutor opens a paragraph with a bold "Task.", "Check", "Question" or a similar word. A task written any other way is shown as plain text.
- **A photo is sent as a path.** The window saves a shrunk JPEG in `Study/handed-in/` and tells the tutor where it is. The tutor reads the file.
- **No reader for sources, no Apple signing or notarization, no update from inside the app.** Apple's signing needs a paid Apple membership. Until then each new version needs the one-time approval again.
- **Claude Code is checked, not installed.** Anthropic's terms set conditions for a product that installs or runs Claude Code. Whether they apply to an app that starts the user's own copy is an open question (Magnesium issue 38).
