# Magnesium's window

One window, made for studying. It sits over [Magnesium](https://github.com/samfricher/magnesium) and the coding agent you already use. It adds no teaching rule of its own: a lesson behaves the same here and in a terminal.

The window opens on one line: the next thing to do, already chosen, and a caret. Return starts it, and the lesson fills the same surface. Everything else is a layer a key brings over it.

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

- **The `magnesium` program.** It is inside the app, built from the same commit. On first open the app puts it in `~/.local/bin`, where Claude Code's own installer puts `claude`. The window, Claude Code's hooks and a terminal then all run that one copy. If another version is already installed, the app asks before it replaces it, because a newer program upgrades each study folder's record when it opens the folder. If you keep the other version, lessons run on it, and the app says what that version cannot do. A program older than the window needs is refused, whatever its version number says: the app offers to replace it, or, run from source, names the install guide, and starts no lesson until it is fixed. "Keep" is not offered for it.
- **Claude Code.** The app looks for `claude` and asks it whether you are signed in (`claude auth status`). Signed out, a button starts Claude Code's own sign-in (`claude auth login`), which opens Anthropic's page in your browser. Without Claude Code, the app links to Anthropic's install page and offers Check again. The app never installs Claude Code, and never reads or stores a credential.
- **How lessons are paid for.** Choose your Claude plan, the default, or an API key, paid per use. The first screen and ⌘K show which one Claude Code would use. With the plan chosen, the app starts Claude Code without the `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` variables, because Claude Code started by another program uses such a key over the plan without asking. A key that Claude Code keeps itself, such as an API key helper in its settings, cannot be held back: the app then says the lesson is not paid with the plan.
- **Your study folders.** ⌘O lists every study folder the window knows. **Open a folder** adds one, and **Make a new folder** makes one. **Find folders on this Mac** looks through your home folder for folders that Magnesium was set up in from a terminal, and adds them. It runs only when you press it, it reads folder names only, and macOS may ask whether the app may read Documents or Desktop. A folder Magnesium has never been set up in is set up when its first lesson starts. A folder an earlier version set up asks once, then is upgraded: Magnesium keeps a copy of its record in `.magnesium/backups` before it changes it. Say "Not now" and the folder stays as it was, and the window starts no lesson in it. ⌘⌫ on a course in ⌘O takes it off the list and leaves the folder as it is.

Setting a folder up also writes two things outside it: Magnesium's plugin for Claude Code in `~/Library/Application Support/magnesium`, and one entry in Claude Code's settings that names it. The [install guide](../docs/install.md#5-set-up-a-study-folder) lists every file.

The window shows its version and the commit it was built from at the foot of the setup screen and of ⌘K. The program inside reports the same commit, as `magnesium --version` prints it: `magnesium 0.3.0+<commit>`.

## Run from source

From the `app` folder of the Magnesium repository:

```sh
./run ~/path/to/your/study-folder
```

The first run builds the app, which takes a few minutes. After that, `./run` alone opens the course you had open last, and `./run` with a folder opens that course. The built app is at `target/release/bundle/macos/Magnesium.app` and also opens with a double click.

You need Claude Code, `magnesium` on your PATH (see the [install guide](../docs/install.md)), Rust and Node. A build from source carries no `magnesium` of its own: it uses the one installed.

To click through the surface with made-up data and no agent, run `npm run dev` and open http://127.0.0.1:1430 in a browser. The end of the address chooses the state. The evidence contract's scenarios: `#s1-cold`, `#s1-cold-first` (the very first open), `#s2-start`, `#s2-answer`, `#s3-before`, `#s3-after`, `#s4-offplan`, `#s4-return`, `#s5-week`, `#s5-week-next`, `#narrow-lesson` and `#settings`. The layers: `#plan` (after the plan changed), `#commands`, `#courses`, `#material`, `#work` and `#help`, and `#source` a cited page open over the lesson. `#reopen` is the window opened again mid-lesson, with a half-written answer; `#working` the tutor at work, `#permission` a permission card, `#new-course` a course with nothing in it, `#paper` a paper running, `#upgrade` a folder an earlier version set up, and `#setup-signed-out`, `#setup-no-claude`, `#setup-replace` and `#setup-too-old` this Mac's setup steps. `scripts/shots.mjs` photographs them all, in both schemes.

## What is on screen

There are no screens to move between. The window is one surface, and everything else is a layer that a key brings over it and Esc takes away. The concept and its reasons are in [CONCEPT.md](CONCEPT.md).

| Where | What it holds | What it leaves out |
|---|---|---|
| **The surface at rest** | The course's name, small; the one next thing, already chosen, large: the goal of the lesson you stopped, else the plan's next item, else the course's name; one quiet line under it (where you stopped, or the item's note); the line you write on, after a `›`; and one line of keys ("↩ carry on   ⌘K everything else"). Return starts it. Words typed first start it with those words, which the tutor takes as the goal. On the first open, or while this Mac needs a step (the program, Claude Code, its sign-in, a folder that needs an upgrade, a paper running), the same place holds that step and its few choices, as Spotlight holds results: ↑ ↓ and Return. | Every other course, the plan, counts, dates, levels, proofs, days studied, and how long you were away. |
| **The lesson** | The same surface: the next thing rises to the top as the lesson's heading, and the conversation fills it below. The tutor's words are in the reading face, yours in the face you type in. A task the tutor sets has a rule hung in the margin, and the line for its answer opens inside it, under the question, with the caret there. Below it, always, the conversation's own line, after a `›`. An answer stays in its task, with the record's mark under it. A change to the plan is a short block under the reply that made it. While the tutor writes, a block caret stands where its words will go. | Buttons, a top strip, a clock, the engine's name, and any word that you are behind. |
| **A layer** | A line to type in, a list under it, and its keys at the foot: ⌘K everything, ⌘O courses, ⌘P the plan, ⌘E material and papers, ⌘Y your work, ⌘, settings, ⌘/ help with the task. The surface stays under it, dimmed. A source the tutor cites opens as a larger layer at its page. | Anything that is a place to go to: Esc always brings back the surface as it was. |

### The two lines

Where you write says what it is, so no message is ever labelled.

- **The task's line** (`data-drive="answer"`). It opens inside the open task, under its question, when the tutor sets one, and the caret goes into it. Return hands it in: the page sends it with `send(text, "answer")`. ⌥1, ⌥2 and ⌥3 say how sure you are, ⌘U adds photos of your work, ⌘/ asks for a hint, says you are lost, or says you don't know (the window's plain words, sent as before, with no intent).
- **The conversation's line** (`data-drive="talk"`). It is always at the foot. What is said on it goes with `send(text, "talk")`, which is never marked. A question off the plan, "the midterm moved, change the plan", "back to the task": all of it is said here.
- **Moving between them.** ↓ at the end of the task's line, or Esc, goes down to the conversation's; ↑ at the start of the conversation's goes back up; a click goes to either. While you talk, the open task stays at the foot, its question said again above its line, just over the line you are talking on: it is never scrolled away and never shown as owed. Each line keeps its own words.
- **Where the caret goes.** Into a task the tutor just set, and back into it after help with it. After anything else, onto the conversation's line. A retry after a wrong answer is not a first try, and only a first try is marked, so nothing is lost when it goes as talk.
- **Knowing the task.** On Magnesium the engine says, after each turn, which task is open, in the tutor's own words from its record block, and whether this reply set it (`open_task`). On Claude Code, which says nothing of the kind, a task is a paragraph that opens with a bold word such as "Task." or "Check".

### Course material, and sources

- **Adding.** Drop files or a folder on the window, or ⌘E and "Add files…", or "Paste text…". When the task's line has the caret, a drop is photos of your work for the answer instead. Each file is copied into `Study/materials/` under its own name and never over another; the window then says, in one line each, what it added and what the tutor's search cannot read (`magnesium searchable`).
- **What is there.** ⌘E lists the newest files in `Study/materials/`, a count of the rest, the papers to sit, and one line that counts answer keys, which are never named or opened.
- **Sources.** A link the tutor writes to a file in the study folder with a page opens that page over the lesson, drawn by the bundled PDF reader; ← and → turn the page. In a reply that sets a task, it is plain text until you have answered. ⌘K lists the sources of the last replies.
- **Papers.** ⌘E, "Sit Paper B": the paper opens in your viewer, and the surface at rest holds the paper and three choices (hand in, open it again, end without handing in) until it is done. Handing in opens a lesson on Claude Code to mark it.

### How it looks, and why

- **The app recedes.** One column on a near-plain ground, no chrome, no icons, no cards. The largest thing on the surface is always your material: the next thing at rest, the tutor's words in a lesson. The window's own words are few and small.
- **Three faces, one for each voice, all the Mac's own.** The material and the tutor read in Charter. Your words are in SF Mono, so the two voices of a lesson need no bubbles, names or avatars. The window's few words (keys, the record's lines, a layer's verbs) are in the system face. The window bundles no text face and loads none; KaTeX's faces draw mathematics.
- **One ink.** Four neutrals and one ink per scheme. The ink is the caret, the `›`, the rule of the task you are writing in, and the record's lines (a mark, a change to the plan). Nothing else has colour: no green for right, no red for wrong.
- **The record speaks small.** A mark is a short sentence in the system face and the ink, under the answer it is about: "Marked right, on your own.", "Marked right, with a hint.", "Not right yet. It comes back another day.", "Left for another day." It comes only from the record (`marked`).
- **Yours to set.** ⌘, holds the reading face (Charter, Iowan Old Style, New York, System, Mono), the face of your own words, the text size (also ⌘= and ⌘−), the line length, the scheme (like the Mac, light, dark) and the ink (blue-black, green-black, violet, graphite), kept on this Mac. `<html data-scheme>` set from outside wins over "like the Mac".
- **Reopens as you left it.** The course, the words on each line, which line had the caret, and how far the lesson was scrolled are kept on this Mac. Opened again within three hours, the runtime's own gap between sittings, and on the same day by the record, the window is inside the lesson as it was, without starting the tutor until you write. Later, it opens at rest, with that lesson above, out of view.
- **One motion.** Starting moves the next thing from a third of the way down to the top, in 240 ms. A layer appears in 120 ms. The tutor's caret blinks only while it waits for the first word. With reduced motion, nothing moves.
- **Keyboard first.** Every action has a key, and ⌘K lists every one with its key. Return starts and sends; ⇧Return makes a new line; Esc closes a layer, else stops the reply, else steps out of the task.

### A mark in the lesson

The window never takes the tutor's word for a mark. After each turn the core reads the runtime's record (`magnesium attempts --json`) and sends `marked` with each row that was not there before the message. The page puts that row's sentence under the message it belongs to, and says nothing when there is none. Talk is never marked, so a line on the conversation's line never has one.

### Progress, and what counts

Progress is a record of your own work, kept by the runtime. ⌘Y lists the skills you have answered, newest first, each with the day and the last two marks in words ("Tue 10 Nov, right, with a hint"), and the papers' scores; Return on one asks the tutor to go over it. The surface shows no proof dots, no level, no count of what is due, no run of study days and no countdown to an exam. They are not gone from the record: `magnesium progress` prints them, the tutor poses what is due at the start of a sitting, and ⌘P shows the exam's date as a date.

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
| The real window, end to end: page-to-shell calls, the setup calls and the choice of payment, the first screen's line, a layer that comes with its key and goes with Esc, the content policy, mathematics in a reply, in one line of text and in the learner's own words, a half-written formula held back, a task set apart, a source link shut as text and then open, a PDF page drawn by the bundled reader, pasted text saved and read back, a path outside the study folder and an answer key refused, the library, the plan and the saved conversation, a lesson started, talk sent as talk, a diagram, a permission card, ⌘P over the lesson, and stop with the conversation kept | `scripts/selftest.sh` | passes, against the stand-in, on the debug build and on the release build |
| The window, looked at: the five scenarios of the evidence contract against the stand-in model on Magnesium, in both schemes (`scripts/scenarios.mjs`): the answer marked by the record, the plan's change under the reply, talk and the return to the task sent as talk | by eye, on made-up courses and the stand-in agent | by eye, on the debug build, on 2026-11-12 |
| **The pipe against the real Claude Code**: a new lesson, a second turn, two interrupts, Stop pressed during a reply, and a resume | `scripts/live-check.sh` | Stop during a reply is new and not yet run. The rest passed on 2026-11-10, with Claude Code 2.1.322 and the runtime of 2026-10-23 |
| **A window lesson against the real Claude Code**: Magnesium's tools and nothing of the learner's, a reply of 100 words or more whose first words show before it is whole, the delivered reply recorded as shown, the goal's steps saved, Up next after the window closed mid-lesson, a paper built and marked, and the working line naming the work and no tool or command | `cargo test -p mg-core --test live a_window_lesson -- --ignored --nocapture` | passed on 2026-11-11, with Claude Code 2.1.322 and the runtime of this branch. A 200-word reply's first words showed at 16.1 s and the whole at 20.0 s. The paper was written in Markdown, so no PDF was built in the sandbox |
| The download: both programs report the commit, the signature verifies, no file holds a home folder path, and the disk image verifies | `scripts/package.sh <version>` | passes on a Mac. Only a copy downloaded with a browser shows what first open does |

`npm run shots` photographs every state of the demo's surface with made-up data into `shots/`, in both schemes, with the evidence contract's names. `cargo run -p mg-core --example look -- <folder>` prints what the core reads of a folder.

The live check uses your subscription for six short turns in a scratch folder. It keeps every line between the window and the real program in a trace file, whose path it prints. Each line starts with its direction and the milliseconds since the conversation started. The trace of 2026-11-10 showed:

- In a folder Claude Code has never opened, the session-start hook runs and the plugin starts to install. Then the program answers `initialize`, finishes the install, and sends `system/init`. So the window judges the plugin from `system/init`, never from the `initialize` reply.
- An interrupt before the tutor's first word ends the turn with a `[Request interrupted by user]` line and a `result` of `error_during_execution` whose only error is the program's own diagnostic. The window shows it as interrupted, not as an error.
- An interrupt during a reply ends the turn within about 30 ms. The program sends a reply's text in bursts, so text that had already arrived stays on screen.
- `resetsAt` on a usage-limit line is in seconds.

## Which runtime it runs

The download installs its own copy, as above. Run from source, the window runs whichever `magnesium` is on the path.

A runtime from this repository after 2026-11-10 never upgrades a folder just because `magnesium digest` looked at it. For a folder an earlier version set up, the digest says `upgrade_needed` and holds no record, and the window asks before it runs `magnesium init --upgrade-instructions`.

- **The runtime of 2026-10-23.** It keeps no quick checks, has no `attempts` command and stores no goal steps. The course page shows no checks, the lesson's top strip shows the goal and the next action, the lesson shows no line under an answer, and the progress page lists no answers given with help. The tutor writes the gap log, and the window reads it as written.
- **The runtime on `main` from 2026-11-10** (Magnesium issues 29, 30 and 31). It has no `progress` command, so the window reads the Gaps table that the runtime writes into each gap log. It shows no level, because only gap-log rows could earn proofs there and a level over them would have a low ceiling. Quick checks are kept where the window cannot read them, so they are not shown. The run falls back on the days an answer was sent from the window. If the file differs from the record, as after a hand edit, the progress page says so and shows no rows until the runtime writes the table again.
- **The runtime of this pull request** (Magnesium issue 46). `magnesium progress --json` prints each skill's proofs, the level, what is due and the days with a marked answer. The window takes all four from it, so a hand edit to a gap log changes nothing on screen. For a plan whose gap log the runtime has not read into the record yet, the command says so, and the window reads that plan's file and shows no level until the tutor next marks an answer.

The window refuses every runtime above, and any other that does not meet its needs. Each digest of a runtime from this branch on prints `window_features`, one number for what the window may rely on, and the core holds the lowest number it needs (`WINDOW_FEATURES_NEEDED` in `runtime.rs`). It needs 2: the runtime that lists a sitting's due paper redos as data, which a lesson on Magnesium poses. A runtime that prints a lower number, or none, is too old, whatever its version says: a build from an older commit can report the same version. The readers for the older shapes above stay in the window.

Upgrading the runtime upgrades the learner record in each study folder it opens, once, after keeping a copy.

## Talk, the plan, and what the page hears

**Talk is never marked.** The page sends each message with an intent: `send(text, "talk")` for conversation (a question, a change to the plan, something off the plan), `send(text, "answer")` for an answer to the open task, or no intent, which leaves it to the tutor as before. On Magnesium code holds to it: a talk message's record block has its `answer` set to null and the task it poses ignored, so nothing can be saved as an attempt and the open task stays open; the tutor is told in the lesson state that the message is talk. On Claude Code the window cannot stop the plugin's tutor writing an attempt, so talk goes with one plain line before it, "(This is not an answer to the task, so please don't mark it.)", which the teaching files respect: they record an attempt only for a first try at a task the tutor set. A runtime change that would make it a rule is written up for the runtime.

**The tutor changes the plan.** The exam's date and the order of what to study live only in the plan file, `Study/<plan>/plan.md`: the first date in the Facts table's When row, and the rows of the Blocks table. Both engines' tutors edit that file when the learner asks, without a permission card, log one line under Changes, and say in a line what changed. The direct engine's lesson state names each plan with its exam and date, and its record block can carry a new `goal`, which goes to the runtime as `learning_step`'s `session.use` goal. The record format and what counts as a proof do not change.

**Which task is open.** On Magnesium, after each turn, the engine sends `open_task` with the task open now, in the tutor's own words from its record block, or none, and whether this turn's reply set it. The page puts the field for an answer inside that task. `turns()` reads the open folder's saved conversation from the window's own copy without starting it, so the lesson left can be drawn above what comes next.

**What the page hears.** `plan()` reads the open folder's plan fresh: the nearest dated plan (as the course page picks its exam), its title, date, path and Markdown, its Blocks rows as `items` (each with an id that is a slug of its title, so it survives a reorder, and a Status and Day column when the plan has them), and the goal and steps of the record's last lesson. After each turn the manager waits for the turn's record to be kept, reads the plan again and sends `plan_changed` with what differs (`exam_date`, `items`, `goal`, `steps`, `text`), then reads the record's marks and sends `marked` with each row that was not there before the message. No screen needs to poll `marks()` or re-read the plan after a turn.

## The evidence harness

A dev-only hook lets the real window be driven, photographed and timed. It is inert unless the window starts with `MAGNESIUM_APP_DRIVE=<dir>` or `MAGNESIUM_APP_PERF=<file>`, and it reads and writes nothing outside those two paths.

- **Startup and typing.** Each screen set calls `ready()` from `src/harness.ts` once the first screen has its real content. With perf on, the window writes to the perf file the time from the top of `main` to the paint after `ready()`, then, once a `data-drive="composer"` field is enabled, the median and 95th percentile of 60 keystrokes typed into it, each timed to the next paint; the field is put back after.
- **Driving.** With drive on, the page reads instructions from `<dir>/in/<n>.json` and answers each in `<dir>/out/<n>.json`. `node scripts/drive.mjs <dir> '<op json>'` sends one and prints the answer. The ops are listed at the top of `src/harness.ts`: type, key, click and focus (by a `data-drive` name, else visible text), wait_idle, wait_reply, wait, text, perf, scheme (sets `<html data-scheme>`, which every screen honours), plan, transcript, ready and quit. The primary action on the first screen carries `data-drive="start"` and the field for an answer to the open task `data-drive="answer"` and the field for talk `data-drive="talk"` (a concept with one field for both would name it `composer`).
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

**Where the learner picks.** "Lessons run on" is a choice of two, "Claude Code" and "Magnesium", under the library's title and on the setup screen, and a command in ⌘K. One click picks and saves; there is no confirm step. It is shut while a lesson is open. The lesson's top strip names the engine of its conversation. The words "plugin" and "direct" never reach the screen. Whether Anthropic's terms allow "Magnesium", which starts the user's own copy of Claude Code as the model only, is an open question (Magnesium issue 50, beside issue 38 below). "Claude Code" stays the default.

**A conversation keeps its engine.** The window stores the engine the learner picked for new lessons, and the engine that started each folder's conversation beside its id. A conversation resumes only on its own engine. When the learner picks the other engine, the next lesson is a new conversation, even from Continue, and the record carries the progress across. The library says so under the choice until that lesson starts: the direct engine resumes the runtime's lesson in it, with its goal, steps and open task.

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
  tests/            the pipe tests, the stand-in agent, the readers against
                    the runtime, paying and signing in, installing, upgrading
                    a folder, the live check
src-tauri/          the window's shell: passes messages, decides nothing
src/                the page (React, plain CSS)
  App.tsx           the surface: the next thing at rest, the lesson, which
                    line has the caret, the keys, and which layer is up
  surface/          Composer (the lines you write on), Conversation (the
                    lesson's turns, marks, plan changes, requests), Layer
                    (the one layer) and Layers (what each key fills it
                    with), Sheets (a source, pasted text), Rich (text with
                    mathematics and diagrams)
  lesson.ts         what the lesson holds and how each event changes it
  prefs.ts          your settings, and where you were, kept on this Mac
  words.ts          the window's own words: marks, plan changes, days
  pdf.ts            the bundled PDF reader
  sources.ts        which links in a reply are sources, and their page
  harness.ts        the evidence harness's page side: ready(), timing, and
                    the drive ops
  demoBase.ts       the demo's plan, talk and the two events, kept apart
                    from demo.ts
  render.ts         Markdown and mathematics to safe HTML
  styles/           surface.css (tokens, faces, the surface, the lines,
                    layers), prose.css (the tutor's words)
scripts/            drive.mjs, capture.sh, scratch-course.sh and
                    scenarios.mjs for the evidence harness (see above);
                    check, selftest, live check, screenshots, package (the
                    download, which .github/workflows/mac-download.yml runs
                    on a GitHub-hosted Mac); make_icon.py draws
                    the app icon as a 1024-pixel PNG, written to the path
                    given as its argument
```

The window's own state is in `~/Library/Application Support/dev.magnesium.window/`, apart from the runtime's folder: the folders in your library, each folder's conversation id and stop line, a copy of the turns it showed, how many messages you sent on each day, how you chose to pay for lessons, and a `magnesium` version you chose to keep. None of it is learning state, and none of it is a credential.

## What is not there yet

- **A due check is the tutor's to pose.** The surface does not count what is due. The runtime stores the next question and its key for each due item (Magnesium issue 30), but no command prints them, so the window cannot show the first one itself.
- **One fact comes from the runtime's files, not from a command**: the steps of the current goal. `peek.rs` reads them read-only and gives nothing when the layout changes. It should become a runtime command. `peek.rs` also still reads the quick checks of the runtime of 2026-10-23, which has no `progress` command.
- **Goal steps need a newer runtime.** ⌘P shows the lesson's steps when the checkpoint holds them. The runtime of 2026-10-23 stores none.
- **A step's tick is the tutor's judgment.** The stored field is the tutor's own `done`, so the window shows the tick and does not say that you produced the step alone.
- **A mark needs the record.** If the tutor did not mark an answer, the record did not change, and the line under the answer says nothing.
- **On Claude Code a task is found by its first words.** That engine does not say which task is open, so a task is a paragraph that opens with a bold "Task.", "Check", "Question" or a similar word, and a task written any other way gets no line of its own: an answer to it goes on the conversation's line, as talk. On Magnesium the engine says which task is open (`open_task`).
- **A photo is sent as a path.** The window saves a shrunk JPEG in `Study/handed-in/` and tells the tutor where it is. The tutor reads the file.
- **A source is read, not searched.** The layer shows a page. It has no search inside the page, no selection of its text, and no zoom. A PDF with a script, a form or a font it needs from outside the file is drawn as pdf.js draws it, and a page that cannot be drawn says so.
- **Files cannot be pasted.** A file copied in Finder and pasted into the window gives the page no path. Drop it, or choose it.
- **Adding does not tell the tutor.** The tutor finds a new file the next time it searches. Say so in the lesson if it should look now.
- **No Apple signing or notarization, no update from inside the app.** Apple's signing needs a paid Apple membership. Until then each new version needs the one-time approval again.
- **Claude Code is checked, not installed.** Anthropic's terms set conditions for a product that installs or runs Claude Code. Whether they apply to an app that starts the user's own copy is an open question (Magnesium issue 38).
