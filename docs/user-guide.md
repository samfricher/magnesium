# Using Magnesium

Magnesium helps your usual assistant teach understanding you can use independently. It draws on your materials, current work, and prior learning to choose what to explain, practise, and revisit. Workspace-local memory preserves descriptions of your understanding and difficulties, backed by your original attempts, so the tutor can build on them across conversations.

## Set up a study folder

[The install guide](install.md) tells your coding agent how to build Magnesium and set up a study folder. To set up another, run `magnesium init /path/to/your/course`, or run `magnesium init` from that folder. Initialization creates private `.magnesium` state and installs the packaged teaching instructions; in an existing Git repository, it adds `.magnesium` to local exclusions. Your study materials stay in place. Subfolders share its memory unless you initialize a separate workspace inside one. Incompatible learner data is preserved and rejected without migration.

The study window calls a study folder a **space**. Home lists your spaces, and each space has its own page with its sessions, your work, its papers and its material.

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
  - Start a new Claude Code session in the folder afterwards, and type `/magnesium:learn` there to turn Magnesium on. Keep `magnesium` on `PATH`.
- **After upgrading Magnesium:** run `magnesium init --upgrade-instructions` once in each study folder, then start a new session. It does everything `magnesium init` does, including the plugin refresh and permission rules above. It also switches the folder to the new teaching instructions. Until you switch it, a folder keeps the instructions it was set up with, and a newer `magnesium` serves only its own. So the tutor reports Magnesium's instructions as unavailable, and plain `magnesium init` stops with an error there. The switch happens only if the old instructions were not edited. It keeps the old files and leaves learner history untouched.
- **Claude Code reply length:** during a lesson, most of your messages reach the tutor with a reminder to teach one idea in under 200 words and to end with one task. If you are confused, the reminder also tells the tutor to ask you one question to find out why before explaining. The reminder can also say that the tutor's previous reply had 200 words or more. The tutor still writes more for a worked example, a full explanation you asked for, a marked paper or a plan. It skips the task when you ask for answers only or no quizzes, or say you are done for now.
- **Claude Code `/clear`:** `/clear` starts Magnesium off again, as a new session does. Magnesium's tool connection also keeps the conversation it started with, so after `/clear` Magnesium refuses to read or record learner state until you start a new Claude Code session and type `/magnesium:learn`. Start a new session instead of using `/clear` during a lesson.
- **Claude desktop pastes:** the desktop app sends pasted text between hidden `<pasted_content id="…">` and `</pasted_content id="…">` lines. Magnesium records a Claude message without those tag lines, blank lines before your first line, or whitespace after your last, so a pasted message keeps your exact words, including indentation, whether the hook records it or the tutor echoes it to start or resume a lesson. A message that reaches Magnesium without Claude Code's hooks is recorded exactly as sent.

## Start with a request

In Claude Code, `/magnesium:help` looks at the folder and your learner record and suggests what to do next, and `/magnesium:help papers` shows how practice papers work. It reads both through `magnesium digest`, which prints the facts it uses as JSON and changes nothing; you can run it yourself in the study folder.

Each Claude Code session starts with Magnesium off, so ordinary project work in the folder never starts a lesson. To start one, type `/magnesium:learn` and then your request:

- `/magnesium:learn Help me understand recursion using this repository.`
- `/magnesium:learn Practise conditional probability with me. One question at a time.`
- `/magnesium:learn Review what we covered last time, then pick up where we stopped.`
- `/magnesium:learn Explain the solution directly. No quiz today.`

