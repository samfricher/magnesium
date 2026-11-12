# Magnesium's window

One window, made for studying. It sits over [Magnesium](https://github.com/samfricher/magnesium) and the coding agent you already use. It adds no teaching rule of its own: a lesson behaves the same here and in a terminal.

The window is the editor of one study folder. The folder's documents open in tabs, in one pane or two: your lessons, your plan, your course material, the record of your answers. It reopens exactly where you left it. A lesson is a page you write with the tutor: an answer goes inside the task, and anything you write outside a task is talk, which is never marked.

macOS on Apple Silicon. Claude Code is the agent.

## Download

1. From the [releases page](https://github.com/samfricher/magnesium/releases), download the newest `Magnesium-<version>-arm64.dmg` and open it.
2. Drag Magnesium to Applications, then open it from Applications.
3. macOS says it could not check the app for malware. Choose **Done**.
4. Open **System Settings → Privacy & Security**. Under Security, choose **Open Anyway** next to Magnesium, and confirm.

Step 4 is needed once for each new version. The app is signed ad hoc, which is free and needs no Apple account, but it is not signed by Apple. If macOS ever says the app is damaged, the copy did not come from the release page: download it again.

You need [Claude Code](https://code.claude.com/docs/en/setup), and a paid Claude plan or an Anthropic Console account. No terminal step is needed.

### What the app sets up

This Mac, a document the window opens by itself when something here needs you, shows each step, and moves on when they are done.

- **The `magnesium` program.** It is inside the app, built from the same commit. On first open the app puts it in `~/.local/bin`, where Claude Code's own installer puts `claude`. The window, Claude Code's hooks and a terminal then all run that one copy. If another version is already installed, the app asks before it replaces it, because a newer program upgrades each study folder's record when it opens the folder. If you keep the other version, lessons run on it, and the app says what that version cannot do. A program older than the window needs is refused, whatever its version number says: the app offers to replace it, or, run from source, names the install guide, and starts no lesson until it is fixed. "Keep" is not offered for it.
- **Claude Code.** The app looks for `claude` and asks it whether you are signed in (`claude auth status`). Signed out, a button starts Claude Code's own sign-in (`claude auth login`), which opens Anthropic's page in your browser. Without Claude Code, the app links to Anthropic's install page and offers Check again. The app never installs Claude Code, and never reads or stores a credential.
- **How lessons are paid for.** Choose your Claude plan, the default, or an API key, paid per use. This Mac, Settings and ⌘K show which one Claude Code would use. With the plan chosen, the app starts Claude Code without the `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` variables, because Claude Code started by another program uses such a key over the plan without asking. A key that Claude Code keeps itself, such as an API key helper in its settings, cannot be held back: the app then says the lesson is not paid with the plan.
- **Your study folders.** ⌘O lists every study folder the window knows, by name. **Open a folder** adds one, and **New study folder** makes one. **Find folders on this Mac** looks through your home folder for folders that Magnesium was set up in from a terminal, and adds them. It runs only when you press it, it reads folder names only, and macOS may ask whether the app may read Documents or Desktop. A folder Magnesium has never been set up in is set up when its first lesson starts. A folder an earlier version set up asks once, then is upgraded: Magnesium keeps a copy of its record in `.magnesium/backups` before it changes it. Say "Not now" and the folder stays as it was, and the window starts no lesson in it. ⌘⌫ on a folder in that list, or "Take this folder off the list" in ⌘K, takes it off and leaves the folder as it is.

Setting a folder up also writes two things outside it: Magnesium's plugin for Claude Code in `~/Library/Application Support/magnesium`, and one entry in Claude Code's settings that names it. The [install guide](../docs/install.md#5-set-up-a-study-folder) lists every file.

The window shows its version and the commit it was built from at the foot of the setup screen and of ⌘K. The program inside reports the same commit, as `magnesium --version` prints it: `magnesium 0.3.0+<commit>`.

## Run from source

From the `app` folder of the Magnesium repository:

```sh
./run ~/path/to/your/study-folder
```

The first run builds the app, which takes a few minutes. After that, `./run` alone opens the study folder you had open last, and `./run` with a folder opens that one. The built app is at `target/release/bundle/macos/Magnesium.app` and also opens with a double click.

You need Claude Code, `magnesium` on your PATH (see the [install guide](../docs/install.md)), Rust and Node. A build from source carries no `magnesium` of its own: it uses the one installed.

To click through every state with made-up data and no agent, run `npm run dev` and open http://127.0.0.1:1430 in a browser. The end of the address chooses the state. The evidence contract's: `#s1-cold` (a lesson stopped two days ago, reopened), `#s1-cold-first` (the first open on a new Mac), `#s2-start`, `#s2-answer`, `#s3-before`, `#s3-after` (the plan changed, beside the lesson), `#s4-offplan`, `#s4-return`, `#s5-week`, `#s5-week-next` and `#settings`. The other documents and states: `#open` (⌘P), `#commands` (⌘K), `#folders` (⌘O), `#sidebar` (⌘B), `#material` (a note beside the lesson), `#paper` and `#paper-running`, `#record`, `#earlier` (an earlier lesson), `#plan`, `#plan-edit` (⌘E), `#new-lesson`, `#hint`, `#drop`, `#upgrade`, and `#setup-signed-out`, `#setup-no-claude` and `#setup-replace` for this Mac's setup. `npm run shots` photographs them all into `shots/`, the contract's in both schemes.

## What is on screen

There are no screens. The window is the editor of the study folder you have open, and everything in it is a document of that folder, opened in a tab.

- **The lesson.** A page you and the tutor write together, under the lesson's goal. The tutor's words run down the page. A task is a block with a rule down its left side and a place for your answer inside it; the caret is already there when the tutor sets one, and Return hands the answer in. Anything you write outside a task, on the line at the end of the page, is talk: Esc takes you there, Tab takes you back into the task. While you talk, the open task stays where it was, and pins itself to the top of the pane once it scrolls away. Hint, I'm lost, I don't know, how sure you are (⌥1 ⌥2 ⌥3) and photos of your working (⌘⇧A) belong to the task, and show when you are in it. A new sitting starts under a line with the day's name. A lesson that is not running ends with where you stopped ("Stopped yesterday at 16:40.") and Continue, which Return on the empty line also does. A new lesson (⌘N, or a folder with none) asks what to work on, with the plan's next items under the line; Return alone starts the first.
- **The plan.** The plan file itself, `Study/<plan>/plan.md`, drawn as a document, with the template's reference sections (the instructor's words, research, the paper bank, the method…) folded until you open them. When the tutor changes it, it opens beside the lesson, the lines that changed carry a bar in the gutter, the old lines are struck through where they were, and the first change is brought into view. A row that only moved is marked where it now is. ⌘E edits it as text, and ⌘S saves it, only if it has not changed since you opened it. "Change it by telling the tutor" takes you to the lesson's line.
- **Your material, papers and handed-in work.** Any file under `Study/`, opened by ⌘P: a PDF drawn page by page by the reader inside the app (← → or j k), a Markdown file drawn with its mathematics, a text file, an image. ⌘Return on a note starts a lesson from it; on a paper it starts the paper, for the minutes the plan gives it, with the time left in the status line. A source the tutor cites opens beside the lesson at its page, after you have answered.
- **The record.** Every answer the record marked, by day, newest first, each with its mark and the help you had, then each skill and what the record holds of it, in words, and the papers' scores.
- **Settings.** A document (⌘,): the face your documents are set in (Charter, New York, Iowan Old Style, SF Pro, Atkinson Hyperlegible or SF Mono), its size, the line width, the scheme, the colour of your own writing, what lessons run on, how they are paid, a paper's default length, and every key. Kept on this Mac.
- **This Mac.** What is set up, in three lines, and what is left to do. On the first open it is the only document, with Open a study folder (⌘O) as the one thing to do.

The window's own chrome is a row of tab names at the top, which shares the line with the traffic lights, and one status line at the foot: the folder's name (a click, or ⌘O, lists your study folders), the time left on a running paper, what the tutor is doing while it works, and what lessons run on. ⌘B shows the folder's files down the left. ⌘\ splits the window into two panes.

### Talk and answers

Where you write decides what a message is. Written inside the open task, it is sent as an answer (`send(text, "answer")`), and the tutor judges it as it always has. Written anywhere else, it is sent as talk (`send(text, "talk")`), and the engine never marks it. The help words go as they always did, with no intent, because the engines read their exact words. The window learns that a reply sets a task from the reply: a paragraph that opens with a bold Task, Question, Check, Quick check or Paper redo, or else a last paragraph that asks a question, from its question on. A task stays open until the record marks what was sent for it, or the tutor sets another; after a reply that sets none, the caret goes to the page.

### A mark in the gutter

A mark comes only from the record. The core sends `marked` when the record has a new row for the last message, and the window sets it in the gutter beside your answer: a glyph and at most four words ("Right, from a blank page", "Right, with a hint", "Partly right", "Not right yet", "Declined"), in the quiet ink, never red or green. With no row, nothing is said, whatever the tutor wrote. What each message was, and the marks beside it, are kept on this Mac by the words of the exchange, so a lesson reads the same when it opens again.

### Courses, and what is no longer shown

A course is a study folder; ⌘O lists them by name. No page shows a level, proof dots, a count of what is due, a run of study days, a countdown to an exam or a congratulation. The tutor opens a sitting with what is due, as part of the lesson. The record lists, per skill, the days it was answered right alone and when it is asked again; the plan says the exam's date as the file has it; `magnesium progress` in a terminal still prints the level.

### How it looks, and why

- **The material is the identity.** Documents are set in the face you choose, Charter by default, at 17px with a line height of 1.6, in a centred column of 66 characters. The window's own words (tabs, the status line, the lists) are in the system's face, small and quiet, so they read as part of macOS rather than as an app's voice. Only faces every Mac has are used, and Atkinson Hyperlegible, which is inside the app; the window makes no request for a font.
- **Two schemes, one ink.** "Slate" (`#1D2023`, text `#DAD8D3`) and "paper" (`#FAFAF8`, text `#1F2226`), each with a quiet grey for what is not content and one hairline colour. Colour is spent in two places: your own writing, the caret and the open task's rule are in your ink (blue-black by default; green-black, violet or graphite, which is none), and a change to the plan is a bar in ochre. The one filled button is Continue, in your ink. The scheme follows macOS unless you choose, and `<html data-scheme>` overrides both.
- **The gutter is the record's.** A mark and a changed line appear only in the gutter, left of the column, and only from the record or the file.
- **Calm.** Nothing moves by itself except the caret the tutor writes with while it works, which stops where motion is reduced. Nothing is drawn as a card.
- **Mathematics everywhere.** A reply, a note, the plan, your own answer and the pinned task all draw `$…$`, `$$…$$`, `\(…\)`, `\[…\]` and bare environments. While the tutor is writing, a formula appears only when it is whole. Your answer shows its mathematics drawn under it as you type; ⌘⇧M inserts a symbol.
- **Keys first.** Everything in the five scenarios has a key, the settings list every key, and Tab reaches every control: in WebKit, which draws the window, Tab skips buttons unless the Mac says otherwise, so `src/main.tsx` gives each one a tab stop.

## What must stay true

- **A mark is the record's.** The lesson says a mark only for an answer whose row appears in `magnesium attempts` after it was sent. It never reads a mark from the tutor's words, and it never invents a number.
- **The window is a layer.** Every teaching rule stays in the Magnesium runtime and its plugin. A button sends plain learner words, such as "I'm lost" or "Can I have a hint?". A lesson on Claude Code started here continues in a terminal with `claude --continue`. A lesson on Magnesium runs in the window, through the same runtime and record (see [Two engines](#two-engines)). The one difference is how a reply reaches the screen: in the window the tutor sends it through Magnesium's `reply` tool, whose words stream as they are written, where text after a tool call would arrive in one piece.
- **Your own agent and subscription.** The window starts the `claude` program you installed, as published, in your study folder. Signing in is Claude Code's own (`claude auth login`), in your browser. The window reads no credential, and never passes `--bare`, which would skip Magnesium's hooks.
- **A lesson loads what it needs and nothing else.** The window starts Claude Code with Magnesium's plugin, from where `magnesium init` put it, and nine of Claude Code's own tools: Read, Write, Edit, Glob, Grep, AskUserQuestion, WebSearch, WebFetch, and Bash inside Claude Code's sandbox. Write and Edit never touch `.magnesium/`. Write and Edit on the study root's plan files (`Study/<plan>/plan.md` and `graph.yaml`) need no permission card, so "change the plan" asks you nothing; every other write still asks. Settings come from the study folder only, so your other plugins, hooks, skills and tool servers stay out, and so do your account's connectors, such as mail and calendar. The model is named (`claude-fable-5-2`, Claude Code's default on 2026-11-11), so a later change of that default cannot change the tutor unseen. It thinks at Claude Code's default effort, as in a terminal. A lesson in a terminal is unchanged.
- **Local files.** No cloud service, no account, no telemetry. Material you add is copied into the study folder and goes nowhere else. The window makes no network request of its own, and an image from the web inside a reply is not loaded. It looks for study folders only when you press **Find folders on this Mac**.
- **The runtime stays small.** The window links none of the runtime's code. It runs `magnesium digest` and `magnesium progress`, and reads your plan files.

## What was checked

| Check | How to run it | Result on 2026-11-11 |
|---|---|---|
| Everything below but the live check | `scripts/check.sh`, and `scripts/check.sh --window` for the self-check too | passes |
| Core: readers, state, files, the rows and the level from the runtime's record, the marked answers read from it, the run of study days and its rest day, the look for study folders | `cargo test -p mg-core --lib` | 124 pass |
| The library: every folder is listed, the nearest exam first, and a folder that was never set up is shown as new | `cargo test -p mg-core --test folders` | passes, against a stand-in runtime |
| The pipe: several turns in one process, permission allowed once, always and refused, a question answered, a reply interrupted, a lesson closed and resumed, a missing plugin reported, a helper's text hidden, a usage limit shown, a conversation the agent no longer has, a reply interrupted before its first word, a stopped or replaced conversation that says nothing more, and what a lesson loads: nine tools, the folder's settings, no account connector, the named model and no effort setting | `cargo test -p mg-core --test pipe` | 14 pass, against a stand-in agent |
| Paying: with the plan chosen, an API key in the environment does not reach the agent; with a key chosen, it does; a key the agent keeps itself is named before and during a lesson; signed out, the agent's own sign-in runs and the window sees the result; without the agent, its install page | `cargo test -p mg-core --test payment` | 3 pass, against the stand-in agent |
| The program inside the app: installed once without a question, another version kept or replaced only on a yes, asked again for a new version. A copy older than the window needs, by the number its digest prints, refused and never kept, with the install guide named when the app carries no copy | `cargo test -p mg-core --test install` | passes, against stand-in programs |
| A folder an earlier version set up: the window asks, starts no lesson, changes nothing, then upgrades with a copy of the record | `cargo test -p mg-core --test upgrade` | passes, against the runtime built from this checkout |
| A paper opens from a folder inside the study folder, and an answer key does not | `cargo test -p mg-core --test papers` | passes, against a stand-in runtime |
| Course material: copying, a name that cannot climb out, a link that cannot lead out, the 200 MB limit, a dropped folder, pasted text, an answer key counted and never named, what search cannot read said in one line each, and the list of what the folder holds (newest first, a nested folder, an answer key or a folder of keys counted and not named, no hidden file or link) | `cargo test -p mg-core --lib material` | passes |
| A cited source: a PDF and a text file open inside the study folder, and nothing outside it, through a link, or an answer key by its name, its folder or a link to it | `cargo test -p mg-core --lib files` | passes |
| Course material against the runtime built from this checkout: a PDF dropped on a course is found by the tutor's own search at its page, a scan and a name word and an unread kind are each said, a folder not set up is judged and left as it was, an older runtime is admitted to | `cargo test -p mg-core --test material` | 3 pass |
| Talk and the plan, against the stand-in model and the runtime: talk is never marked and keeps the open task; an off-plan question gets no task and "back to" poses the task again; the tutor's Edit of the plan file moves the exam a week and swaps two items, the digest reads the new date, and `plan_changed` names exam_date, items and text; a goal in the record block becomes the lesson's goal; `marked` carries the record's row; the plugin engine allows Edit and Write on the plan files only, and talk carries its lead | `cargo test -p mg-core --test direct` | passes, on 2026-11-12 |
| The five scenarios in the real window, through the harness, with the stand-in model | `node scripts/scenarios.mjs target/debug/magnesium-window <out>` | every step and photo, on base's screens, on 2026-11-12 |
| A runtime with no `attempts` command, and one with it: no marks and no error, then its rows | `cargo test -p mg-core --test marks` | passes, against stand-in programs |
| The readers against the runtime built from this checkout: its digest, `progress --json`, `attempts --json`, the marked answers as typed rows, the gap log it writes, the goal's steps, the proofs and the level on the progress page and on the library's card, a quick check from a chat lesson, the study days, and a gap log edited by hand | `cargo test -p mg-core --test runtime` | passes |
| The real window, end to end: page-to-shell calls, the setup calls and the choice of payment, the list of folders, the content policy, mathematics in a reply, in one line of text and in the learner's own words, a half-written formula held back, a task found where the tutor sets it, talk told from an answer by where it was written, a diagram, a permission card, a PDF page drawn by the bundled reader under the content policy, a source link shut as text and then open, pasted text saved, listed and read back, a path outside the study folder and an answer key refused, a write refused to anything but a plan file, the record opened, the lesson closed, kept, listed and resumed | `scripts/selftest.sh` | passes, against the stand-in, on the debug build |
| The five scenarios in the real window: a new lesson started, an answer marked in the gutter, the plan changed beside the lesson, talk off the plan unmarked, the task back, and the window reopened a week later where it was left | `node scripts/scenarios.mjs <window> <out>` | passes against the stand-in model on 2026-11-12: the answer sent as an answer and marked, the plan change and the off-plan question sent as talk and not marked |
| **The pipe against the real Claude Code**: a new lesson, a second turn, two interrupts, Stop pressed during a reply, and a resume | `scripts/live-check.sh` | Stop during a reply is new and not yet run. The rest passed on 2026-11-10, with Claude Code 2.1.322 and the runtime of 2026-10-23 |
| **A window lesson against the real Claude Code**: Magnesium's tools and nothing of the learner's, a reply of 100 words or more whose first words show before it is whole, the delivered reply recorded as shown, the goal's steps saved, Up next after the window closed mid-lesson, a paper built and marked, and the working line naming the work and no tool or command | `cargo test -p mg-core --test live a_window_lesson -- --ignored --nocapture` | passed on 2026-11-11, with Claude Code 2.1.322 and the runtime of this branch. A 200-word reply's first words showed at 16.1 s and the whole at 20.0 s. The paper was written in Markdown, so no PDF was built in the sandbox |
| The download: both programs report the commit, the signature verifies, no file holds a home folder path, and the disk image verifies | `scripts/package.sh <version>` | passes on a Mac. Only a copy downloaded with a browser shows what first open does |

`npm run shots` photographs every state with made-up data into `shots/`: the evidence contract's in both schemes, and the rest in the dark one. `cargo run -p mg-core --example look -- <folder>` prints what the library, the course page, the lesson's steps and the progress page would show for a folder.

The live check uses your subscription for six short turns in a scratch folder. It keeps every line between the window and the real program in a trace file, whose path it prints. Each line starts with its direction and the milliseconds since the conversation started. The trace of 2026-11-10 showed:

- In a folder Claude Code has never opened, the session-start hook runs and the plugin starts to install. Then the program answers `initialize`, finishes the install, and sends `system/init`. So the window judges the plugin from `system/init`, never from the `initialize` reply.
- An interrupt before the tutor's first word ends the turn with a `[Request interrupted by user]` line and a `result` of `error_during_execution` whose only error is the program's own diagnostic. The window shows it as interrupted, not as an error.
- An interrupt during a reply ends the turn within about 30 ms. The program sends a reply's text in bursts, so text that had already arrived stays on screen.
- `resetsAt` on a usage-limit line is in seconds.

## Which runtime it runs

The download installs its own copy, as above. Run from source, the window runs whichever `magnesium` is on the path.

A runtime from this repository after 2026-11-10 never upgrades a folder just because `magnesium digest` looked at it. For a folder an earlier version set up, the digest says `upgrade_needed` and holds no record, and the window asks before it runs `magnesium init --upgrade-instructions`.

- **The runtime of 2026-10-23.** It keeps no quick checks, has no `attempts` command and stores no goal steps. The lesson shows no mark beside an answer, and the record lists no marked answers. The tutor writes the gap log, and the window reads it as written.
- **The runtime on `main` from 2026-11-10** (Magnesium issues 29, 30 and 31). It has no `progress` command, so the window reads the Gaps table that the runtime writes into each gap log. It shows no level, because only gap-log rows could earn proofs there and a level over them would have a low ceiling. Quick checks are kept where the window cannot read them, so they are not shown. The run falls back on the days an answer was sent from the window. If the file differs from the record, as after a hand edit, the record document lists no skills until the runtime writes the table again.
- **The runtime of this pull request** (Magnesium issue 46). `magnesium progress --json` prints each skill's proofs, the level, what is due and the days with a marked answer. The window takes all four from it, so a hand edit to a gap log changes nothing on screen. For a plan whose gap log the runtime has not read into the record yet, the command says so, and the window reads that plan's file and shows no level until the tutor next marks an answer.

The window refuses every runtime above, and any other that does not meet its needs. Each digest of a runtime from this branch on prints `window_features`, one number for what the window may rely on, and the core holds the lowest number it needs (`WINDOW_FEATURES_NEEDED` in `runtime.rs`). It needs 2: the runtime that lists a sitting's due paper redos as data, which a lesson on Magnesium poses. A runtime that prints a lower number, or none, is too old, whatever its version says: a build from an older commit can report the same version. The readers for the older shapes above stay in the window.

Upgrading the runtime upgrades the learner record in each study folder it opens, once, after keeping a copy.

## Talk, the plan, and what the page hears

**Talk is never marked.** The page sends each message with an intent: `send(text, "talk")` for conversation (a question, a change to the plan, something off the plan), `send(text, "answer")` for an answer to the open task, or no intent, which leaves it to the tutor as before. On Magnesium code holds to it: a talk message's record block has its `answer` set to null and the task it poses ignored, so nothing can be saved as an attempt and the open task stays open; the tutor is told in the lesson state that the message is talk. On Claude Code the window cannot stop the plugin's tutor writing an attempt, so talk goes with one plain line before it, "(This is not an answer to the task, so please don't mark it.)", which the teaching files respect: they record an attempt only for a first try at a task the tutor set. A runtime change that would make it a rule is written up for the runtime.

**The tutor changes the plan.** The exam's date and the order of what to study live only in the plan file, `Study/<plan>/plan.md`: the first date in the Facts table's When row, and the rows of the Blocks table. Both engines' tutors edit that file when the learner asks, without a permission card, log one line under Changes, and say in a line what changed. The direct engine's lesson state names each plan with its exam and date, and its record block can carry a new `goal`, which goes to the runtime as `learning_step`'s `session.use` goal. The record format and what counts as a proof do not change.

**What the page hears.** `plan()` reads the open folder's plan fresh: the nearest dated plan (the one the window shows as the plan), its title, date, path and Markdown, its Blocks rows as `items` (each with an id that is a slug of its title, so it survives a reorder, and a Status and Day column when the plan has them), and the goal and steps of the record's last lesson. After each turn the manager waits for the turn's record to be kept, reads the plan again and sends `plan_changed` with what differs (`exam_date`, `items`, `goal`, `steps`, `text`), then reads the record's marks and sends `marked` with each row that was not there before the message. No screen needs to poll `marks()` or re-read the plan after a turn.

## The evidence harness

A dev-only hook lets the real window be driven, photographed and timed. It is inert unless the window starts with `MAGNESIUM_APP_DRIVE=<dir>` or `MAGNESIUM_APP_PERF=<file>`, and it reads and writes nothing outside those two paths.

- **Startup and typing.** Each screen set calls `ready()` from `src/harness.ts` once the first screen has its real content. With perf on, the window writes to the perf file the time from the top of `main` to the paint after `ready()`, then, once a `data-drive="composer"` field is enabled, the median and 95th percentile of 60 keystrokes typed into it, each timed to the next paint; the field is put back after.
- **Driving.** With drive on, the page reads instructions from `<dir>/in/<n>.json` and answers each in `<dir>/out/<n>.json`. `node scripts/drive.mjs <dir> '<op json>'` sends one and prints the answer. The ops are listed at the top of `src/harness.ts`: type, key, click and focus (by a `data-drive` name, else visible text), wait_idle, wait_reply, wait, text, perf, scheme (sets `<html data-scheme>`, which every screen honours), plan, transcript, ready and quit. The primary action on the first screen carries `data-drive="start"` and the main text field `data-drive="composer"`.
- **Photographs.** `scripts/capture.sh <pid> <out.png>` captures that process's window with `screencapture -l`, finding it with a small Swift helper compiled once into `target/winid`.
- **A scratch course.** `scripts/scratch-course.sh <dir>` makes an introductory statistics course with three lecture notes and a plan for "Midterm 2" ten days ahead, two more folders for the library, a runtime wrapper with a scratch `HOME` and a scratch window home. `--days-ago <N>` moves every time it recorded back N local days.
- **The scenarios.** `node scripts/scenarios.mjs <app-binary> <out-dir>` plays the five scenarios of the evidence contract in the real window, with the stand-in model on Magnesium, and photographs each step in both schemes; it writes `perf.json`, `attempts.json`, `plan-before.json`, `plan-after.json` and `transcript.md` beside the photographs. `--live` uses the real `claude` instead, on your subscription.

## How it talks to the agent

`crates/core/src/agent.rs` is the one interface. Nothing outside `claude.rs` knows which agent is running.

For Claude Code the window runs:

```
claude -p --input-format stream-json --output-format stream-json --verbose \
  --include-partial-messages --permission-prompt-tool stdio [--resume=<id>]
```

It keeps that one process open for the sitting and writes each of your messages to it as one line of JSON. Permission requests and the tutor's questions arrive as control requests on the same pipe. This is the wire format the Agent SDKs use. Closing the window ends the process, and the conversation stays on disk under its id.

**Another agent** would need a second file beside `claude.rs` that implements the same two traits: start a conversation in a folder, send words, stream the reply, surface approvals, interrupt, resume by id. None is written. Magnesium itself is built and tested for Claude Code only.

## Two engines

A lesson runs on one of two engines. Both reach the model through the learner's own Claude Code, so sign-in, account and payment are Claude Code's in both.

- **Plugin** (`Engine::Plugin`, the default; the learner sees it as "Claude Code"). Claude Code runs the lesson through Magnesium's plugin, as described above: it loads the teaching files, keeps the record with `learning_step`, and replies through the `reply` tool.
- **Direct** (`Engine::Direct`; the learner sees it as "Magnesium"). The window runs the lesson (`crates/core/src/direct.rs`). Claude Code is started as the model alone: Magnesium's tutor prompt in place of Claude Code's (`--system-prompt`), no settings files (`--setting-sources ""`), so no plugin, hooks, skills or tool servers of the learner's or the folder's; no CLAUDE.md, no auto memory, no account connectors; the same named model and effort as the plugin engine. Its tools look things up and change the plan, nothing else: Read, allowed only inside the study root (`Read(//<root>/**)`, with `--add-dir` for a root above the open folder; a bare `Read` would approve any path on the disk, and a live check showed it did); Edit and Write, allowed only on the plan files (`Edit(//<root>/Study/*/plan.md)` and the same for `graph.yaml`, with `.magnesium/**` denied); and Magnesium's `search_sources` and `sources`. The record tools are disallowed, which takes their definitions out of the model's context: a first turn reads about 4,000 tokens, against about 20,000 with the five tools.

Each direct turn, the window captures the learner's words with the runtime's prompt hook, starts or resumes the runtime's lesson when none is active in the conversation, and sends the words with the lesson state: the goal, its steps, the open task, the quick checks and paper redos due (the runtime's `sitting_checks` and `sitting_redos`), earlier answers on the goal, what the learner's memories say and the course graphs, all of it at the start of a conversation, then only what changed. The reply streams as it is written. It ends with a record block of the tutor's judgment (the mark, the help it gave, the skill, a note, the task it poses, the goal's steps, a misunderstanding found or cleared, and after a due quick check or paper redo its next question), which is never shown: `crates/core/src/record.rs` cuts it from the stream, checks it and turns it into one `learning_step`, adding what code knows better than the model, such as the task that was open, the help given on it across turns, the Hint button, "I don't know", and the gap-log rows a paper redo counts for. The turn ends for the learner as soon as the reply is whole; the record is kept just after it, within a 20-second budget, and a message sent meanwhile waits for it. The stop hook then records the reply as shown. A tool call that hangs for 8 seconds replaces the runtime's tool server and is sent again exactly as it was, which the runtime answers with its first result. A missing or unreadable block is asked for once more, in a request the learner never sees; a message sent meanwhile waits for it. If the second answer is no better, the window says that one answer was not marked, the trace log (`MAGNESIUM_APP_TRACE`) gets a line, and the lesson goes on. An interrupted reply gets no mark, and its task stays open.

**Where the learner picks.** "Lessons run on" is a choice of two, "Claude Code" and "Magnesium", in Settings, and a command in ⌘K. One click picks and saves; there is no confirm step. It is shut while a lesson is open. The status line names the engine of the open conversation. The words "plugin" and "direct" never reach the screen. Whether Anthropic's terms allow "Magnesium", which starts the user's own copy of Claude Code as the model only, is an open question (Magnesium issue 50, beside issue 38 below). "Claude Code" stays the default.

**A conversation keeps its engine.** The window stores the engine the learner picked for new lessons, and the engine that started each folder's conversation beside its id. A conversation resumes only on its own engine. When the learner picks the other engine, the next lesson is a new conversation, even from Continue, and the record carries the progress across. Settings says so under the choice until that lesson starts: the direct engine resumes the runtime's lesson in it, with its goal, steps and open task.

**Practice papers stay on the plugin engine.** Handing in a paper from the paper screen always opens a plugin conversation to mark it, whatever the learner picked; the lesson after that is on the learner's pick again. Sitting a paper uses no tutor. In a direct lesson the tutor says, in one line, that papers are built and marked with the Claude Code tutor, and goes on.

**Paper redos run on both.** A paper redo asks again, cold, for a question the learner lost marks on in a practice paper. The runtime keeps each one as a row of the plan's gap log and works out when it is due. A direct sitting opens with the redos that are due, after its quick checks. The runtime lists the due rows as data (`sitting_redos`), and the engine makes one redo of the rows that share a tag in one plan. The runtime takes one cold answer on a label in a day, so a tag has one redo in a sitting: rows of that tag in another plan, more than twelve rows, and a tag already answered that day wait for a later day. A sitting lists at most six redos; the rest stay due. The tutor sees each redo's tag, the paper questions it came from, what went wrong, and the stored question and its key when there is one, never the rows' names.

- **Posing.** The tutor poses a redo with the words "Paper redo N:" where it sets the task. The colon is what the engine reads: a redo only spoken of binds nothing, and when a reply names several, the last names the task. A redo is posed once in a sitting. Posed again in other words before its answer, it keeps its rows and the help given on it.
- **The answer.** The engine sends it under the rows' tag with the rows in `counts_for`, and the runtime works out each row's pass or fail. It is sent cold only by the rule for every answer (see Cold answers below): a redo comes after the quick checks and whatever they taught. A decline changes nothing, the stored question included.
- **Timed redos.** A row lost for slowness is timed. The tutor states the time allowed, and the engine tells it how long the answer took, counted from the reply that posed the redo, on the wall clock, whatever was said in between. A right answer over the time is sent as `slow`. A timed redo read back after a resume has no clock, so its answer is not sent cold: it earns no pass and stays due.
- **What the record made of it.** The engine reads the runtime's verdict on each redo answer, and on each answer to a due quick check. The tutor is told on the next turn, and is asked never to say that a redo or a check passed. When the tutor marked an answer right and the record holds no pass, the window says so in a notice.
- **When the engine cannot know what is on screen.** A reply stopped part-way, or one whose record block cannot be read, may have marked the open task or posed another, whatever kind it was. The engine then forgets the open task, takes it out of the record's checkpoint, so that a later conversation cannot read it back, and does not pose again the redos that reply showed. The tutor is told with the next message that no task is open. The next answer is saved against no question: it has no mark, and the learner is told. A quick check that was open goes back on the list, since the record still holds it due, and the tutor may pose it again; the learner answered it in the sitting already, so that answer is not cold. A stop before the first word showed nothing new and keeps the task. A task the reply calls a paper redo when the engine has none for it is saved as an ordinary answer, with a notice.
- **When the runtime refuses.** A tag it will not take as a label, as one from a gap log written by hand can be, gives way to the skill the tutor named, and the answer still counts for the rows. Rows it no longer knows are left out, and the answer is kept as an ordinary one.
- **The next question.** The tutor's block gives the redo's next question, which the engine stores on each row, so the next redo is a new problem.

"Continue without the quick checks" leaves the redos out too, for that sitting.

**Repairs run on both.** A redo that fails leaves its row waiting for a repair: the runtime sets its passes back to none and gives it no next redo until a repair is recorded. A lesson on Magnesium records one.

- **What waits.** At the start of a sitting the engine asks the runtime for the rows waiting for a repair (`repairs_waiting`, in a `context status` result for a caller that asks, `window_features` 3), and after a redo fails it reads the failed rows from the save's result. The rows of one tag in one plan make one repair, which the tutor sees as "Repair N" with its tag, the paper questions and what went wrong, never the rows' names. A number is never reused in a conversation.
- **The tutor repairs** a redo that failed in the lesson at once, unless the learner declines, and names the repairs that already waited in one line after the sitting's checks and redos, taking the first unless the learner wants their goal first. The repair's checks are ordinary tasks.
- **When it is done**, the record block says so: `"repair": {"number": N, "done": "what the repair was", "next": {"question", "key"}}`. The engine asks the runtime, just then, which rows wait, and sends a practice change (`repaired`, and `next` when given) for each row of that repair that does; never for another row, and never twice. A repair on a row whose redo is already set would move that redo, so a row repaired meanwhile in another conversation gets none. The engine reads back each row's next redo, and the tutor is told on the next turn; it is asked to say nothing of when the question comes back before that.
- **When it cannot be recorded**: a reply with no readable record block while a repair waits, a stopped reply whose block had begun to name one, a repair with no number or no line on what was done, a number no repair is listed by, a repair none of whose rows still waits, a block that also asks for a pause, stop or recording change, a turn whose record was not kept, or a refused save. Nothing is recorded, the row keeps waiting, and the learner and the tutor are told. A save whose outcome stays unknown is asked again with the same request first; if the record still cannot say, the learner is told the repair may not be saved. Recording off, pause and stop hold as for everything else.
- **Rows in no plan**, from a paper marked where no plan exists, are found and repaired by their id, as `counts_for` names them. `repairs_waiting` lists the most recently changed rows first, up to 96, more than a sitting can fail.

**The tutor's prompt** is `crates/core/prompts/tutor.md`, built into the window. The direct engine teaches from this text, not from the teaching files in `src/workspace/learning/`. It is much shorter than those files and leaves some of their rules out, such as anchors. It does hold the reason for each move, a new explanation when confusion repeats, transfer, a route built from the course graph, self-explanation at a hinge, an answer to each guess, telling a slip from a misunderstanding, and aiming higher after right answers. The first lesson state of a conversation names each course graph in the study root (`Study/<plan>/graph.yaml`, as in the teaching files), and the tutor reads the one whose tags cover the goal when it sets or changes the route. Until one engine is retired, a change to a teaching rule must be made in both places. To try other wording without a rebuild, name a file in `MAGNESIUM_TUTOR_PROMPT`. Claude Code records a conversation's prompt once, when the conversation starts, and keeps it on resume, so a changed file reaches new conversations only.

The page reaches the choice through two commands, `engine` and `set_engine`, and `open_session` says which engine it opened:

```
engine()                 -> {"chosen": "direct", "conversation": "plugin"}
set_engine({engine})     -> the same shape; "plugin" or "direct"
open_session({resume})   -> {"resumed": false, "turns": [...], "engine": "direct"}
```

`conversation` is the engine of the open folder's conversation, running or remembered, or null when it has none.

**What the direct engine does not do yet:**

- It writes no anchors into the learner's memory; a misunderstanding found or cleared is its only memory edit. That edit keeps one line of its own in the memory's description, rewritten in place, and never changes another line, such as an anchor the plugin engine's tutor wrote. When the description is at the runtime's limit (16 KiB), it makes no edit. The tutor names the memory it revises by the short id the lesson state lists; it sees the memories a `prepare` for the goal returns, at most eight.
- It stores a next question and answer key (`next` in an attempt) only with the answer to a due quick check or paper redo. A miss in a lesson makes a quick check with none, where the plugin engine's tutor stores one; the check's opener then asks the tutor to make a new question.
- It builds and marks no papers.
- It repairs only rows the runtime lists as waiting for a repair, and marks no handed-in paper, so it logs no new gap-log row.
- It poses a paper redo only at the start of a sitting. Asked for one later, or after the learner chose to go on without them, the tutor says that those due open the next sitting. One question never counts for rows of different tags, as the Claude Code tutor can make it with the course graph.
- A redo set aside, by a stopped reply or an unreadable record block, is not posed again in that sitting. It stays due and opens a later one.
- It makes no files but the plan's: no figures or pages. It edits `Study/<plan>/plan.md` and `graph.yaml` when the learner asks for a change, as the plugin engine's tutor does. It asks no permission and no multiple-choice question.
- A paused or stopped lesson goes on only when the learner opens it again from the window (Continue or a new lesson), not from words typed in the same conversation.

**Pause, stop and recording off.** The record block can carry a control the learner asked for in their own words: `pause`, `stop`, `recording_off` or `recording_on`. The engine makes the same `learning_step` control call the plugin engine's tutor makes. After a pause or stop it sends nothing more to the record in that conversation, and the tutor is told to set no task unless the learner asks to go on; opening the lesson again from the window resumes it. After recording off it sends nothing at all, hooks included, in that conversation or any later one in the study folder (the window keeps the choice by study root), and an answer with no mark raises no notice, until the learner asks for recording on. A write the runtime refuses because recording is off or the lesson is inactive stops the engine the same way.

**Cold answers.** One rule for every answer, a due quick check's and a paper redo's included. An answer is sent cold only when the tutor's block says it came from a blank page, no help was given, it is not a decline, and the engine knows of no earlier answer, help or correction on its label in this sitting, nor of an answer on it earlier the same day. The engine notes, for the sitting, the label and skill of each answer, of each task given help, of each misunderstanding found, and of a task set aside after a stopped or unreadable reply. A due quick check answered right and not cold earns no pass and stays due, and the learner is told. If the runtime still refuses a cold answer, it is sent again as not cold, so the mark is kept.

**The buttons.** On an ordinary task, "Hint" and "I'm lost" are help on the task in hand: code records at least a hint on it, whatever the tutor's block says, as the teaching files answer a lost learner with a question that finds the cause and then teach from the answer. On a due quick check or paper redo the press alone records nothing: the tutor first warns, in one line with `help` none, that help means no pass, and help counts only when a later reply's block gives it. Once a reply has given help, or the sitting taught or corrected the skill, the answer earns no pass. "I don't know" is a decline.

**A new sitting** starts from what is due now: the quick checks and redos the last sitting listed and did not pose are dropped, and so is what it taught, and no task is open, even one read back from the record; a check posed and not answered is still due and listed again. The engine tells a sitting by the wall clock, as the runtime does, so a window left open while the Mac sleeps sees the new sitting.

**A window started again** knows the sitting it left: the engine keeps, in the window's store by folder, when the learner last wrote and what the sitting taught, and, for the same conversation, the repairs, checks and redos it listed by number. A repair number is never given twice in a conversation, so a repair the tutor finishes after a restart binds the rows it was posed for or none. A resumed conversation with nothing kept sends no answer cold until a new sitting begins.

**Other tasks.** A quick check that another task replaces before its answer goes back on the list and may be posed again. When recording comes back on, no task is open, in the checkpoint too: replies while it was off may have set tasks the engine never read.

**A save whose outcome is unknown.** When the runtime does not answer a save, even replayed, the engine asks it once more with the same request, which the runtime answers with its first result if it kept it and keeps once if it did not. Only an answer it still cannot confirm gets a notice, which says the answer may not be marked.

`crates/core/tests/direct.rs` drives it with a stand-in model and the real runtime. `crates/core/tests/live_direct.rs` runs it against the real Claude Code (ignored by default; it starts a model on your subscription).

## Where things are

```
crates/core/        everything that is not drawing, with its tests
  src/agent.rs      the interface to a coding agent
  src/claude.rs     Claude Code behind that interface
  src/runtime.rs    runs the magnesium program and reads what it prints,
                    including the marked answers (`marks`)
  src/install.rs    puts the app's own magnesium on this Mac, asking first
                    before it replaces another copy
  src/material.rs   copies added course material into the study folder, lists
                    what it holds, and words what search can and cannot read
                    of it
  src/files.rs      handing in work, opening a paper, and reading a cited
                    source inside the study folder
  src/study.rs      what the library, the course page and the progress page
                    show: the cards, the proofs and the level
  src/activity.rs   the days studied in the window, and the run they make
  src/find.rs       looks for study folders set up in a terminal
  src/gaplog.rs     reads plan.md, gap-log.md and graph.yaml
  src/peek.rs       two facts read straight from the runtime's files (see below)
  src/manager.rs    one open folder and its one conversation, and what each
                    turn changed (plan_changed, marked)
  src/plan.rs       the open folder's study plan, and what differs between
                    two readings of it
  src/workspace.rs  the folder's lessons and files under Study/, and the one
                    write the window makes: your own edit of a plan file
  tests/            the pipe tests, the stand-in agent, the readers against
                    the runtime, paying and signing in, installing, upgrading
                    a folder, the live check
src-tauri/          the window's shell: passes messages, decides nothing
src/                the pages (React, plain CSS)
  App.tsx           the workspace: which documents are open in which pane,
                    the keys, ⌘P ⌘K ⌘O, and the lesson's sends
  workspace.ts      the layout kept per folder on this Mac: tabs, panes,
                    scroll, pages, drafts, folds
  prefs.ts          your face, size, width, scheme and ink
  docs/             one file for each kind of document: Lesson (and Writer,
                    the places to write), Plan, FileDoc, Record, Settings,
                    Setup (this Mac)
  lesson.ts         what the lesson holds, and how each event changes it
  page.ts           the lesson as a page: which task is open, what each
                    message was, where the days change
  task.ts           where a reply sets its task
  diff.ts           which lines of the plan changed, and which only moved
  ui/               Rich (text with mathematics and diagrams), Sheet (the
                    one list over a document), pdf (the bundled reader)
  sources.ts        which links in a reply are sources, and their page
  harness.ts        the evidence harness's page side: ready(), timing, and
                    the drive ops
  demo.ts           made-up data for every state, by address
  demoBase.ts       the demo's plan, talk and the two events, kept apart
                    from demo.ts
  render.ts         Markdown and mathematics to safe HTML
  styles/app.css    every colour, the type and the layout
scripts/            drive.mjs, capture.sh, scratch-course.sh and
                    scenarios.mjs for the evidence harness (see above);
                    check, selftest, live check, screenshots, package (the
                    download, which .github/workflows/mac-download.yml runs
                    on a GitHub-hosted Mac); make_icon.py draws
                    the app icon (three proof dots on a dark tile) as a 1024-pixel
                    PNG, written to the path given as its argument
```

The window's own state is in `~/Library/Application Support/dev.magnesium.window/`, apart from the runtime's folder: the folders in your library, each folder's conversation id and stop line, a copy of the turns it showed, how many messages you sent on each day, how you chose to pay for lessons, and a `magnesium` version you chose to keep. None of it is learning state, and none of it is a credential. What the workspace looks like (the open documents, scroll, drafts, folded sections, your settings, and what each message of a lesson was) is kept in the page's own storage on this Mac.

## What is not there yet

- **A check shown before the lesson.** The tutor asks the checks that are due at the start of a sitting. The window could show the first one in full, with its key locked until you answer. The runtime on `main` stores the next question and its key for each due item (Magnesium issue 30), but no command prints them yet.
- **One fact comes from the runtime's files, not from a command**: the steps of the current goal. `peek.rs` reads them read-only and gives nothing when the layout changes. It should become a runtime command. `peek.rs` also still reads the quick checks of the runtime of 2026-10-23, which has no `progress` command.
- **The goal's steps are not drawn.** `plan()` and `rail()` read them, but the lesson page shows the goal as its title and leaves the steps to the tutor's words.
- **A mark needs the tutor to record one.** The gutter shows a mark only when the record has a row for the answer. If the tutor did not mark it, the gutter stays empty, and the task stays open.
- **A skill never got wrong has no redo date.** The tutor schedules redos only for what you got wrong. A skill you never got wrong earns its next proof when a later paper or lesson asks it again, so the record says no day it is asked again.
- **A sound skill can read as answered on one day.** The tutor reads a skill as sound after one right, cold answer. The record still counts days, so the record document says "on 1 day" until it is answered right on two more.
- **A task is found in the reply's words.** No event says that a reply set a task, so the window reads it: a paragraph that opens with a task word, else a last paragraph that ends with a question. A task written any other way is shown as plain text, and an answer to it can only be sent as talk; a closing question that is not a task gets an answer place it does not need. A read-only command or event for the open task would settle it.
- **A photo is sent as a path.** The window saves a shrunk JPEG in `Study/handed-in/` and tells the tutor where it is. The tutor reads the file.
- **A source is read, not searched.** A PDF shows one page at a time. It has no search inside the page, no selection of its text, and no zoom. A PDF with a script, a form or a font it needs from outside the file is drawn as pdf.js draws it, and a page that cannot be drawn says so.
- **Files cannot be pasted.** A file copied in Finder and pasted into the window gives the page no path. Drop it, or choose it.
- **Adding does not tell the tutor.** The tutor finds a new file the next time it searches. Say so in the lesson if it should look now.
- **No Apple signing or notarization, no update from inside the app.** Apple's signing needs a paid Apple membership. Until then each new version needs the one-time approval again.
- **Claude Code is checked, not installed.** Anthropic's terms set conditions for a product that installs or runs Claude Code. Whether they apply to an app that starts the user's own copy is an open question (Magnesium issue 38).
