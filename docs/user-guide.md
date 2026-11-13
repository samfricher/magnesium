# Using Magnesium

Magnesium helps your usual assistant teach understanding you can use independently. It draws on your materials, current work, and prior learning to choose what to explain, practise, and revisit. Workspace-local memory preserves descriptions of your understanding and difficulties, backed by your original attempts, so the tutor can build on them across conversations.

## Set up a study folder

[The install guide](install.md) tells your coding agent how to build Magnesium and set up a study folder. To set up another, run `magnesium init /path/to/your/course`, or run `magnesium init` from that folder. Initialization creates private `.magnesium` state and installs the packaged teaching instructions; in an existing Git repository, it adds `.magnesium` to local exclusions. Your study materials stay in place. Subfolders share its memory unless you initialize a separate workspace inside one. Incompatible learner data is preserved and rejected without migration.

Magnesium is built and tested for Claude Code. Its learner record, files and tools are open to any agent, but the lesson hooks, the skills and the plan templates work only in Claude Code. An adapter for another agent is separate work.

- **Claude Code:** initialization sets up Magnesium's Claude plugin for this folder only. The CLI and the desktop app read the same files.
  - It copies the plugin to Magnesium's per-user data directory.
  - It declares that copy as the `magnesium` plugin source in your Claude user settings (`~/.claude/settings.json`, or `$CLAUDE_CONFIG_DIR`). That declaration turns Magnesium on nowhere.
  - It turns the plugin on only in this folder's `.claude/settings.local.json`.
  - In that file, it also adds Magnesium's permission rules to `permissions.allow`. Magnesium then runs in this folder without permission prompts.
  - `mcp__plugin_magnesium_magnesium__*` allows Magnesium's tools. A `Read` rule allows reads of the copied plugin, where the skills keep their templates. `Skill(magnesium:...)` rules let Magnesium's skills start without a prompt.
  - `Bash(magnesium digest)` allows the read-only command that `/magnesium:help` runs.
  - Claude Code applies `ask` and `deny` rules before `allow` rules. Initialization does not change those two lists.
  - To be asked before Magnesium's tools run, put `mcp__plugin_magnesium_magnesium__*` under `permissions.ask`. To block them, put it under `permissions.deny`.
  - To turn Magnesium off in one folder, set `"magnesium@magnesium": false` in that file. Initialization keeps that choice.
  - Start a new Claude Code session in the folder afterwards. Keep `magnesium` on `PATH`.
- **After upgrading Magnesium:** run `magnesium init --upgrade-instructions` once in each study folder, then start a new session. It does everything `magnesium init` does, including the plugin refresh and permission rules above. It also switches the folder to the new teaching instructions. Until you switch it, a folder keeps the instructions it was set up with, and a newer `magnesium` serves only its own. So the tutor reports Magnesium's instructions as unavailable, and plain `magnesium init` stops with an error there. The switch happens only if the old instructions were not edited. It keeps the old files and leaves learner history untouched.
- **Claude Code reply length:** during a lesson, most of your messages reach the tutor with a reminder to teach one idea in under 200 words and to end with one task. If you are confused, the reminder also tells the tutor to ask you one question to find out why before explaining. While recording is on, the reminder can also say that the tutor's previous reply had 200 words or more. The tutor still writes more for a worked example, a full explanation you asked for, a marked paper or a plan. It skips the task when you turn quizzes off or end the lesson.
- **Claude Code `/clear`:** Magnesium's tool connection keeps the conversation it started with, so after `/clear` Magnesium refuses to read or record learner state until you start a new Claude Code session. Start a new session instead of using `/clear` during a lesson.
- **Claude desktop pastes:** the desktop app sends pasted text between hidden `<pasted_content id="…">` and `</pasted_content id="…">` lines. Magnesium records a Claude message without those tag lines, blank lines before your first line, or whitespace after your last, so a pasted message keeps your exact words, including indentation, whether the hook records it or the tutor echoes it to start or resume a lesson. A message that reaches Magnesium without Claude Code's hooks is recorded exactly as sent.

