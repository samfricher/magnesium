---
name: help
description: Help for a learner in a folder set up with Magnesium (it has a .magnesium directory). It looks at the folder and suggests next steps, or shows a fixed help topic. Use for /magnesium:help, for questions about Magnesium itself, such as "how do I use Magnesium", "what can Magnesium do", "what commands does Magnesium have" or "help with Magnesium", and for "what should I do next with Magnesium". Ordinary coding or repository work does not start it. It never starts a lesson or writes learner records.
---

# Magnesium help

With no words after `/magnesium:help`, or a question such as "what should I do next", build the next-steps screen. Your first action is to run `magnesium digest`, which lists the study folder. Do not print Topic: overview in this case. Topic: overview is only a fallback for when the folder cannot be listed.

Otherwise, choose from the words after `/magnesium:help`:

| Words | Show |
|---|---|
| none, or "what should I do next" in any phrasing | the next-steps screen below |
| `papers`, or a close word such as paper, practice, exam, mock | Topic: papers |
| `recording`, or a close word such as privacy, pause, export, memory | Topic: recording |
| `setup`, or a close word such as install, init, upgrade | Topic: setup |
| `trouble`, or a close word such as problem, error, broken | Topic: trouble |
| `overview` | Topic: overview |

The topics are at the end of this file. To show a topic, print the text inside its fence word for word. Leave out the fence lines, and add no preface or summary.

When the learner asks a specific question instead, answer it in two to five plain sentences. Use facts from the topics first. If the topics do not answer it, Read `<base>/user-guide.md`, the full user guide, if that file exists. `<base>` is the path after "Base directory for this skill:", which Claude Code prints when it loads this skill. After the answer, name the `/magnesium:help <topic>` command that has more, if one does. When neither the topics nor the guide covers the question, say so in one sentence and suggest the learner ask the tutor directly.

Use only tools that read, and never cause a permission prompt:

- To read the study folder and the learner record, run `magnesium digest` exactly as written, once.
- To list a folder, use Glob if this session has it. Otherwise run `ls -A <folder>`, one folder per call.
- To find dates and assessment words in a text file, use Grep if this session has it. Otherwise run `grep -n -i -E '<words>' <file>`, one file per call.
- Read opens a file. For a PDF, give Read its `pages` parameter to read only the first pages.

Run each shell command alone. Never use `find`, `cd`, pipes, `;`, `&&`, loops or any other command, because those ask the learner for permission.

This skill only reads. Leave lessons, Magnesium sessions, Magnesium's tools and learner records as they are, and write no file.

## Next-steps screen

The learner wants one clear next step for this folder, and wants it fast. Many learners find long or changing text hard to follow. So the screen keeps one shape, says only what you saw, and ends with one action.

### Look, in about a minute

Run `magnesium digest` once. It changes nothing. It prints JSON facts that code has already worked out:

- `today`, and `files`: the study folder listed two levels deep, without hidden entries, `AGENTS.md` and `CLAUDE.md`.
- `plans`: each `Study/<target>/plan.md`, with its `date`, `days_left`, `blocks`, gap counts, paper `scores`, and the latest Magnesium `session` whose goal names the target. `open_gaps` counts the gap-log rows whose status is open, 1 of 3 or 2 of 3. `redos_due` counts those whose next redo is today or earlier. `first_due` is the most overdue one, with its `paper` and `question`.
- `record.sessions`: the latest Magnesium sessions, newest first. Each has a `goal`, the checkpoint's `current_problem` and `next_action`, and `days_ago`, which counts from the session's last change, not from a paper's date.
- `record.reviews`: `ready` is the count of review suggestions ready now, with their `ready_purposes`. A suggestion is an offer, not a task the learner must clear.

Use these facts as printed. Do not recount them or redo their date sums. If `magnesium digest` fails, leave out the record and look at the folder yourself. The study folder is the nearest folder, at or above the working directory, that holds `.magnesium/`. List it two levels deep: the folder itself, then each subfolder. Skip `.magnesium`, `.claude`, `.git`, other hidden entries, and `AGENTS.md` or `CLAUDE.md`, which setup can write.

Then look only for what the digest does not hold:

- Sort the listed files into one kind. **Empty** has none. **Course** has a syllabus or course outline, slides, lecture notes, a textbook, assignments, tutorials, quizzes, old tests or exams, or photos of work. **Code or other** is anything else, such as a software project.
- For a course, find the course name and the dated assessments in the syllabus or course outline, if there is one. Read only the first pages of a PDF, or Grep a text file for words such as midterm, test, exam, quiz and due. Keep the next one or two dates after today's date. Count the days to each one from `today` in the digest.
- Take chapter, week and lecture names from file names. Judge slides, notes and textbooks by their names alone.
- For a plan, the next block is in its session's `next_action` or `current_problem`, which name the current paper and the next block. Without a session, the next block is the first block whose paper has no score. When you name a score, give the date in `scores`. If `magnesium digest` failed, read the plan's Facts and Blocks sections and its `gap-log.md` instead.

If a file cannot be read, or a fact is unclear, leave it out. A short screen is better than a wrong one. If you cannot list the folder at all, show Topic: overview instead.

### Choose 2 to 4 actions

Put the most urgent first. When the record adds an action, drop the least urgent one, so the screen stays short. Base each one on what you saw:

- A plan with `redos_due` above 0: this action comes first. Name `first_due` by its paper and question, and give `redos_due` as printed. "Redo Diagnostic Q4 cold. It is one of 2 redos due today." Say "cold redo".
- A saved lesson in `record.sessions[0]` with a `next_action`, and no plan that uses it: "Last time you stopped at <next_action, as a short phrase about the learner>." Say "Continue my last lesson".
- A review suggestion ready: lead with one of `ready_purposes`, not the count. "A review is ready: <the purpose in a few words>." Say "Review <the same few words>".
- A plan with no `date`: name the plan, and say nothing about days left. Its next block follows the plan rule above.
- A plan whose date is ahead: the next block's words, such as "mark my Diagnostic", "cold redo" or "re-time today". If the next block is unclear, offer `/magnesium:papers`, which continues the plan.
- An assessment ahead with no plan: "I have <assessment> on <date>", or `/magnesium:papers`.
- Course files by chapter or week: "Teach me <a chapter or week you saw> from the slides", or "Quiz me on <it>. One question at a time." When the syllabus gives the next assessment's coverage, pick a chapter or week inside it.
- An empty folder: the learner may not know what the folder is for. Under In this folder, say that a study folder is for their course files and their learning history. Add a `## What Magnesium does` section after In this folder. Its three bullets say that Magnesium teaches from their files, remembers their answers and weak points between sessions, and builds practice papers and marks them. Then offer two actions. They can copy course files into the study folder, such as a syllabus, slides, notes, assignments, old tests and photos of work, then type `/magnesium:help` again. They can also say "Teach me" and name a topic.
- Code or other: "Help me understand <a file or folder you saw>", or "Quiz me on how <it> works".

End each action with the exact words to say in straight quotes, or the command to type in backticks.

### Show the screen

Your first text is the heading. Use this shape:

```markdown
# Magnesium help

Magnesium turns Claude into a tutor that remembers your work.

## In this folder

- <1 to 3 short facts you saw>

## What you can do now

1. **<Short verb phrase>.** <One short sentence, if needed.> Say "<exact words>".
2. <2 to 4 actions in all>

## More help

- `/magnesium:help papers` shows how practice papers work.
- `/magnesium:help recording` shows what Magnesium keeps and how to stop it.
- `/magnesium:help setup` shows how to install Magnesium.
- `/magnesium:help trouble` shows fixes for common problems.

Next: <action 1, with its words or command>.
```

Write the screen by these rules:

- Keep each sentence under 15 words where you can, and never over 20. Split a long list of files into two sentences.
- Give each sentence one idea. Use common words, and call the learner "you".
- Use one word for one thing: study folder, tutor, session, plan, gap.
- Copy names and dates exactly as found. Never invent a date, chapter, paper or file.
- Use straight quotes. Leave out long dashes, semicolons, markdown links, and e.g., i.e. or etc.
- Stay under about 180 words.
- Write nothing before the heading and nothing after the Next line.

### Examples

These folders are made up. The heading, the first sentence and More help are as in the shape above. `<N>` stands for a count of days that you work out from today's date.

#### Example: empty folder

The study folder holds only `.magnesium/`, `.claude/` and `AGENTS.md`.

```markdown
## In this folder

- This is a study folder. It is for your course files and your learning history.
- It has no course files yet.

## What Magnesium does

- **Teaches** you from the notes and files in this folder.
- **Remembers** your answers, strong points and weak points between sessions.
- **Builds practice papers** for a test and marks your answers.

## What you can do now

1. **Add your course files.** Copy them into this folder. A syllabus, slides, notes and old tests all help. Then type `/magnesium:help` again.
2. **Magnesium without files.** Say "Teach me" and name a topic.

Next: copy your course files into this folder, then type `/magnesium:help`.
```

