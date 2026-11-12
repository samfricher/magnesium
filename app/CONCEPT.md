# Concept A: the study folder is a workspace, and the window is its editor

Slug: `workspace-editor`

## The concept in one paragraph

You open the app and it is exactly where you left it: the same study folder, the same open documents, the same place on the page, your half-written answer still under the question. There is no home screen. A study folder is a workspace of documents, opened the way Sublime Text or Obsidian opens files, in tabs and in up to two panes, and found with ⌘P. There are four kinds: your **material** (the slides, notes and papers in the folder), the **plan** (`Study/<target>/plan.md`, the file itself), **lessons**, and the **record** of your marked answers. A lesson is a page that you and the tutor write together: the tutor's explanation, then a task, and under the task the place where your answer goes, with the caret already in it. Return hands it in. Anything you write anywhere else on the page is talk: press Esc to step out of the task and write on the page itself, and what you say there is never marked, whatever it says. The open task stays where it was, pinned to the top of the pane while you are away from it, and Tab takes you back into it. When you say "I skipped three days and the midterm moved, change the plan", the tutor edits the plan file, and the plan opens in the pane beside the lesson with the changed lines marked in the gutter, as an editor marks changed lines, and the old values struck through above them. When the record marks an answer, the mark appears in the gutter beside your answer, from the record, in four words or fewer. Your past work is documents too: earlier lessons, the record (every marked answer, by day), your handed-in photos. The window never shows a level, a proof dot, a count of what is due, a run of days, a countdown to an exam, or a congratulation. Its own chrome is a row of tab names, one quiet status line, and nothing else until you ask.

## Why it meets each feeling

**Mine.** The window shows your files, under their own names, and nothing that belongs to the app: no home screen, no dashboard, no app vocabulary on the page. Font, text size, line width, scheme and ink colour are yours (⌘,), and settings are a document you open in a tab, the way Sublime opens its settings. It reopens exactly as you left it, per folder: which documents were open, in which pane, the scroll position of each, and the draft in each writing place, kept on this Mac. Everything is reached by keys: ⌘P for any document, ⌘K for any command, ⌘O for another folder. It never asks you anything: closing a lesson's tab saves where you stopped by itself, from the open task, and a paper starts with the length your plan gives it.

**Calm.** On opening there is one page of your own work, a row of tab names at the top, and one dim status line at the foot. No card, no number, no button row. The tutor's writing state is one line on the page where its words will appear; marks and plan changes live in the gutter, outside the text. The folder list, the file list and the commands exist but stay out of sight until a key brings them.

**Easy to start.** The next action is already set up: the caret blinks where you stopped, and the page says "Return to continue". One keystroke and the tutor picks up. In a new folder or a new lesson (⌘N) the page asks one question, what to work on, with the plan's next item already filled in, so Return alone starts it. Nothing on screen says what you owe. Progress is the record: a plain log of your answers, by day, each with its mark and the help you had, which you open when you want it.

**Flexible.** Talk is a place, not a label. Inside the open task you are answering; anywhere else you are talking, and talk is sent as `talk`, which the engine never marks. So you can change the plan, reorder it, or ask about something else at any moment, without first saying that you are doing so, and the open task waits pinned at the top. When the plan changes, the plan document is what changes, in place, and it opens beside the lesson so you see it change. A plan item you did not do is just a line in a file; nothing is red, late or "behind", and no date is turned into a countdown.

## How talk and answers are told apart