## Start with a request

In Claude Code, `/magnesium:help` looks at the folder and your learner record and suggests what to do next, and `/magnesium:help papers` shows how practice papers work. It reads both through `magnesium digest`, which prints the facts it uses as JSON and changes nothing; you can run it yourself in the study folder.

Ordinary project work doesn't start a lesson. Ask explicitly:

- “Help me understand recursion using this repository.”
- “Practise conditional probability with me. One question at a time.”
- “Review what we covered last time, then pick up where we stopped.”
- “Explain the solution directly. No quiz today.”

Give the tutor your attempts, questions and constraints. It can use authorized local files and preserve a checkpoint. You don't need to maintain memories yourself.

If a new kind of problem leaves you without a way to start, the tutor works through a similar problem, then gives you a changed version to solve yourself. If that kind of problem stops you again, the tutor leaves some steps for you to fill in. After that, you get the problem alone. If you can't finish the practice problem yourself, or forget the method later, the tutor goes back a step and gives you more help next time. Magnesium keeps count of this for each kind of problem from your marked answers, so the help fades even when you come back days later in a new conversation. If you say you are confused, or ask a question because you are lost, it first asks one question, a small concrete case of the idea in plain words, without code or symbols, that takes under a minute, and uses your answer to choose what to explain. A plain factual question about something already covered, such as what a name stands for, gets a one-line answer and no question back.

When you start a topic you have not answered anything on before, the tutor asks up to three short opening questions, one at a time: an easy one, one from the middle of the way to your goal, and the hardest step the goal needs. Each is a concrete case you can answer in about a minute, not a definition to recite. It explains nothing between them. At the first one you miss, it stops and starts teaching there. It says in one line what you got right and builds on it, without teaching it again. If you get all three right, it aims higher. It skips the opening questions when you have answered questions on the topic before, when you ask a plain factual question, and when you say where to start. Names and symbols come after you have done a step. Every answer you give to a question the tutor set is marked, including "I don't know".

If you ask for a hint, the tutor gives the smallest help that lets you take the next step: a question or a pointer, not the step itself. A skill you get right only with a hint comes back the next day as a quick check, to answer alone, and keeps the passes it already has. So does a question you answer "I don't know". Declining a quick check that is due costs nothing. On a quick check or a redo that is due, the tutor tells you before it gives a hint that an answer after a hint does not count as a pass.

Before it explains a step you have not seen, the tutor asks what you think comes next and why. A rough guess is fine, and a wrong guess is never recorded against you. The explanation then starts from what your guess got right and wrong, and ends with a check question. It skips the guess when you asked for an explanation or a full solution, when you say you are confused, when it shows you a worked example because you had no way to start, when it is repairing a missed check, and right after it asked you another question. A check question doesn't count: after you pass a check, the tutor still asks for your guess on the next step.

Before it explains something new, the tutor names a skill you have already shown in your own work and builds from it. These skills are called anchors. If you have no way to start a new kind of problem, it shows the worked example first, built on an anchor if you have one that fits. If you know how to start, you try first. Otherwise, if no anchor fits, it first asks what you already know that looks like the new idea. It skips that question when it has just asked you one, such as how you see a problem that confuses you, when you asked for an explanation or a full solution, and when it is rewriting a task that stopped you twice. When you then answer a check question correctly on your own, with no hint or solution from the tutor and without copying a solution it just showed you, it adds the approach that worked to your anchors. It keeps them in your learner record, and a practice plan keeps a copy of the ones it uses in its Anchors section. If the two differ, the learner record is right, and the tutor updates the plan whenever it adds, corrects or retires an anchor.

