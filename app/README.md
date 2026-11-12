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

- **The `magnesium` program.** It is inside the app, built from the same commit. On first open the app puts it in `~/.local/bin`, where Claude Code's own installer puts `claude`. The window, Claude Code's hooks and a terminal then all run that one copy. If another version is already installed, the app asks before it replaces it, because a newer program upgrades each study folder's record when it opens the folder. If you keep the other version, lessons run on it, and the app says what that version cannot do. A program older than the window needs is refused, whatever its version number says: the app offers to replace it, or, run from source, names the install guide, and starts no lesson until it is fixed. "Keep" is not offered for it.
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

To click through every screen with made-up data and no agent, run `npm run dev` and open http://127.0.0.1:1430 in a browser. The end of the address chooses the screen: `#library`, `#course`, `#session:answer`, `#summary`, `#progress`, `#paper:running`. `#welcome:replace`, `#welcome:signed-out`, `#welcome:no-claude`, `#welcome:key`, `#welcome:too-old`, `#welcome:too-old-source` and `#course:upgrade` show the setup states. `#library:magnesium` shows Magnesium picked to run lessons, `#library:lesson` the library with a lesson open, `#library:continue` Enter on the main card, `#session:magnesium` a lesson on Magnesium, and `#summary:unmarked` a sitting in which no answer was marked. `#course:new` is a new folder's first step, `#course:material`, `#course:drop` and `#course:paste` show adding material, and `#session:source`, `#session:key`, `#session:material` and `#session:drop` show a source open beside a lesson, an answer key refused, and adding in a lesson. `scripts/shots.mjs` lists them all.

## What is on screen