#### Example: course with a test ahead

The folder holds `BIO110-syllabus.pdf`, `Slides/Week01.pdf` to `Slides/Week06.pdf` and `Old tests/Midterm-2024.pdf`. The syllabus lists "Midterm 1, October 23". There is no `Study/` folder.

```markdown
## In this folder

- BIO 110 files: the syllabus, slides for weeks 1 to 6, and one old midterm.
- The syllabus lists Midterm 1 on October 23. That is in <N> days.

## What you can do now

1. **Plan for Midterm 1.** The tutor writes a study plan and timed practice papers. Say "I have Midterm 1 on October 23", or type `/magnesium:papers`.
2. **Learn one week.** Say "Teach me week 6 from the slides".
3. **Check what you know.** Say "Quiz me on week 1. One question at a time."

Next: say "I have Midterm 1 on October 23".
```

#### Example: lesson history, no plan

The folder holds `BIO110-syllabus.pdf` and `Slides/Week01.pdf` to `Slides/Week05.pdf`. The syllabus lists "Midterm 1, October 23". The digest has no plans. Its newest session has the goal "Week 4: enzyme kinetics", `days_ago` 1, and the `next_action` "Ask the learner to read Km and Vmax off a Lineweaver-Burk plot". `record.reviews.ready` is 2, and one purpose is "Recall the meaning of Km without notes".

```markdown
## In this folder

- BIO 110 files: the syllabus and slides for weeks 1 to 5.
- The syllabus lists Midterm 1 on October 23. That is in <N> days.
- Yesterday you studied week 4, enzyme kinetics.

## What you can do now

1. **Pick up where you stopped.** Last time you stopped at reading Km and Vmax off a plot. Say "Continue my last lesson".
2. **Try a review.** A review is ready: the meaning of Km. Say "Review the meaning of Km".
3. **Plan for Midterm 1.** Say "I have Midterm 1 on October 23", or type `/magnesium:papers`.

Next: say "Continue my last lesson".
```

#### Example: plan in progress

The digest has a plan for `midterm-1` with the date 2026-10-23, a Diagnostic score of 18/30 dated 2026-10-15, 3 open gaps and 2 redos due. `first_due` is Diagnostic Q4. Its session's `next_action` is "Cold redo of Q4 and Q6, then sit Paper1-Easy".

```markdown
## In this folder

- Your plan for Midterm 1 is in `Study/midterm-1/`. The test is on 2026-10-23, in <N> days.
- The tutor marked your Diagnostic 18/30 on 2026-10-15. 3 gaps are still open.

## What you can do now

1. **Redo Diagnostic Q4 cold.** It is one of 2 redos due today. You answer from a blank page. Say "cold redo".
2. **Sit Paper1-Easy next.** Use a timer. Then send photos and say "mark my Paper1-Easy".
3. **Fit today to your time.** Say "re-time today" and give your free hours.

Next: redo Diagnostic Q4 cold. Say "cold redo".
```

## Topic: overview

````markdown
# About Magnesium

Magnesium turns Claude into a tutor that remembers your work.
Each study folder keeps its own learning history.

## What Magnesium does

- **Teaches** you from the notes and files in this folder.
- **Remembers** your answers, strong points and weak points between sessions.
- **Builds practice papers** for a test and marks your answers.

## How to start

Tell the tutor what you want to learn. For example:

- "Teach me confidence intervals from my notes."
- "Quiz me on chapter 3. One question at a time."
- "Review what we did last time."

The tutor starts a lesson only when you ask to learn, practise or review.

## Practice papers

Say "I have a midterm Monday" to start.
Type `/magnesium:help papers` to see the steps.

## Help topics

- `/magnesium:help papers` shows how practice papers work.
- `/magnesium:help recording` shows what Magnesium keeps and how to stop it.
- `/magnesium:help setup` shows how to install Magnesium.
- `/magnesium:help trouble` shows fixes for common problems.

Next: type `/magnesium:help papers`.
````

## Topic: papers

````markdown
# Practice papers

You sit timed practice papers.
The tutor marks them and helps you fix each gap.

## Steps

1. Tell the tutor about your test, for example "I have a midterm Monday". Or type `/magnesium:papers`.
2. Answer the tutor's questions.
3. The tutor writes your plan and papers. The first paper is the Diagnostic.
4. **Sit the Diagnostic** alone, with a timer. Write `?` where you would look something up.
5. **Send photos or a PDF** of your answers. Then say "mark my Diagnostic".
6. The tutor marks your answers. It writes each lost mark in the gap log.
7. **Repair** each gap with the tutor.
8. Later, do a **cold redo** of each gap with no help, on three different days. Three passes close it.