`/magnesium:learn` alone goes on with your last lesson. After that, talk to the tutor as usual: Magnesium stays on until the session ends. [What Magnesium keeps](#what-magnesium-keeps) says what that means for your record.

Give the tutor your attempts, questions and constraints. It can use authorized local files and preserve a checkpoint. You don't need to maintain memories yourself.

If a new kind of problem leaves you without a way to start, the tutor works through a similar problem, then gives you a changed version to solve yourself. If that kind of problem stops you again, the tutor leaves some steps for you to fill in. After that, you get the problem alone. If you can't finish the practice problem yourself, or forget the method later, the tutor goes back a step and gives you more help next time. Magnesium keeps count of this for each kind of problem from your marked answers, so the help fades even when you come back days later in a new conversation. If you say you are confused, or ask a question because you are lost, it first asks one question, a small concrete case of the idea in plain words, without code or symbols, that takes under a minute, and uses your answer to choose what to explain. A plain factual question about something already covered, such as what a name stands for, gets a one-line answer and no question back.

When you start a topic you have not answered anything on before, the tutor asks up to three short opening questions, one at a time: an easy one, one from the middle of the way to your goal, and the hardest step the goal needs. Each is a concrete case you can answer in about a minute, not a definition to recite. It explains nothing between them. At the first one you miss, it stops and starts teaching there. It says in one line what you got right and builds on it, without teaching it again. If you get all three right, it aims higher. It skips the opening questions when you have answered questions on the topic before, when you ask a plain factual question, and when you say where to start. Names and symbols come after you have done a step. Every answer you give to a question the tutor set is marked, including "I don't know". Each task the tutor sets says again every number, name and condition you need to answer it, so you can answer it with only the task in front of you.

If you ask for a hint, the tutor gives the smallest help that lets you take the next step: a question or a pointer, not the step itself. A skill you get right only with a hint comes back the next day as a quick check, to answer alone, and keeps the passes it already has. So does a question you answer "I don't know". Declining a quick check that is due costs nothing. On a quick check or a redo that is due, the tutor tells you before it gives a hint that an answer after a hint does not count as a pass.

Before it explains a step you have not seen, the tutor asks what you think comes next and why. A rough guess is fine, and a wrong guess is never recorded against you. The explanation then starts from what your guess got right and wrong, and ends with a check question. It skips the guess when you asked for an explanation or a full solution, when you say you are confused, when it shows you a worked example because you had no way to start, when it is repairing a missed check, and right after it asked you another question. A check question doesn't count: after you pass a check, the tutor still asks for your guess on the next step.

Before it explains something new, the tutor names a skill you have already shown in your own work and builds from it. These skills are called anchors. If you have no way to start a new kind of problem, it shows the worked example first, built on an anchor if you have one that fits. If you know how to start, you try first. Otherwise, if no anchor fits, it first asks what you already know that looks like the new idea. It skips that question when it has just asked you one, such as how you see a problem that confuses you, when you asked for an explanation or a full solution, and when it is rewriting a task that stopped you twice. When you then answer a check question correctly on your own, with no hint or solution from the tutor and without copying a solution it just showed you, it adds the approach that worked to your anchors. It keeps them in your learner record, and a practice plan keeps a copy of the ones it uses in its Anchors section. If the two differ, the learner record is right, and the tutor updates the plan whenever it adds, corrects or retires an anchor.

When the tutor draws a figure or a page as a file, it renders it to an image and looks at it before you see it. It fixes overlapping or cut-off parts, unreadable labels, decoration with no label, and anything the picture gets wrong, then looks again. This needs a browser that can run headless, such as Chrome, Chromium or Edge, or an SVG-to-PNG converter such as `rsvg-convert`. Without either, the tutor says it cannot check the figure and uses text or a Mermaid diagram instead.

An answer counts only when it rules out the likely mistake. If a wrong idea could have given the same answer, the tutor asks one more question that tells the two apart. A step in your plan counts as done only after you answer it on your own on a later day. If you ask for a model answer, you get it, and the tutor asks you for it cold the next day. When you ask for a study plan, the reply ends with one task you can answer in the chat in about two minutes, not a request to go and read the plan.

If a memory is wrong, say so: “That wasn't a conceptual gap; I misread the question. Please correct the memory.” The tutor can inspect your original work and revise its interpretation while keeping the history.

## Practise with timed papers

In Claude Code, type `/magnesium:papers` and then what you are studying for: `/magnesium:papers I have a midterm Monday` or `/magnesium:papers Build me practice papers for chapters 6 and 7`. `/magnesium:papers` alone continues the plan you have. The tutor asks only for what your message and folder do not show. It then writes a plan, papers with separate keys, drills and a gap log in `Study/<target>/`, for example `Study/midterm-1/`. The tutor also keeps a map of the course's skills in `graph.yaml` and uses it to place you and to choose repairs. It asks when and where you will practise, suggests a time, and lets you skip the question. It writes your answer in the plan as if-then lines, with one backup for a missed session, for example “If it is Monday to Friday at 08:00, then I do the due cold redos at my desk before class.” On a day with a planned session, `/magnesium:help` names the session and how many redos are due. LaTeX papers build with `tectonic`. The build stops at a line that would run off the page, and the tutor rewrites that line before you get the paper. The tutor asks before it installs tectonic; without it, the tutor writes the papers in Markdown or HTML.

Sit each paper on your own, timed, and write `?` where you would look something up. After each answer, write how sure you are: S if you are sure, U if you are unsure or G if you guessed. Send photos or a PDF of your answers and say “mark my Diagnostic”. The tutor marks against the key and repairs each lost mark. Magnesium writes each part that lost marks as one row of the gap log and dates its cold redos, so a redo date is never missing or mistyped. The marking shows how many answers you got right and wrong for each letter. The tutor repairs the mistakes you were sure of first, because you would not think to review them. Your letters never count as a pass. Only your marked answers do. A gap counts as fixed only after you pass it cold on three different days, at least 1, 3 and 7 days apart, or closer together when the assessment is sooner. Every mock has at least one question of each of three shapes that recognising the problem type cannot pass: say why a given result holds, tell two look-alike problems apart, and say what breaks when one assumption is dropped. A question that gives you the result earns full marks only for a correct reason. Cold redos use these shapes for a gap in understanding. In a cold redo, a reason question always comes second: you first produce the result from a blank page, and the redo passes only if both parts are right. The tutor keeps the plan file current and asks before it adds a lot of time. A later session continues from the plan when you type `/magnesium:papers`. Say “rebuild” to start over.

In the study window, a paper sat from the space's page shows its time left in words, such as "53 minutes left", and beside them a disc whose shaded part is the share of the paper's time still left, like a Time Timer. Both change once a minute. Past the end the disc is empty and the words say "Time is up".

## Start each sitting with what you got wrong

A sitting is one stretch of study. It begins with your first message after about three hours without one. A compaction does not begin one.

When you get a task wrong on your first try, the tutor gives the mistake a short label, such as "sign of b in the quadratic formula". Magnesium then schedules a quick check on that label for the next day. The next sitting opens with what is due:

- **Quick checks:** up to three, about a minute each. Each sets a new problem to answer in a line or two, without saying which step it checks, so you have to use the step on your own. Then it shows lettered options, and the wrong ones are the mistakes you made before. A right answer with no help moves the check on to 3 days later, then 7 days later, and after three passes on three different days it is done. A wrong answer is repaired, and the check comes back the next day. So does an answer you reached only after the tutor showed you the full solution, or a full solution you asked for before trying.
- **Paper redos:** the tutor says how many gap-log redos are due and which comes first. They run as the first block of your paper session.

When the tutor stores the next question for a check or a redo, the sitting starts with that question, and the tutor keeps its answer back until you have replied.

You can decline any of it. A decline costs nothing: it is recorded as your choice, and the check stays due for the next sitting. "Skip", "no quiz" and "I'm done for now" still win. Each sitting adds one line to `.magnesium/sittings.log`: what was due, what was posed, answered or declined, what passed or failed, and how many more checks were due than the sitting had room for.

**Just the checks.** To take only the quick checks that are due, with nothing new after them, type `/magnesium:checks` in Claude Code, or say "just the checks" in a session where Magnesium is already on. In the study window, choose Just the quick checks beside Start, under the step to study on Home or on the space's page, or File > Just the Quick Checks (⌘⇧N). It is there only while a check is due, and not while a timed paper runs. The sitting poses up to three, one at a time, with no paper redo, no new material and no other task. After each answer the tutor shows the options and says which is right and why; a check you got wrong comes back on its next day. When the last one is answered or declined, Magnesium ends the sitting itself, and its closing lines show under that reply, as in [End a sitting](#end-a-sitting), or nothing when no answer was marked. No task follows. A paper redo that is due stays on the space's page and opens your next sitting. If you ask for something else part-way, a lesson, a change to the plan or a paper, you get it: the sitting becomes an ordinary one, and the checks not yet answered stay due. For a plan or a paper, the tutor asks you to type `/magnesium:papers` first. If you ask for more afterwards, the lesson goes on as usual. A lesson in another tab of the same folder is not affected.

**The question on Home.** The study window's Home starts with Today and the date. Under Today is the step to study next, in the space you studied last: the plan's next item, with a line under it saying what the session starts with ("Starts with 1 quick check, about a minute.") or what is due, and the skill you last answered on ("Last time: margin of error."), taken from your learning history. When the next message in the space you studied last begins a sitting that opens with a quick check, and the record holds that check's next question, Home shows the question under Today instead: "A quick check, about a minute." over it, then the question, then a line to answer on, ending in Check ↩. Its key is never shown. Type your answer and press Return: a sitting of just the checks starts, and your words go as your answer to that check as soon as the tutor has posed it, so it is marked as any answer to it is. They go only to that check, posed with the question Home showed. If the tutor poses another check, or the same one with a new problem, or you send something else or stop the reply first, your words wait on the session's line instead, unsent; when that is because the check posed could not be matched to the one Home showed, a quiet line in the lesson, under the tutor's reply, says your answer was not sent. Return on the empty line starts the same sitting without an answer. Under the question, Then names the plan's next item, which you can start instead. Each space under Spaces ends with one thing about its day: the quick checks due there (left out for the space of the step above, whose line already says them), else its exam when it is two weeks off or less ("Midterm 2 in 10 days"), else how many of its skills can count today, else the day you last studied it.

To turn this off for a study folder, run `magnesium opener off` there. Checks are still scheduled, but sittings no longer open with them. `magnesium opener on` turns it back on. `magnesium opener block` also makes the tutor wait to teach new material until each quick check has been asked or declined.

## End a sitting

When you are stopping, say so in your own words: "that's enough for today", "I have to go". The tutor tells Magnesium, closes in a line and sets no task. Under its reply you then see at most two lines, worked out from your learning history and never from the tutor's words: the passes you earned today, and the first day a skill you answered today can count again. For example:

```text
Earned today: a pass on slope and Km and 1 more.
Can count from tomorrow: units of a rate.
```

Each line is kept short enough to fit on one line: when there are more skills than fit, the rest are counted ("and 2 more"), and a name too long for the line is cut at a word and ends in "…". A skill proved today leads the first line ("Proven today: rate equation."). A wrong or partly right answer earns no pass, so it is never named as earned, but its skill can be named in the second line. If no answer was marked today, nothing is shown: a declined question is your choice, not an answer. In Claude Code in a terminal the lines come from Claude Code itself, after the reply. In the study window they sit under the reply, and stay there when you open the lesson again. Closing the window or the terminal without saying so shows nothing. If you write again after stopping, the sitting carries on with the same lesson, and stopping again shows the lines again. Asking the tutor to stop quizzing you, or to just tell you the answer, is not the same: the tutor does as you ask, and the lesson goes on. After `/clear` in Claude Code nothing is shown until you start a new session, as Magnesium records nothing there. The space's page shows when each of your skills can count again.

On the space's page, Your work lists the skills you have answered on by the day each can next count: Today, then To repair for a lost mark on a paper that Magnesium says waits for its repair (it can count again once a lesson repairs it, on a later day), then Tomorrow and each later day ("Fri 20 Nov"), then No day yet for a skill with no date for another reason, then Proven. Each day is named once, in the margin beside its first skill, and each skill has its three rings. The skills to repair are one list of names, without rings: past about three lines it ends "and 9 more", which shows the rest. The plan's exam, while it is ahead, has a row of its own on its date. When the exam is today or tomorrow, To repair comes after it, since a skill repaired now cannot count again before then. The newest mark on each skill is not repeated there: it stays in the lesson and in `magnesium attempts`. The closing lines show what you earned and what can count next.

From the study folder, `magnesium closing` prints the lines for today as they would show if the sitting ended now, and `--json` the same as data with when the current sitting ended. The command changes nothing in the folder.

## Your gap log

Magnesium keeps every gap-log row in your learning history and writes the Gaps table of `gap-log.md` from it after each change. It works out each row's passes, status and next redo date itself, with the rule above, and makes the gaps shorter to fit before the date in your plan's When row. If a row is wrong, tell the tutor: it corrects the mark or the row, and the table is written again. If you edit the table by hand, nothing about what is due changes; the next time Magnesium writes the table, it saves your edited file in `.magnesium/backups/gap-logs/` and the tutor asks what you meant.

The first time the tutor loads a paper stage for a plan, Magnesium brings in that plan's gap log with its rows, passes and dates, from any earlier template, and keeps the original in `.magnesium/backups/gap-logs/`. A row it cannot read stops that: `magnesium digest` lists it, and typing `/magnesium:papers fix my plan files` has the tutor correct it.

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

In the study window, only what you write in the open task is sent as an answer. Anything else you write is talk: a question, a change to the plan, something off the plan. The window says so at the start of the message, and the runtime refuses to mark a message sent that way, so talk is never counted, whichever engine runs the lesson.

In the study window, a lesson's steps are set into it where each began. When a reply moves the lesson on to the next step of its route, the step's name sits just above that reply, in italic, with its number in the margin, so a long lesson reads like a chapter with numbered heads. The window learns of the new step only once the reply has ended, so the head never pushes the reply you are reading: it appears when you start writing or send your next message, or at once if you have already scrolled past the start of that reply. Once you scroll past the lesson's heading, the top of the window says where you are, such as "2 of 3 A 95% interval". The heads stay where they were when you open the session again. A step is numbered by the route as it was when the step began; if the tutor changes the route later, earlier heads keep their numbers. Going back to an earlier step sets its head in again where you went back. A session from before this feature has no heads.

While a task is open and the tutor is not writing, the study window lets you answer it from the task alone. A reply is shown whole, where it was written, so you can read it. Once you start writing on the line, the lesson folds away. The page then shows the session's name, one quiet line, "⌘↑ Show the lesson", the task's own words, and your line under them. To see the whole lesson again, press ⌘↑, click that line, or choose Session > Show the Lesson. To fold it again, press ⌘↑ again, click "Hide the lesson", or choose Session > Hide the Lesson. Looking back is free: the window sends nothing about it, and nothing about it is recorded. A session you open again with a task open starts with the task alone. The line still takes anything: an answer, Stuck's choices, a question, or words sent without a mark (⌥↩). While the task is alone, a document open beside the lesson is closed, and its place says how to have it back. A past paper and the plan stay open. Picking a document in the panel opens it, and the lesson with it. You can give ⌘↑ another key in Settings, under Keys.

In Settings, under Your line, you choose how the line you write on is drawn. Bare line hangs an ink "›" in the margin before your words and nothing else. Ruled line is the bare line on one thin rule, from the "›" to the end of the column, with Stuck and More sitting on it. Soft field draws the line as a shaded row. A new window starts on the soft field. The choice changes only how the line looks: what you write is sent and recorded the same way.

## See the lesson you are in

From the study folder, `magnesium lesson` prints the lesson you were last in: its goal, its steps, whether learning is on, and the task open now, in the tutor's words, for example:

```text
Factorising quadratics
    active, learning on. Last changed 2026-11-15 16:40.
  [x] Find two numbers that multiply to c
  [>] Write the factors
Open task, posed 2026-11-15 16:40:
Factorise x² + 7x + 12.
```

A task is open from the reply that set it until an answer to it is marked. A quick check counts as open once a reply poses it. Add `--json` for the same as data, and `--conversation <id>` for the lesson of one Claude Code conversation. The study window reads it there after each turn. The command changes nothing in the folder.

## See how far each skill is proved

`magnesium progress` adds those answers up. It lists each skill with its passes, from none to three. A pass is one answer you gave right, alone and cold, on its own day, and three passes prove a skill. A skill you got wrong earns its passes by the redos the tutor schedules. A skill you never got wrong earns one for each day you answer it right. The command also prints a level from 1 to 10 for each plan, which is the share of the plan's passes you have earned, and the days on which an answer of yours was marked. A plan's level counts only the skills in its course graph, so it never falls because you start something new. It counts each skill at the most passes it has held, so it never falls when you miss either. The dots still show the passes you hold now: a miss after a pass starts that skill's passes again. A chat answer on a skill the graph names is recorded under the graph's name, so chat lessons on the course move the level too. Skills in no plan are listed with their passes and counted as proven or in progress, with no level. Beside each skill not yet proven it says when its next pass can count: "due" when a redo or check asks for it today, else the first day it can. A pass needs a later day than your last answer on that skill, so an answer later the same day never counts, and a skill you got wrong also waits for its next redo or check. For example:

```text
midterm-1: level 5 of 10 · 4 of 9 passes · 1 of 3 skills proven
  ●○○  slope and Km · due
  ●●●  rate equation · proven
  ○○○  Hill coefficient · waits for its repair
In no plan: 0 proven, 1 in progress
  ●○○  units of a rate · next today
Days with a marked answer: 2 in all. The latest: 2026-11-08, 2026-11-09
```

Add `--json` for the same facts as data. The study window reads them from there. The data also counts what you did today: the answers you gave, the skills you answered for the first time, the passes you earned and the checks due tomorrow. Under `first_check` it gives the question of the quick check a sitting begun now would pose first, when the record stores one, without its key. The command changes nothing in the folder.

When an answer earns a pass, Magnesium shows one more line from the record: the last time you got that skill wrong, with its day and label, and how many days the answer came after your last answer on the skill. The study window shows it under the mark. Claude Code in a terminal shows it once the tutor has recorded the answer. For example:

```text
Not right on Fri 13 Nov: sign of b. It held through the 3 days since you last answered it.
```

A skill you never got wrong gets only the days, and your first answer on a skill gets no line. A lost mark on a paper says the paper: "Lost marks on Midterm 2023, Mon 2 Nov: slope and Km." The `--json` data has these lines under `pass_receipts`.

In the study window, a mark that earns a pass also shows its skill's three days in the margin to the left of the mark, or at the start of the mark's line when the margin has no room for them (a narrow window, a large text size, the plan open beside the lesson): the days earned filled, in the same rings as a space's Your work. The day just earned fills as the mark arrives, unless the Mac is set to reduce motion. A wrong, partly right, helped or declined answer shows its words alone. A screen reader reads each mark once, as it arrives.

The first time this version opens a study folder from an earlier version, it upgrades the learning history once. It first copies it to `.magnesium/backups/`. Quick checks kept in `.magnesium/sittings.sqlite` by an earlier version move into the learning history then too. So do the help steps and tag readings the tutor wrote into your learner record, such as "example step (discrete expected value): partly worked" or "sound from MockA Q3". To go back to the earlier version, put that copy in place of `.magnesium/state.sqlite`.

## Add material, and read a source beside the lesson

In the study window, drop files on a space or a lesson, or choose them with the paperclip or the button on the space's page. You can also paste text. The window copies each file into `Study/materials/` in the study folder, under its own name. Pasted text is saved there as a `.txt` file. The tutor's source search reads that folder like any other. A folder you make in the window offers this as its first step.

After it adds, the window says in a few plain lines what it added and what search cannot read. A PDF with no text, such as a scan, is listed as one whose text cannot be searched. A file whose name has the word key or keys is left out of search, in case it is an answer key or holds a password, and the window names the word. So is a file you type as text, such as Markdown, plain text, a web page or a data or config file, whose name has the word token, tokens, secret, secrets, credential or credentials. Those words do not leave out a file that comes from an export or a publisher, such as a PDF, Word file, slide deck, spreadsheet, EPUB or image, so "Token economies.pdf" is searched. Search still checks every file's text for passwords and keys. So is a kind of file search does not read, such as a slide deck. A file that looks like an answer key is added, and the window never names it or opens it.

From a terminal, `magnesium searchable PATH...` says the same for the files you name, and `--json` prints it as data. It changes nothing, and it works in a folder that is not set up yet.

When the tutor cites a passage, the reply shows a link such as "Week 4 slides, p. 12". The link reads as plain text until you have answered that reply's task. After that it opens that page in a panel beside the lesson. Nothing leaves your Mac. The panel closes when the tutor sets the next task, or when you close it. The window opens PDFs, text and images from the study folder, and never an answer key.

## Choose the model a lesson runs on

In the study window, a session's line is the same before a lesson and in it. It ends in two words, Stuck and More. Stuck offers Hint, Help me and I don't know, once the lesson has begun. More offers Attach a photo or PDF, Symbols and Model, and, while you are typing an answer, Send without a mark (⌥↩). The Model row names the model the session runs on, or starts on. Press More and then Model, or choose Model and Effort… in the Session menu (⌘⇧J), to see the models Claude Code offers on your account, each with Claude Code's own line about it. Current models come first; older ones are behind "Older models". Under the chosen model you can set its effort: Usual leaves it to Claude Code, and the other levels are the ones that model reports.

You can change the model or the effort in the middle of a lesson. The change goes to Claude Code at once and holds from the tutor's next reply, in the same conversation. The line then names what Claude Code reports the session runs on; if Claude Code turns a change down, the list says why and the session stays as it was. A change also becomes your pick for new sessions. A session you carry on later keeps the model and effort it last ran on.

The window asks Claude Code for the list each time it opens, without starting a lesson and without sending a message, so the list is there before your first lesson and a new model appears the next time you open the window. If the list did not come, opening it asks again. Return never waits for it. Nothing changes the model by itself: your pick stays when Claude Code's default changes, and if the list no longer offers it, More's Model row says so before you start. If a new lesson never opens your record in its first reply, the lesson says so and offers to start again. When you picked an effort, it names the effort and starts again at Usual. Otherwise it names the model, and starts again on the same one.

## Long lessons

A long conversation can get compacted: Claude Code replaces its older part with a summary. The tutor's detailed instructions are lost with it. During a lesson, Magnesium then puts a short set of core teaching rules in front of the tutor itself: one idea and one task per reply, a guess before each new step, and the rules above about concrete cases, telling mistakes apart, finishing alone on a later day and ending a plan with a task. Claude Code gets them before the next reply.

To turn this off for a study folder, run `magnesium core-rules off` there. The tutor is then asked to reload its instructions after a compaction instead, as before. `magnesium core-rules on` turns it back on.

## What Magnesium keeps

Each Claude Code session in a study folder starts with Magnesium off. In a session that is off, Magnesium does nothing: it keeps nothing, it does not index your files, and its tools only answer that it is off. Claude cannot turn it on by itself. You turn it on by typing `/magnesium:learn`, `/magnesium:papers` or `/magnesium:checks`, alone or followed by your request, such as `/magnesium:learn teach me week 3`. `/magnesium:help` only reads, and does not turn it on. Once on, a session stays on until it ends, through a resume or a compaction. `/clear` or a new session starts off again. The study window starts each of its sessions on, so there is nothing to type there.

In a session that is on, everything you type in a lesson is kept in `.magnesium/` in the study folder, on your computer, from the message that turned it on until the session ends. There is no pause and no switch to stop the record. To talk to Claude off the record, start a new session and do not type a Magnesium command in it. So Magnesium can sit in a folder you also use for other work, and leave those sessions alone. Claude Code keeps its own chat history either way.

Magnesium stores learner data locally, but your assistant's model receives selected memories and source excerpts. To leave files out of Magnesium's file search, write one file or folder name per line in `.magnesium/magnesiumignore`. Files that look like they hold passwords or keys are left out of the search index too.

From the study folder, run `magnesium export ../study-export` for a readable copy, or add `--format zip` for an archive. Sources are references by default; `--include-source-copies` adds verified in-scope files. Add `--overwrite` to replace an export that already exists at that path. Keep exports private.

To see Magnesium's status for a study folder, run `magnesium info` there. It prints the workspace, the instructions it serves, how many memories and unprocessed events there are, and the last hook error, if any. It records nothing. Add `--json` for one line of JSON.
