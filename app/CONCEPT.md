# Logbook: the study window as a record of your own days

## The concept

The window is one long page: your logbook. Today is at the top under its date, and earlier days run down below it, each under its own date. Opening the app shows today's page with one line on it, the thing you stopped on ("Pick up where you stopped on Tuesday: reading Vmax and Km from a Lineweaver–Burk plot"), and the cursor under it. Press Return and that line becomes the first line of today; the tutor's first words are written in under it, and you are in the lesson. Or type your own first line and start there. A lesson is written into today's page as it happens. The tutor's words run as plain text. Each task it sets is set apart as an exercise, and the cursor sits inside it: what you write there is your answer, and the record's mark appears beside it in the left margin, the way a teacher marks a margin. Press Esc and the cursor steps out of the exercise onto the page: what you write on the page is talk (a question, "change the plan", something off the plan), which the record never marks, and the exercise waits, open, until you step back in with Esc. The plan is a short agenda in the right margin of today's page: the exam's date and the next few things, in order. Ask the tutor to change it and the agenda changes in place, with a timed line on today's page saying what moved. Your past work is the pages below: what you were asked, what you wrote, how the record marked it. After a week away, the page under today is simply the last day you worked. The app never shows a level, a count of proofs or of things due, a run of days, a countdown, or a greeting.

Slug: `logbook`.

## Why it meets the four feelings

**Mine.** The only large type on screen is a date and your own words; everything the app says is set smaller and fainter than anything you or the tutor wrote. The page is in the face, size and scheme you chose (⌘, or ⌘K), and they are kept on this Mac. The window reopens on the same day at the same scroll position with your half-written line still in the box and the cursor where it was, because the page is not a screen state to restore: it is the log, and today is still today. Every action has a key. Nothing on the page asks you for anything; the one suggestion is a line you may ignore by typing.

**Calm.** The cold page holds a date, one line and a cursor. Today's page fills the window, so the earlier days are below the fold, with only the next date peeking at the bottom edge to say they are there. There are no cards, no panels, no counts and no navigation chrome: courses, material, papers and settings are sheets that open on a key and close with Esc. During a lesson the margin holds the agenda and nothing else.

**Easy to start.** The next action is the line on the page, and Return does it. The app picks it the way you would: the lesson you last worked in, at the line you stopped at, worded as your own work ("where you stopped on Tuesday"), never as a debt ("3 due", "6 days left", "you missed a week"). Due quick checks and redos are the tutor's business once you are in; they are not shown before. Progress is the pages under today: a list of what you did, each answer with the record's mark, not a score anywhere.

**Flexible.** Where you write decides what a message is, so you never label one: inside the open exercise is an answer; on the page is talk, sent as `talk`, which the core makes impossible to mark. One key moves between them, and the exercise stays open while you talk. The agenda is the plan file as the runtime reads it, refreshed on every `plan_changed`, and each change is written into today's page from the difference between the plan before and after, never from the tutor's words. Being off the plan has no picture at all: the agenda has no "today" column, no overdue colour, no count of items behind; if today's page is about something else, that is what today's page is about.

## Design plan

### Colour

Two neutrals carry the page, one ink carries your words, and one green ink carries the record's marks: teachers who want marking to read as feedback rather than judgment mark in green, not red, and the record's marks are the only thing the app writes in colour.

| Name | Light | Dark | Used for |
|---|---|---|---|
| Paper | `#F6F7F5` | `#1D2022` | the page |
| Ink | `#1E2631` | `#E9EBE7` | your own lines, the date of today |
| Text | `#363B41` | `#C5C9C4` | the tutor's words |
| Margin | `#6A7076` | `#8C9296` | times, hints, the agenda's notes, earlier dates |
| Rule | `#DFE2DE` | `#30353A` | the date rule, an exercise's left rule, focus ring base |
| Green ink | `#2D6A55` | `#86C2A7` | the record's marks, the open exercise's rule, the cursor, focus |

Contrast on Paper, light and dark: Ink 14.2 and 13.7 to 1, Text 10.5 and 9.8, Margin 4.7 and 5.2 (so even the smallest margin note passes AA), Green ink 5.9 and 8.0.

### Type