## What to say

| Say this | What happens |
|---|---|
| "mark my PAPER" | The tutor marks that paper. |
| "teach me TAG" | The tutor teaches that topic. |
| "repair TAG" | The tutor does a short repair on that topic. |
| "cold redo" | You redo the gaps that are due. |
| "new mock" | You get another paper at test level. |
| "re-time today" | The tutor fits today's work to the hours you have. |
| "rebuild" | The tutor starts the plan again. |

PAPER is a paper name, like Diagnostic. TAG is a topic name from your gap log.

## Your files

Your files are in `Study/<target>/`, like `Study/midterm-1/`.
Each new session continues from the plan in `plan.md`.

## Rules

- **Keep the KEY files closed** until you send your answers.
- The tutor asks before it adds a lot of time.

Next: tell the tutor about your test.
````

## Topic: recording

````markdown
# What Magnesium records

Magnesium keeps your learning history in `.magnesium/` inside your study folder.
This history stays on your computer.

## What Magnesium keeps

- Your answers and how you got to them.
- Your strong points and weak points.
- The help that the tutor gave you.
- Where you stopped, so that you can continue later.

## What Claude sees

Claude's model gets the memories and the parts of your files that a lesson needs.
Magnesium uses your usual Claude connection. Your usual limits and charges apply.

## How to control it

| Say this | What happens |
|---|---|
| "pause" or "stop learning" | The lesson stops, and Magnesium stops recording it. |
| "turn recording off" | Magnesium makes no new records of your words. |
| "turn recording on" | Magnesium records your words again. |

To continue a paused lesson, ask the tutor to continue it.
"Turn recording on" alone does not continue a lesson.

Turning recording off does not:
- delete old Magnesium records or exports.
- delete your Claude chat history.
- stop Claude from answering you.

## Hide files from Magnesium

Write one file or folder name per line in `.magnesium/magnesiumignore`.
Magnesium's file search skips those files.

## Keep a copy

1. Open a terminal in your study folder.
2. Run `magnesium export ../study-export`.
3. Add `--format zip` to get one zip file.

Keep your exports private.

Next: type `/magnesium:help papers` to start practice papers.
````

## Topic: setup

````markdown
# Set up Magnesium

## Install Magnesium

Give the Magnesium repository to your coding agent.
Say "Install Magnesium by following docs/install.md".
The agent asks before it installs anything beyond the build.

Keep the `magnesium` command on your PATH. Claude runs it by name.

## Set up a study folder

1. Open a terminal in your study folder.
2. Run `magnesium init`.
3. **Start a new Claude Code session** in that folder.

Magnesium works only in folders where you ran `magnesium init`.
Your study files stay where they are.
Subfolders share the learning history of the study folder.

## After you upgrade Magnesium

1. Run `magnesium init --upgrade-instructions` in each study folder.
2. **Start a new Claude Code session.**

Your learning history stays the same.

## For LaTeX papers

The tutor uses tectonic to build LaTeX papers.
The tutor asks before it installs tectonic.
Without tectonic, the tutor writes your papers in Markdown or HTML.

Next: type `/magnesium:help` to see what Magnesium does.
````

## Topic: trouble

````markdown
# Fix a problem

Find your problem below. Then do the steps under it.

## Claude asks permission for every Magnesium step

1. Run `magnesium init --upgrade-instructions` in this study folder.
2. Start a new Claude Code session.

## Claude says Magnesium's instructions are unavailable

1. Run `magnesium init --upgrade-instructions` in this study folder.
2. Start a new Claude Code session.

## Magnesium does nothing in a folder

1. Run `magnesium init --upgrade-instructions` in that folder.
2. Start a new Claude Code session in that folder.

## Claude cannot find the `magnesium` command

Ask your coding agent to put `magnesium` on your PATH. Step 4 of docs/install.md shows how.

## Magnesium stops working after `/clear`

Start a new Claude Code session.
During a lesson, start a new session instead of `/clear`.

## The paper build says "tectonic is not on PATH"

1. Ask the tutor to install tectonic. It asks before it installs anything.
2. Or ask the tutor for Markdown or HTML papers.

## A memory about you is wrong

Tell the tutor what really happened. For example:
"That was not a gap. I misread the question. Please correct the memory."

The tutor reads your original work and corrects the memory.
It keeps the old history.

Next: type `/magnesium:help` to see all help topics.
````