- **An answer is written inside the open task.** When a reply sets a task, the task becomes a block on the page with a writing place under it, and the caret moves there. Return sends it as `answer`. The Hint, I'm lost and I don't know words, the confidence keys, photos of working and the symbol picker all belong to that block.
- **Talk is written on the page.** Esc leaves the task: the caret drops to the line at the end of the page, outside every task, and what you send from there is `talk`. The same holds for every other place you can write: a new lesson's first line, the plan's own line, a reply with no task open.
- **The task waits.** While you talk, the open task stays where it was and pins to the top of the pane once it scrolls away. Tab (or ⌘↑) puts the caret back in it, with your draft. When the tutor sets a new task, the old one closes and your unsent draft moves into the new one.
- **A mark comes only from the record.** The gutter beside an answer stays empty until the `marked` event (or the record's new row) arrives. The tutor's words are never read for a mark.

The window knows a task is open from the reply: a paragraph that opens with a bold Task, Question, Check, Quick check or Paper redo, or, failing that, a final paragraph that asks a question. Answer intent is the tutor's ordinary judgement (as today); talk intent is the guarantee. So the cost of a reply misread as a task is an answer the tutor judges as it always did, never a mark on talk.

## Design plan

### Colour (two schemes; the learner picks the ink)

Dark, "slate":

| name | hex | use |
|---|---|---|
| ground | `#1D2023` | page, title row and status line, one surface |
| shelf | `#181A1D` | the folder sidebar and the ⌘P / ⌘K sheet |
| text | `#DAD8D3` | the documents |
| quiet | `#8B8F95` | tab names, status line, gutter notes, placeholders |
| rule | `#2D3135` | the few lines there are |
| ink | `#8DB3E2` | your own writing, the caret, focus, the open task's edge |
| change | `#D2B062` | changed lines in the gutter |

Light, "paper":

| name | hex | use |
|---|---|---|
| ground | `#FAFAF8` | |
| shelf | `#F1F1EE` | |
| text | `#1F2226` | |
| quiet | `#666B71` | |
| rule | `#E0E0DC` | |
| ink | `#2C5E9E` | blue-black, like a fountain pen |
| change | `#A67A12` | |

Ink choices: blue-black (default), green-black, violet, graphite (no colour at all). Marks are drawn in `quiet` with a shape, never in red or green: the record is information, not a grade.

### Type

Only fonts every Mac has, plus the one already bundled; the window makes no request for a font.

- **Documents**: Charter by default (sturdy at 17 px, made for screens, sits well beside KaTeX's maths). The learner can pick New York, Iowan Old Style, the system sans (SF Pro), Atkinson Hyperlegible (bundled, for readers who want it) or SF Mono.
- **Chrome** (tabs, status line, sheets, sidebar): the system sans, `-apple-system`, so it reads as part of macOS rather than as the app's voice.
- **Plan in source mode** and code: SF Mono (`ui-monospace`).

Scale, from the document size `s` (17 px by default, 14 to 22 with ⌘= and ⌘−), ratio 1.2: title `1.5s` (26), heading `1.2s` (20), body `s` (17) at line height 1.6, small `0.8s` (13.5) for gutter notes. Chrome is fixed: 13 px for sheets and sidebar, 12 px for tabs and the status line. Measure: 66 characters by default; the learner can pick 58, 66 or 80.

### Layout

One window: a 38 px title row holding the traffic lights and the tab names; under it one pane, or two side by side (⌘\); a 24 px status line. The sidebar (⌘B) is the folder's files, hidden until asked for. Each pane centres a column at the chosen measure, with a 40 px gutter on its left for marks and change bars. Text is left aligned, ragged right. Under 880 px wide, a second pane becomes a tab instead.

S1 opened cold, a lesson stopped two days ago (s1-cold):

```
 ● ● ●   Confidence intervals   Midterm 2 plan
 
          Confidence intervals
          
          …the tutor's last explanation, as it was…
          
       ▏  Task. With n = 25, s = 10 and 95% confidence,
       ▏  what is the margin of error?
       ▏  E = 1.96 × 10 / 5 =▌                (your draft)
          
          Stopped on Tuesday at 16:40.   Return to continue
 
 Statistics 201                                              Magnesium
```

S2 one Return later (s2-start), then answered (s2-answer):

```
          ── Thursday ──────────────────────────────────
          Welcome back. You were working out a margin of
          error…
       ▏  Task. With n = 25, s = 10 and 95% confidence,
       ▏  what is the margin of error?
       ▏  E = 1.96 × 10 / 5 = 3.92
     ✓    Right, no help                      (gutter, from the record)
          Good. Now the interval itself…
```

S3 the plan changes (s3-before, s3-after): the request was written on the page, outside the task; the plan opens in the right pane, changes marked.

```
 ● ● ●   Confidence intervals   Midterm 2 plan
 ┌ Task. What is the 95% interval for…  (pinned) ┐│ Midterm 2: study plan
                                                 ││ | When | ~~2026-11-20~~ |
   I skipped three days and the midterm          │▌ | When | 2026-11-27 (Fri) |
   moved, change the plan.                       ││ ## 6. Blocks
   Done. The midterm is on Friday 27 Nov…        │▌ | 3 | Cold redos | … |
                                                 │▌ | 4 | Paper B    | … |
   Return to say more · Tab back to the task     ││ ## 14. Changes
                                                 │▌ - 2026-11-12: moved the date…
 Statistics 201                                              Magnesium
```

S4 off-plan, then back (s4-offplan, s4-return): the question and the reply sit on the page with no task frame and nothing in the gutter; the task stays pinned at the top; Tab, or "back to the task", returns to it with the caret in place.

```
 ┌ Task. What is the 95% interval for…   Tab ┐
   By the way, why do we divide by n − 1?
   Good question, and not on this plan…
   ▌
```

S5 after a week (s5-week, s5-week-next): the same as S1. The stop line names the day ("Stopped on Thursday 5 Nov"); nothing counts the days in between. Return, and the tutor starts the new sitting under a new day rule.

First open on a new Mac (s1-cold-first): one document, Welcome, with three plain lines of what is set up (the program, Claude Code, how lessons are paid) and three actions: Open a study folder (⌘O, the primary action), New study folder (⌘N), Find study folders on this Mac.

### Keys

| key | does |
|---|---|
| Return | hand in the answer, or send what you wrote; on a stopped lesson, continue |
| ⇧Return | new line |
| Esc | leave the task and write on the page (talk); close a sheet |
| Tab, ⌘↑ | back into the open task |
| ⌘⇧H, ⌘⇧L, ⌘⇧D | Hint, I'm lost, I don't know |
| ⌥1, ⌥2, ⌥3 | sure, unsure, guess on the answer |
| ⌘⇧A | photos of your working |
| ⌘⇧M | maths symbols |
| ⌘. | stop the reply |
| ⌘P | open any document: lessons, plan, material, papers, record, settings |
| ⌘K, ⌘⇧P | every command |
| ⌘O | another study folder (the library) |
| ⌘N | new lesson |
| ⌘W | close the tab; closing the live lesson saves where you stopped |
| ⌘1–⌘9, ⌃Tab | go to a tab |
| ⌘\ | split, or join, the panes; ⌘⌥← ⌘⌥→ moves between them |
| ⌘B | the folder's files |
| ⌘E, ⌘S | edit the plan as text, save it |
| ⌘, | settings |
| ⌘= ⌘− ⌘0 | text size |

### Principles

1. **Your files are the interface.** Every view is a document you could also open in Finder, or the record's own rows. The app adds a gutter and a status line, nothing between you and the page.
2. **Where you write decides what it is.** Answers inside the task, talk on the page. No label, no toggle.
3. **The gutter is the record's.** Marks and plan changes appear only in the gutter, only from the record or the file, never in the text and never from the tutor's words.
4. **Reopen, don't greet.** The window restores; it never summarises, counts or welcomes.
5. **Keys first, chrome last.** Anything the mouse can do has a key; anything that is not a document stays hidden until its key.

## What the app stops showing, and where it went

| shown before | now |
|---|---|
| Proof dots and proofs per skill | Not on any page. The record document lists, per skill, the days it was answered right from a blank page, in words. |
| Levels | Nowhere in the window. `magnesium progress` in a terminal still prints them. |
| Due counts (checks, redos) | Nowhere. The tutor opens a sitting with what is due, as part of the lesson. The record says, per skill, the day it comes back. |
| Study days and the run they make | Nowhere. The record's log is grouped by day, so the days are there, never counted. |
| Exam countdowns | Nowhere. The plan shows the date as the file has it. |
| The library of course cards | ⌘O, a list of folder names. The facts that were on a card are in each folder's plan. |
| End-of-lesson summary | The record's section for today. Closing a lesson shows nothing. |
| "Your answer" box | The writing place inside the task, which has no label. |

## Review against the brief

What my first plan had that I would have produced for any app, and what changed:

- **A chat column with a docked input box** (the default for anything with an AI in it). Replaced by a page with tasks as blocks: your answer is written into the task, and talk is written on the page. That one change is what makes talk structural rather than a toggle.
- **A home screen with the next lesson as a big button.** Replaced by restoring the last document. The only "button" on the first screen is a line of text that Return triggers.
- **A blue brand accent.** Kept a blue, but as ink, the learner's own colour for their own writing, with three other inks and "graphite" (none at all) one key away. The app has no colour of its own.
- **Icons in the sidebar and in ⌘P.** Removed: Sublime and Obsidian show names only, and the names are the learner's.
- **A settings modal.** Settings is a document in a tab.
- **A progress page.** It became the record, a log of your own answers.
- **Marks as coloured badges after the answer.** Moved to the gutter, in the quiet colour, with a shape and at most four words.

What is open, decided here: the window learns that a task is open from the reply's text, because no event says so; the engine keeps the real open task, so a misread costs nothing worse than today. The learner editing the plan file with ⌘E needs one new core command that writes only `Study/*/plan.md`, and refuses if the file changed since it was read; the record and the gap log are never touched.

## Round 4

The round-1 judges called this new and "an editor, not an app wearing a theme"; what held the four feelings was noise left on the page. This round changes structure and behaviour, not the look.

- **A lesson at rest is almost nothing.** Reopened, the lesson shows its open task with the half-written answer drawn as mathematics, then where you stopped and Continue; everything before is one quiet line ("The lesson so far"). Running, it shows this sitting and folds the earlier ones. After a week, Return continues, the tutor picks up where the screen said, and the caret is in the task.
- **The plan is read, not opened.** The plan document is the exam and its day, then what to study in order, each on its day. What the tutor moved is marked once, until seen; days that now run backwards say so. The file itself is one click away, and ⌘E edits it.
- **The open task comes from the record.** On Magnesium the engine says, after each turn, which task is open and whether the reply posed it (`open_task`, from concept B). The task block, the pinned line and the caret follow it, so a real tutor's bold question is never an empty strip.
- **Only the place to write under a task.** Help and confidence moved to keys and ⌘K, behind one quiet Help key. Placeholders meet AA, and `hints = off` removes them.
- **Settings is a file.** It is plain text and applied as you type: any installed font for the page and for your own words, any colours, every key binding, and hints. ⌘K reaches every value.
- **The record speaks once.** An answer with no mark says "No mark" with its reason one key away; a retry says nothing; the record page never claims more than its rows.
- **Chrome with a ground.** Documents fade at an edge where more lies beyond; the tab row gets a hairline when scrolled; sheets get a scrim.

## Round 5

The round-4 judges named the same few things; each is fixed in behaviour, not look.

- **The task comes back where it is answered.** With the real tutor, "back to the task" gets the question restated in prose, and the engine never says a reply to talk posed a task. Now a reply to talk that carries most of the open task's words shows it: the task block and the answer place go under it, the caret goes in, and the pinned line goes.
- **The draft is drawn once.** A kept draft with mathematics is drawn in place, as an editor's live preview draws it; its source shows only while it is edited.
- **The plan pane comes and goes.** It opens beside the lesson when the plan changes and closes once the change is seen (a message written, Esc, a new sitting). A window opened later opens on one page.
- **Every change is marked, and order is kept.** Moved, new and re-dated items carry a bar, with old days struck through. Reordered rows take their days with them, in the stand-in and in what the tutor is told; a day out of order is said quietly, not as an alarm.
- **The app's words left the column.** Key hints and "plan changed at" sit in the status line in the window's own type.
- **Marks explain themselves.** "No mark, not taken as the answer" when the engine kept the task open; the help words are help wherever they are typed.
- **Craft.** The light change colour is at AA, the tab names start at the text column, there is one date format ("Thu 12 Nov"), names are cut with one ellipsis, the lesson follows its end in two panes, and ⌘K lists every setting value so any can be set back.

## Round 6

- **Talk has a place.** While the caret is in talk, the end of the page shows a quiet rule. The caret stays in the task's answer line where it belongs: Tab no longer carries it out, ⌘K gives focus back where it was, and it is moved only away from a field the learner is not typing in.
- **No mark, with its reason.** On Magnesium the record is kept before the engine says which task is open, so an answer with no row there was not taken as the answer, and the page says so in place.
- **A plan change leaves a trace.** One quiet line in the lesson, read from the plan ("Plan: exam Tue 17 → Tue 24 Nov; 2 blocks moved; 2 blocks re-dated"), opens it; the plan's tab stays in the tab row after its pane closes.
- **Less bookkeeping.** The fold line has no count, Return is named once, the record's skills are a past-tense log of the learner's answers, and lessons are named after their goal.
- **The task block reads in order.** The answer place follows the latest hint; a task said again starts at its question.
- **Settings are a real file** (`settings.conf` in the window's state folder), named in the view, wrapped, and written back from ⌘K without moving its notes.
- **Craft.** Tab names start at the text column in every layout, titles and tabs typeset mathematics, inline formulas do not break, ⌘K rows are even and search by words.