When the tutor draws a figure or a page as a file, it renders it to an image and looks at it before you see it. It fixes overlapping or cut-off parts, unreadable labels, decoration with no label, and anything the picture gets wrong, then looks again. This needs a browser that can run headless, such as Chrome, Chromium or Edge, or an SVG-to-PNG converter such as `rsvg-convert`. Without either, the tutor says it cannot check the figure and uses text or a Mermaid diagram instead.

An answer counts only when it rules out the likely mistake. If a wrong idea could have given the same answer, the tutor asks one more question that tells the two apart. A step in your plan counts as done only after you answer it on your own on a later day. If you ask for a model answer, you get it, and the tutor asks you for it cold the next day. When you ask for a study plan, the reply ends with one task you can answer in the chat in about two minutes, not a request to go and read the plan.

If a memory is wrong, say so: “That wasn't a conceptual gap; I misread the question. Please correct the memory.” The tutor can inspect your original work and revise its interpretation while keeping the history.

## Practise with timed papers

In Claude Code, say what you are studying for: “I have a midterm Monday” or “Build me practice papers for chapters 6 and 7”. You can also run `/magnesium:papers`. The tutor asks only for what your message and folder do not show. It then writes a plan, papers with separate keys, drills and a gap log in `Study/<target>/`, for example `Study/midterm-1/`. The tutor also keeps a map of the course's skills in `graph.yaml` and uses it to place you and to choose repairs. It asks when and where you will practise, suggests a time, and lets you skip the question. It writes your answer in the plan as if-then lines, with one backup for a missed session, for example “If it is Monday to Friday at 08:00, then I do the due cold redos at my desk before class.” On a day with a planned session, `/magnesium:help` names the session and how many redos are due. LaTeX papers build with `tectonic`. The build stops at a line that would run off the page, and the tutor rewrites that line before you get the paper. The tutor asks before it installs tectonic; without it, the tutor writes the papers in Markdown or HTML.

Sit each paper on your own, timed, and write `?` where you would look something up. After each answer, write how sure you are: S if you are sure, U if you are unsure or G if you guessed. Send photos or a PDF of your answers and say “mark my Diagnostic”. The tutor marks against the key and repairs each lost mark. Magnesium writes each part that lost marks as one row of the gap log and dates its cold redos, so a redo date is never missing or mistyped. The marking shows how many answers you got right and wrong for each letter. The tutor repairs the mistakes you were sure of first, because you would not think to review them. Your letters never count as a pass. Only your marked answers do. A gap counts as fixed only after you pass it cold on three different days, at least 1, 3 and 7 days apart, or closer together when the assessment is sooner. Every mock has at least one question of each of three shapes that recognising the problem type cannot pass: say why a given result holds, tell two look-alike problems apart, and say what breaks when one assumption is dropped. A question that gives you the result earns full marks only for a correct reason. Cold redos use these shapes for a gap in understanding. In a cold redo, a reason question always comes second: you first produce the result from a blank page, and the redo passes only if both parts are right. The tutor keeps the plan file current and asks before it adds a lot of time. Later conversations continue from the plan. Say “rebuild” to start over.

## Start each sitting with what you got wrong

A sitting is one stretch of study. It begins with your first message after about three hours without one. A compaction does not begin one.

When you get a task wrong on your first try, the tutor gives the mistake a short label, such as "sign of b in the quadratic formula". Magnesium then schedules a quick check on that label for the next day. The next sitting opens with what is due:

- **Quick checks:** up to three, about a minute each. Each sets a new problem to answer in a line or two, without saying which step it checks, so you have to use the step on your own. Then it shows lettered options, and the wrong ones are the mistakes you made before. A right answer with no help moves the check on to 3 days later, then 7 days later, and after three passes on three different days it is done. A wrong answer is repaired, and the check comes back the next day. So does an answer you reached only after the tutor showed you the full solution, or a full solution you asked for before trying.
- **Paper redos:** the tutor says how many gap-log redos are due and which comes first. They run as the first block of your paper session.