- **The page** (your words, the tutor's, exercises, dates): one face, the learner's choice. Offered: Charter (default), Iowan Old Style, New York, the system sans (SF Pro), SF Mono. All are on every Mac; nothing is fetched. Charter is the default because it was drawn to stay sturdy at text sizes on screens, reads as a plain working page rather than a book, and has the even colour a page of mixed prose and maths needs. KaTeX keeps its own maths faces.
- **The margin** (times, marks, hints, the agenda): the system sans (`-apple-system`), with tabular figures for times, so the margin is quiet and aligned whatever face the page is in.
- Scale, on a base the learner picks (15, 17 default, 19, 21 px), ratio about 1.25: margin 0.76 em; page text 1 em with line height 1.6 (serif) or 1.5 (sans, mono); exercise 1 em; today's date 1.5 em regular; an earlier date 1 em, in Margin. No bold except what the tutor writes in Markdown. No capitals for labels.
- Measure: the page column is at most 36 em wide (about 66 characters of Charter).

### Layout

One column of text centred in the window, with a left gutter (5.5 em) that holds times and marks, and on wide windows a right margin (13 em) that holds the agenda of the course you are working in. Text is left aligned, ragged right. The top 36 px is an empty drag strip for the window controls. Sheets (courses, the course's material and papers, the record, settings, commands) open over the page from the right edge, one at a time, and close with Esc.

Below 860 px the agenda leaves the margin: it becomes one line under today's date ("Midterm 1, Tue 17 Nov. Next: Paper B"), and ⌘P opens it in full.

**1. Opened cold** (three courses, a lesson stopped two days ago)

```
┌────────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●                                                                          │
│                                                                                │
│            Thursday 12 November                                                │
│            ────────────────────────────────────────────────────                │
│                                                                                │
│            Pick up where you stopped on Tuesday                       ↩        │
│            Reading Vmax and Km from a Lineweaver–Burk plot                     │
│            Biochemistry                                                        │
│                                                                                │
│            ▏                                                                   │
│            Return picks it up, or write your own first line.  ⌘O other courses │
│                                                                                │
│                                                                                │
│                                                                                │
│            Tuesday 10 November ─────────────────────────────────────           │
└────────────────────────────────────────────────────────────────────────────────┘
```

**2. A lesson started** (one Return later)

```
│            Thursday 12 November                                                │
│            ──────────────────────────────────────────           Midterm 1      │
│    10:02   I'm back. I stopped at the y-intercept.               Tue 17 Nov    │
│                                                                                │
│            You stopped at the y-intercept. In the double-        Repair: Vmax  │
│            reciprocal form 1/v = (Km/Vmax)(1/[S]) + 1/Vmax …     and Km        │
│                                                                    y-intercept │
│          ┃ The line meets the y-axis at 0.25 s/µM. What is        slope        │
│          ┃ Vmax, with its units?                                 Paper B       │
│          ┃ ▏                                                     Cold redos    │
│          ┃ Hint   I'm lost   I don't know       Esc: talk        Paper C       │
```

After an answer, the mark sits in the gutter beside it, in green ink, from the record's row:

```
│          ┃ The line meets the y-axis at 0.25 s/µM. What is Vmax?               │
│   right  ┃ Vmax = 4 µM/s                                                       │
│ unaided  ┃                                                                     │
│            Right: the intercept is 1/Vmax …                                    │
│          ┃ The slope is 0.5 s. What is Km?                                     │
│          ┃ ▏                                                                   │
```

**3. "I skipped three days and the midterm moved, change the plan"** (written on the page, after Esc)

```
│          ┃ The slope is 0.5 s. What is Km?                       Midterm 1     │
│          ┊ (open)                                                Tue 24 Nov    │
│    10:07   I skipped three days and the midterm moved,           was Tue 17 Nov│
│            change the plan                                                     │
│            Done. Midterm 1 is now on Tue 24 Nov, and …           Repair: Vmax… │
│    10:07   Plan changed: Midterm 1 is on Tue 24 Nov (was         Cold redos    │
│            Tue 17 Nov). Cold redos come before Paper B.            moved up    │
│                                                                  Paper B       │
│            ▏                                       Esc: back to the exercise   │
```

**4. Off the plan, then back**

```
│    10:11   By the way, what's a Hanes–Woolf plot?                              │
│            Good question, and it is not on this plan …                         │
│                                                                                │
│          ┊ The slope is 0.5 s. What is Km?            Esc: back to it          │
│            ▏                                                                   │
│    ...                                                                         │
│    10:13   Back to the task                                                    │
│            Back to it. You were on step 3.                                     │
│          ┃ The line meets the y-axis at 0.25 s/µM. What is Vmax?               │
│          ┃ ▏                                                                   │
```

**5. After a week away**: scenario 1 again. The date under today is the last day you worked ("Thursday 5 November"), and the line says "Pick up where you stopped on Thursday 5 November". Nothing counts the week.

### Keyboard

| Key | Does |
|---|---|
| Return | Send. On an empty first line: pick up the suggested line |
| Shift-Return | New line |
| Esc | Step out of the open exercise onto the page (talk), or back into it; close a sheet |
| ⌘. | Stop the tutor's reply |
| ⌘E | End the sitting: one line saying where you stopped, kept for next time |
| ⌘N | A new lesson in this course: the next line you write is its goal |
| ⌘O | Courses: switch, add a folder, new folder, find folders, remove |
| ⌘I | This course: material, papers, the record |
| ⌘P | The plan in full |
| ⌘K | Every command by name |
| ⌘, | Settings: face, size, scheme, engine, payment, keys |
| ⌘⇧H / ⌘⇧L / ⌘⇧D | Hint / I'm lost / I don't know (in an exercise) |
| ⌥1 / ⌥2 / ⌥3 | Sure / unsure / guess, added to the answer |
| ⌘⇧A | Add a photo or PDF of your working |
| ⌘↑ / ⌘↓ | Today / the next earlier day |
| ⌘+ / ⌘− | Text size |

### Principles

1. **Time is the only axis.** There are no screens to move between: everything you did is on a page under a date, and everything you can do happens on today's.
2. **Your ink is the darkest.** Your lines are in Ink, the tutor's in Text, the app's own words in Margin at a smaller size. The app never speaks louder than you.
3. **The place decides the message.** An exercise holds answers; the page holds talk. Nothing is labelled "your answer".
4. **The record writes in the margin.** A mark appears only from the record's row, beside the answer it belongs to, in green ink, worded plainly ("right", "with a hint"). The tutor's words never make a mark.
5. **Nothing is counted.** No number on screen describes the learner: not days, not proofs, not items due, not days left.
6. **Everything is kept as left.** Face, size, scheme, scroll, draft, the open sheet, the cursor's place in or out of the exercise.

## What the app stops showing

| Today's vocabulary | In this concept | Where it still lives |
|---|---|---|
| Proofs, proof dots | Not on the page | ⌘I, "The record": each skill with the record's own note in words ("right from a blank page on 2 days; next on Fri 13 Nov") |
| Levels | Not on the page | The record sheet, one sentence at its foot, for whoever looks |
| Due counts (checks, redos) | Not shown before a lesson | The tutor poses them at the start of a sitting; the record sheet gives each one's day |
| Study days, the week strip | Gone | The pages are the days |
| Runs, streak-like counts | Gone | Nowhere |
| Exam countdown ("6 days left") | Gone | The agenda shows the date only |
| Lesson length countdown | Gone | A paper's minutes stay on the paper |
| The Library, Course, Lesson and Proofs screens | One page | Courses ⌘O, this course ⌘I, both sheets |
| End-of-lesson summary | Gone | Today's page is the summary |
| "Your answer" | Gone | The exercise is where answers go |
| The engine's name in the lesson | Gone | Settings |

## Review against the brief

My first plan had six defaults I would have produced for any study app. I changed each:

- **A sidebar of courses beside the lesson.** That is every chat app and every notes app, and it is the old Library split kept on screen. Replaced by one page across all courses: a sitting's course is a word in its first line, and switching is ⌘O.
- **A "today" summary at the top of the page** (answered 4, first answers 2). It is a scorecard wearing a diary's clothes. Removed: the page itself is what you did.
- **Warm cream paper, a high-contrast serif and a terracotta accent**, the reflex for "notebook". Replaced by a cool white page, a sturdy text face the learner can swap, and one functional colour, the green of a marking pen, used only by the record.
- **Monospace timestamps.** A tell, and it makes the margin noisier than the text. Replaced by the system sans with tabular figures.
- **An Answer / Talk switch on the box.** That is the label the brief rules out. Replaced by place: inside an exercise or on the page, with Esc between them, so the structure decides and the exercise stays visibly open while you talk.
- **The exam as a countdown in the agenda.** Replaced by the date. A date is a fact about the exam; a countdown is a fact about you.

The accessory removed before leaving the house: a coloured dot that marked which course a sitting belonged to. The course's name in the sitting's first line says it without a legend.