| Screen | What it holds | What it leaves out |
|---|---|---|
| **Library** | One card for each course: its next exam and the days left, what you last worked on, its level (or, for a course with no plan, how many skills are proven and in progress), and how much is due today. The most urgent course is the large card. Its button, and Enter, continue its lesson at once, as Continue on the course page does, and "Course page" opens its page. A course that is new, needs an upgrade or has a paper running opens its page, which says what comes first. Under the title, one quiet choice of what lessons run on, "Claude Code" or "Magnesium" (see [Two engines](#two-engines)), with one line about the one picked. It is saved at once, and it is shut while a lesson is open. A new pick takes effect with the next lesson, which starts a new conversation, and the record carries the progress across. This week's study days. | Statistics, a list of sessions, anything about a course that does not help you choose it. |
| **Course** | One card: what comes next, where you stopped, and one button. Under it, one quiet line of today's counts, such as "6 answers marked today · 2 new skills · 1 proof · 3 due tomorrow", with any count of zero left out. Under that, three tiles: your proofs, a timed paper, a new lesson. Under those, a strip to add course material. A folder made in the window starts with that as its first step, before it asks what to learn. | Everything else about the course. |
| **Lesson** | The tutor's reply, appearing as it is written, one answer box, "Hint", "I don't know" and "I'm lost". While the tutor works, one line that names each step as it starts: "Thinking", then "Saving your work" while the record is written, and for long work "Writing paper 2 · 12 min". The reply takes its place as it is written. In the top strip: the goal's steps as marks, the minutes left in the block, and, quietly, the engine this conversation runs on: "Claude Code" or "Magnesium". A click on the steps shows the goal. When the tutor asks you to choose, the answer box gives way to the choices, so there is one thing to answer. A paperclip in the answer box adds course material. A source the tutor cited opens in a panel beside the reply, at its page. | Tool calls, save messages, token counts, the model's name, other sessions, a sidebar. The working line never shows a tool's name, a command, or a path outside the study folder. The line the window sends to open a sitting is shown as one quiet line, not as something you said. |
| **Sitting done** | How many answers the tutor marked in the sitting, and one plain sentence when you sent messages and none was marked. The proofs earned in the sitting, when there are any. Today's counts, as on the course page. Where you stopped. Today's place in the week. | Any mark or proof the runtime's record does not show. The window compares the record before and after the sitting, and never takes the tutor's word for one. A sitting that crosses midnight counts the answers marked after midnight, and says nothing about marks when there are none. |
| **Proofs** | One row for each skill, with three dots: unaided answers on separate days. The level, the papers' scores, and five weeks of study days. | Minutes and sessions. Answers given with help are listed apart and never counted. |
| **Paper** | A countdown and Hand in. The tutor is hidden until you hand in. The window never opens an answer key. | Everything else. |

### Course material, and sources

- **Adding.** Drop files or a folder anywhere on a course page or in a lesson, choose them with a file dialog, or paste text. Each file is copied into `Study/materials/` in the study folder, under its own name, and never over another file. That folder is under `Study/`, where the tutor keeps the rest of the course. It is not under `.magnesium/`, which search never reads, and it is not a name search skips. Pasted text is saved there as a `.txt` file. A paste with nothing selected to type in, on the course page, opens a sheet to keep it. A paste into the answer box stays an answer.
- **What it says.** After an add, the window says in a few lines what it added and what search cannot read: a PDF with no text layer, a file left out because of a word in its name, a kind of file search does not read. It does not work this out itself. It runs `magnesium searchable`, which asks the same rules and the same PDF reader that the tutor's search uses. With a runtime that has no such command, it says so. The window changes neither rule. For a folder not set up yet it asks the same, and sets nothing up.
- **Answer keys.** A file that looks like an answer key (the rule that keeps papers' keys closed) is added like any other. The window never names it or opens it, and counts it in one line.
- **Sources.** The tutor cites a passage as a link whose text is the file and page and whose target is the path from the study folder with a page, such as `Study/stats/week4-slides.pdf#page=12`. In a reply that sets a task, the link reads as plain text until you answer. Then it opens that page in a panel beside the lesson, and a row under your answer keeps the last reply's passages in reach while the next reply is written. The panel closes when the tutor sets the next task, when you close it, or with Esc. A reply that sets no task has nothing to answer, so its links are open. A PDF is drawn by a reader bundled in the app (pdf.js), from the file's bytes, with no server and no network request. A text file shows as text. Only the study folder is read, never an answer key, whatever the link says. A link to a web page, an image or an HTML file opens as before.

Everything that is not the lesson is behind one key, **⌘K**: the library, your proofs, a paper, a new lesson, the block length, what lessons run on, this Mac's setup.

Other keys: **Enter** sends, **Shift+Enter** makes a new line, **⌘.** stops a reply, **⌘N** starts a new lesson, **⌘O** opens a folder, **Esc** goes back.

### How it looks, and why

- **Ink on paper.** The light scheme is cool exam-booklet paper with blue-black ink; the dark scheme is a blue-black ink page with paper-white text. Colour is spent only on what you have proved and on the one thing that moves you on. Green means a proof, the one primary button on a screen, and the task in hand. Amber means due, and nothing else. Red means a fault that blocks you. Everything else is ink: a link, a chip, a study day, a clock, a chosen option, the focus ring. There are no gradients, glows or washes.
- **Two voices, both bundled.** What is read is set in Literata (a tutor's words, a title, a skill's name, anything with mathematics in it); what is pressed or glanced at is set in Magnesium Sans: Atkinson Hyperlegible Next, which keeps `I`, `l` and `1` apart, with its zero redrawn without a slash, because a slashed zero reads as the empty set in an answer such as `1/[S] = 0` (`src/fonts`, under the SIL Open Font License). Both are bundled into the app, so the window still makes no network request. Labels are sentence case at normal tracking, nothing is set below 12px, and only weights 400 and 600 are used.
- **One set of tokens.** `src/styles/base.css` holds every colour, a spacing scale (`--s1` to `--s8`: 4, 8, 12, 16, 24, 32, 48, 64), three radii (`--r-control`, `--r-card`, `--r-pill`), a type scale (`--fs-caption` to `--fs-display`: 12, 13.5, 15, 18, 22, 30, 42) and two motion times (`--t-fast`, `--t-move`). A value that is not on a scale is a mistake.
- **Flat.** A surface is told from the page by tone and a hairline. Only a layer that floats (the command palette, a menu, a sheet, a popover) has a shadow. A card you can press changes its border and fill on hover and does not lift.
- **Controls.** One primary button (40px, green, label only, one to a screen), one secondary (36px, hairline), one quiet. A shortcut is in the button's tooltip and in ⌘K, never inside its label. The focus ring is 2px of ink, for the keyboard only.
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

- **The window is a layer.** Every teaching rule stays in the Magnesium runtime and its plugin. A button sends plain learner words, such as "I'm lost" or "Can I have a hint?". A lesson on Claude Code started here continues in a terminal with `claude --continue`. A lesson on Magnesium runs in the window, through the same runtime and record (see [Two engines](#two-engines)). The one difference is how a reply reaches the screen: in the window the tutor sends it through Magnesium's `reply` tool, whose words stream as they are written, where text after a tool call would arrive in one piece.
- **Your own agent and subscription.** The window starts the `claude` program you installed, as published, in your study folder. Signing in is Claude Code's own (`claude auth login`), in your browser. The window reads no credential, and never passes `--bare`, which would skip Magnesium's hooks.
- **A lesson loads what it needs and nothing else.** The window starts Claude Code with Magnesium's plugin, from where `magnesium init` put it, and nine of Claude Code's own tools: Read, Write, Edit, Glob, Grep, AskUserQuestion, WebSearch, WebFetch, and Bash inside Claude Code's sandbox. Write and Edit never touch `.magnesium/`. Settings come from the study folder only, so your other plugins, hooks, skills and tool servers stay out, and so do your account's connectors, such as mail and calendar. The model is named (`claude-fable-5-2`, Claude Code's default on 2026-11-11), so a later change of that default cannot change the tutor unseen. It thinks at Claude Code's default effort, as in a terminal. A lesson in a terminal is unchanged.
- **Local files.** No cloud service, no account, no telemetry. Material you add is copied into the study folder and goes nowhere else. The window makes no network request of its own, and an image from the web inside a reply is not loaded. It looks for study folders only when you press **Find folders on this Mac**.
- **The runtime stays small.** The window links none of the runtime's code. It runs `magnesium digest` and `magnesium progress`, and reads your plan files.

## What was checked

| Check | How to run it | Result on 2026-11-11 |
|---|---|---|
| Everything below but the live check | `scripts/check.sh`, and `scripts/check.sh --window` for the self-check too | passes |
| Core: readers, state, files, the rows and the level from the runtime's record, the run of study days and its rest day, the look for study folders | `cargo test -p mg-core --lib` | 58 pass |
| The library: every folder is listed, the nearest exam first, and a folder that was never set up is shown as new | `cargo test -p mg-core --test folders` | passes, against a stand-in runtime |
| The pipe: several turns in one process, permission allowed once, always and refused, a question answered, a reply interrupted, a lesson closed and resumed, a missing plugin reported, a helper's text hidden, a usage limit shown, a conversation the agent no longer has, a reply interrupted before its first word, a stopped or replaced conversation that says nothing more, and what a lesson loads: nine tools, the folder's settings, no account connector, the named model and no effort setting | `cargo test -p mg-core --test pipe` | 14 pass, against a stand-in agent |
| Paying: with the plan chosen, an API key in the environment does not reach the agent; with a key chosen, it does; a key the agent keeps itself is named before and during a lesson; signed out, the agent's own sign-in runs and the window sees the result; without the agent, its install page | `cargo test -p mg-core --test payment` | 3 pass, against the stand-in agent |
| The program inside the app: installed once without a question, another version kept or replaced only on a yes, asked again for a new version. A copy older than the window needs, by the number its digest prints, refused and never kept, with the install guide named when the app carries no copy | `cargo test -p mg-core --test install` | passes, against stand-in programs |
| A folder an earlier version set up: the window asks, starts no lesson, changes nothing, then upgrades with a copy of the record | `cargo test -p mg-core --test upgrade` | passes, against the runtime built from this checkout |
| A paper opens from a folder inside the study folder, and an answer key does not | `cargo test -p mg-core --test papers` | passes, against a stand-in runtime |
| Course material: copying, a name that cannot climb out, a link that cannot lead out, the 200 MB limit, a dropped folder, pasted text, an answer key counted and never named, what search cannot read said in one line each | `cargo test -p mg-core --lib material` | passes |
| A cited source: a PDF and a text file open inside the study folder, and nothing outside it, through a link, or an answer key by its name, its folder or a link to it | `cargo test -p mg-core --lib files` | passes |
| Course material against the runtime built from this checkout: a PDF dropped on a course is found by the tutor's own search at its page, a scan and a name word and an unread kind are each said, a folder not set up is judged and left as it was, an older runtime is admitted to | `cargo test -p mg-core --test material` | 3 pass |
| The readers against the runtime built from this checkout: its digest, `progress --json`, `attempts --json`, the gap log it writes, the goal's steps, the proofs and the level on the progress page and on the library's card, a quick check from a chat lesson, the study days, and a gap log edited by hand | `cargo test -p mg-core --test runtime` | passes |
| The real window, end to end: page-to-shell calls, the setup calls and the choice of payment, the library and the study days, the content policy, mathematics in a reply, in one line of text and in the learner's own answer, a half-written formula held back, a task set apart, a diagram, a permission card, a PDF page drawn by the bundled reader under the content policy, a source link shut as text and then open, pasted text saved and read back, a path outside the study folder and an answer key refused, the end of a sitting, stop and resume | `scripts/selftest.sh` | passes, against the stand-in, on the debug build and on the release build |
| The release build's window, looked at: the library, a course, a lesson with formulas, matrices and a table, the steps, the end of a sitting and the proofs | by eye, on made-up courses and the stand-in agent | drawn as in the browser, on 2026-11-11 |
| **The pipe against the real Claude Code**: a new lesson, a second turn, two interrupts, Stop pressed during a reply, and a resume | `scripts/live-check.sh` | Stop during a reply is new and not yet run. The rest passed on 2026-11-10, with Claude Code 2.1.322 and the runtime of 2026-10-23 |
| **A window lesson against the real Claude Code**: Magnesium's tools and nothing of the learner's, a reply of 100 words or more whose first words show before it is whole, the delivered reply recorded as shown, the goal's steps saved, Up next after the window closed mid-lesson, a paper built and marked, and the working line naming the work and no tool or command | `cargo test -p mg-core --test live a_window_lesson -- --ignored --nocapture` | passed on 2026-11-11, with Claude Code 2.1.322 and the runtime of this branch. A 200-word reply's first words showed at 16.1 s and the whole at 20.0 s. The paper was written in Markdown, so no PDF was built in the sandbox |
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

The window refuses every runtime above, and any other that does not meet its needs. Each digest of a runtime from this branch on prints `window_features`, one number for what the window may rely on, and the core holds the lowest number it needs (`WINDOW_FEATURES_NEEDED` in `runtime.rs`). It needs 2: the runtime that lists a sitting's due paper redos as data, which a lesson on Magnesium poses. A runtime that prints a lower number, or none, is too old, whatever its version says: a build from an older commit can report the same version. The readers for the older shapes above stay in the window.

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

## Two engines

A lesson runs on one of two engines. Both reach the model through the learner's own Claude Code, so sign-in, account and payment are Claude Code's in both.

- **Plugin** (`Engine::Plugin`, the default; the learner sees it as "Claude Code"). Claude Code runs the lesson through Magnesium's plugin, as described above: it loads the teaching files, keeps the record with `learning_step`, and replies through the `reply` tool.
- **Direct** (`Engine::Direct`; the learner sees it as "Magnesium"). The window runs the lesson (`crates/core/src/direct.rs`). Claude Code is started as the model alone: Magnesium's tutor prompt in place of Claude Code's (`--system-prompt`), no settings files (`--setting-sources ""`), so no plugin, hooks, skills or tool servers of the learner's or the folder's; no CLAUDE.md, no auto memory, no account connectors; the same named model and effort as the plugin engine. Its tools only look things up: Read, allowed only inside the study root (`Read(//<root>/**)`, with `--add-dir` for a root above the open folder; a bare `Read` would approve any path on the disk, and a live check showed it did), and Magnesium's `search_sources` and `sources`. The record tools are disallowed, which takes their definitions out of the model's context: a first turn reads about 4,000 tokens, against about 20,000 with the five tools.

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
- It makes no files: no figures, pages or plans. It asks no permission and no multiple-choice question.
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
  src/runtime.rs    runs the magnesium program and reads what it prints
  src/install.rs    puts the app's own magnesium on this Mac, asking first
                    before it replaces another copy
  src/material.rs   copies added course material into the study folder, and
                    words what search can and cannot read of it
  src/files.rs      handing in work, opening a paper, and reading a cited
                    source inside the study folder
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
  sources.ts        which links in a reply are sources, and their page
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
- **A source is read, not searched.** The panel shows a page. It has no search inside the page, no selection of its text, and no zoom. A PDF with a script, a form or a font it needs from outside the file is drawn as pdf.js draws it, and a page that cannot be drawn says so.
- **Files cannot be pasted.** A file copied in Finder and pasted into the window gives the page no path. Drop it, or choose it.
- **Adding does not tell the tutor.** The tutor finds a new file the next time it searches. Say so in the lesson if it should look now.
- **No Apple signing or notarization, no update from inside the app.** Apple's signing needs a paid Apple membership. Until then each new version needs the one-time approval again.
- **Claude Code is checked, not installed.** Anthropic's terms set conditions for a product that installs or runs Claude Code. Whether they apply to an app that starts the user's own copy is an open question (Magnesium issue 38).