When the tutor stores the next question for a check or a redo, the sitting starts with that question, and the tutor keeps its answer back until you have replied.

You can decline any of it. A decline costs nothing: it is recorded as your choice, and the check stays due for the next sitting. "Skip", "no quiz" and "stop" still win. Each sitting adds one line to `.magnesium/sittings.log`: what was due, what was posed, answered or declined, what passed or failed, and how many more checks were due than the sitting had room for.

To turn this off for a study folder, run `magnesium opener off` there. Checks are still scheduled, but sittings no longer open with them. `magnesium opener on` turns it back on. `magnesium opener block` also makes the tutor wait to teach new material until each quick check has been asked or declined.

## Your gap log

Magnesium keeps every gap-log row in your learning history and writes the Gaps table of `gap-log.md` from it after each change. It works out each row's passes, status and next redo date itself, with the rule above, and makes the gaps shorter to fit before the date in your plan's When row. If a row is wrong, tell the tutor: it corrects the mark or the row, and the table is written again. If you edit the table by hand, nothing about what is due changes; the next time Magnesium writes the table, it saves your edited file in `.magnesium/backups/gap-logs/` and the tutor asks what you meant.

The first time the tutor loads a paper stage for a plan, Magnesium brings in that plan's gap log with its rows, passes and dates, from any earlier template, and keeps the original in `.magnesium/backups/gap-logs/`. A row it cannot read stops that: `magnesium digest` lists it, and saying "fix my plan files" has the tutor correct it.

Magnesium also works out where you stand on each tag in `graph.yaml`, from the gap log, your marked answers and the graph:

- **untested:** no answer from a blank page yet;
- **open:** a gap-log row on it that has not passed a redo since its repair;
- **conditional:** a row part way through its three redos, a tag it relies on that is open, or passed only slowly, partly, with help or on the day it was taught;
- **sound:** answered right from a blank page, on your own and in time, itself or as part of a bigger question, with nothing it relies on open.

These choose what the tutor repairs and what the next mock covers. They change as soon as a row changes, for example from open to conditional when you pass the first redo. If `graph.yaml` cannot be read, the tutor is told which line is wrong, and the gap log alone decides until it is fixed.

## See every answer you gave

Each answer the tutor marks, in chat or on a paper, is kept as its own row: the question, the mark, when, the help you had before answering, how sure you said you were, the skill it tests and whether it was cold, meaning unaided, from a blank page, on a later day than the teaching. An answer you were helped with stays marked as helped, and only your first answer on a skill each day can count as cold. From the study folder, `magnesium attempts` lists them, for example:

```text
2026-11-10 09:15  MockA Q2a: Find E(X) for the table
    partly 1.5/3, help none, confidence unsure, cold, kind discrete expected value. Tests: expected value
```

Add `--json` for the rows as data. They are also in an export.

## See how far each skill is proved

`magnesium progress` adds those answers up. It lists each skill with its passes, from none to three. A pass is one answer you gave right, alone and cold, on its own day, and three passes prove a skill. A skill you got wrong earns its passes by the redos the tutor schedules. A skill you never got wrong earns one for each day you answer it right. The command also prints a level from 1 to 10 for each plan, which is the share of the plan's passes you have earned, and the days on which an answer of yours was marked. A plan's level counts only the skills in its course graph, so it never falls because you start something new. It counts each skill at the most passes it has held, so it never falls when you miss either. The dots still show the passes you hold now: a miss after a pass starts that skill's passes again. A chat answer on a skill the graph names is recorded under the graph's name, so chat lessons on the course move the level too. Skills in no plan are listed with their passes and counted as proven or in progress, with no level. For example:

```text
midterm-1: level 5 of 10 · 4 of 9 passes · 1 of 3 skills proven
  ●○○  slope and Km · due
  ●●●  rate equation · proven
  ○○○  Hill coefficient · waits for its repair
In no plan: 0 proven, 1 in progress
  ●○○  units of a rate
Days with a marked answer: 2 in all. The latest: 2026-11-08, 2026-11-09
```

Add `--json` for the same facts as data. The study window reads them from there. The data also counts what you did today: the answers you gave, the skills you answered for the first time, the passes you earned and the checks due tomorrow. The command changes nothing in the folder.

The first time this version opens a study folder from an earlier version, it upgrades the learning history once. It first copies it to `.magnesium/backups/`. Quick checks kept in `.magnesium/sittings.sqlite` by an earlier version move into the learning history then too. So do the help steps and tag readings the tutor wrote into your learner record, such as "example step (discrete expected value): partly worked" or "sound from MockA Q3". To go back to the earlier version, put that copy in place of `.magnesium/state.sqlite`.

## Add course material, and read a source beside the lesson

In the study window, drop files on a course or a lesson, or choose them with the paperclip or the button on the course page. You can also paste text. The window copies each file into `Study/materials/` in the study folder, under its own name. Pasted text is saved there as a `.txt` file. The tutor's source search reads that folder like any other. A folder you make in the window offers this as its first step.

After it adds, the window says in a few plain lines what it added and what search cannot read. A PDF with no text, such as a scan, is listed as one whose text cannot be searched. A file whose name has the word key or keys is left out of search, in case it is an answer key or holds a password, and the window names the word. So is a file you type as text, such as Markdown, plain text, a web page or a data or config file, whose name has the word token, tokens, secret, secrets, credential or credentials. Those words do not leave out a file that comes from an export or a publisher, such as a PDF, Word file, slide deck, spreadsheet, EPUB or image, so "Token economies.pdf" is searched. Search still checks every file's text for passwords and keys. So is a kind of file search does not read, such as a slide deck. A file that looks like an answer key is added, and the window never names it or opens it.

From a terminal, `magnesium searchable PATH...` says the same for the files you name, and `--json` prints it as data. It changes nothing, and it works in a folder that is not set up yet.

When the tutor cites a passage, the reply shows a link such as "Week 4 slides, p. 12". The link reads as plain text until you have answered that reply's task. After that it opens that page in a panel beside the lesson. Nothing leaves your Mac. The panel closes when the tutor sets the next task, or when you close it. The window opens PDFs, text and images from the study folder, and never an answer key.

## Long lessons

A long conversation can get compacted: Claude Code replaces its older part with a summary. The tutor's detailed instructions are lost with it. During a lesson, Magnesium then puts a short set of core teaching rules in front of the tutor itself: one idea and one task per reply, a guess before each new step, and the rules above about concrete cases, telling mistakes apart, finishing alone on a later day and ending a plan with a task. Claude Code gets them before the next reply.

To turn this off for a study folder, run `magnesium core-rules off` there. The tutor is then asked to reload its instructions after a compaction instead, as before. `magnesium core-rules on` turns it back on.

## Control recording and keep a copy

“Pause” or “stop learning” stops further lesson capture. Resume explicitly when ready. Already recorded work may still be consolidated while recording remains enabled.

“Turn recording off” blocks new Magnesium learner-text records. “Turn recording on” and resuming a lesson are separate actions.

Magnesium stores learner data locally, but your assistant's model receives selected memories and source excerpts. Exclude files from Magnesium's source access with `.magnesium/magnesiumignore`. Recording-off doesn't erase earlier Magnesium data, exports or provider chat history, or stop ordinary assistant responses.

From the study folder, run `magnesium export ../study-export` for a readable copy, or add `--format zip` for an archive. Sources are references by default; `--include-source-copies` adds verified in-scope files. Add `--overwrite` to replace an export that already exists at that path. Keep exports private.

To see Magnesium's status for a study folder, run `magnesium info` there. It prints the workspace, the instructions it serves, how many memories and unprocessed events there are, and the last hook error, if any. It records nothing. Add `--json` for one line of JSON.
