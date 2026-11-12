# One surface

Slug: `one-surface`

## The concept

The window is one surface, and there are no screens to move between. Opened, it shows one line of your own material, the thing to do next, already chosen, with the line you write on under it and the caret blinking there, the way Spotlight or a terminal opens: *Reading V_max and K_m from a Lineweaver–Burk plot*, and under it, quieter, *You stopped at the y-intercept.* Return starts it. Typing starts it too, with your words first. Nothing moves away: that line rises to the top, and the lesson fills the same surface below it, the tutor's words in a reading face and yours in the face you type in. When the tutor sets a task, the line you write on sits inside the task, under its question, the way an answer sits under a question on a worksheet; Return there hands it in as the answer, and the record's mark appears under it when the record has one. Below the task there is always the conversation's own line, marked only by `›`. Whatever you say there is talk, and code never marks talk: ↓ or Esc steps out of the task onto it, ↑ steps back in, and nothing is labelled, because the place is the label. Everything else (other courses, the plan, your material, your past work, settings) is a layer summoned by one key over where you are, ⌘O, ⌘P, ⌘E, ⌘Y, ⌘, or ⌘K for all of it, and Esc puts it away; none is a place you go. When you say "the midterm moved, change the plan", the tutor answers in the conversation and the window writes what changed in the plan right under that answer, read from the plan itself, where you are already looking; ⌘P shows the whole plan with those rows marked. Your past work is the same surface scrolled up, and the record of your answers is ⌘Y. The app never shows a level, a proof dot, a due count, a run of study days, a countdown to an exam, or a word about being behind.

## Why it meets the four feelings

**Mine.** The only large text on the surface is the learner's material and the two voices of the lesson; the app's own words are a hint line and the record's lines, in a small system face. It bends: reading face (Charter, Iowan Old Style, New York, the system sans, or mono), the face for your own words (mono or the same as reading), text size (⌘= and ⌘−, as in any Mac app), line length, scheme, and the colour of the ink (the caret and the record's rule), all kept on this Mac. It reopens as left: the course, the draft in the line, where the line was (task or conversation), and the scroll of the lesson. Every action has a key, and the layers are the same component everywhere, a line to type in over a list, so one habit covers all of them. The plan is the learner's own Markdown file: ⌘P shows it and ⌘↩ there opens it in their editor.

**Calm.** At rest the surface holds three lines and a caret: the course name, the next thing, where you stopped. There is no library, no course page, no strip, no sidebar, no footer. A lesson adds only the conversation. The layers dim the surface rather than replace it, so there is always one thing in front. Marks are words in the record's voice, never colour-coded verdicts.

**Easy to start.** The next action is chosen before you see the window, and it is one key: Return. If you would rather do something else, you type it, and that is the start. The window never opens on what is owed: due checks and paper redos are the tutor's to pose at the start of a sitting, and the surface does not count them. After a week away the surface is the same three lines; nothing says how long it has been. Progress is the record of your own answers (⌘Y), in plain sentences, with no streak, score or congratulation.

**Flexible.** Talk has its own line that is always there, and code sends what is said on it as talk (`intent: "talk"`), which the record cannot mark; the open task stays open above it, and ↑ goes back to it. Changing the plan is said in the conversation or in the plan layer's own line (⌘P, type, Return), and either way the change is drawn from `plan()` after `plan_changed`, inline under the tutor's reply and in the layer, with each changed row marked; the next thing on the rest surface follows it. Being off the plan is never shown: items have no late state, and an exam has a date, not a countdown.

## Design plan

### Colour

One ink and four neutrals per scheme. The ink is the only colour: the caret, the `›`, the rule of the task you are writing in, and the rule of the record's lines. The learner can choose it.

| Name | Dark | Light | Role |
|---|---|---|---|
| ground | `#1C1D20` | `#FAFAF8` | the surface |
| text | `#E3E3DF` | `#1E2023` | the material and both voices |
| quiet | `#9A9DA3` | `#6A6E75` | where you stopped, hints, the record's words |
| faint | `#3D4045` | `#CDCFD3` | rules, an empty task's line, placeholders |
| raised | `#26282C` | `#FFFFFF` | a layer over the surface |
| ink | `#9AA9EE` | `#2F3F86` | blue-black: caret, `›`, the record's rule |

Other inks the learner can pick: green-black (`#86C2A6` / `#2E5B4A`), violet (`#BDAEEF` / `#4E3C8C`), graphite (`#C7CAD0` / `#3B3E44`).

### Type

Three faces, one per voice, all already on every Mac, so the window loads none:

- **The material and the tutor**: Charter (fallbacks Iowan Old Style, `ui-serif`, Georgia). Charter was cut for screens at small sizes and sits well beside KaTeX's maths. Choices: Charter, Iowan Old Style, New York (`ui-serif`), System (`-apple-system`), Mono.
- **You**: `ui-monospace`, SF Mono, Menlo. Everything you type and everything you said is in it, so the two voices of a lesson need no bubbles, names or avatars. Choices: Mono, or the same face as the reading one.
- **The app**: `-apple-system` (SF Pro), small, for hints, keys and the record's lines. It is the system's own voice, so it recedes.

Scale at the default size (17 px), a major third: 12.5 · 14 · 17 · 21 · 27. The next thing at rest is 27/1.25, regular weight. The lesson's heading is 21/1.3. The tutor's prose is 17/1.62 (a serif needs the extra leading); your words are mono at 15/1.6, which matches the serif's x-height. The app's words are 13/1.4, key hints 12.5. Sizes S/M/L/XL (15/17/19/21) scale all of it. Text is left-aligned in one column of at most 38 em (about 66 characters), centred in the window.

### Layout and wireframes (1040 × 720)

At rest the block sits a third of the way down; on start it moves to the top in one 240 ms motion, the only motion the window makes of its own (none with reduced motion). A layer is a sheet 620 px wide at 12 % from the top, over a dimmed surface, radius 10 px; nothing else on the surface has a radius above 4 px.

**1. Opened cold** (`s1-cold`)
```
 ● ● ●

              Biochemistry
              Reading Vmax and Km from a Lineweaver–Burk plot
              You stopped at the y-intercept.

              › ▍
              ↩ carry on    ⌘K everything else
```

**2. Start a lesson** (`s2-start`, then `s2-answer`)
```
              Reading Vmax and Km from a Lineweaver–Burk plot
              ──
              You had the reciprocal form. The line it draws has
              1/Vmax where it meets the y-axis …

            ┃ Task. The line meets the y-axis at 0.25 s/µM.
            ┃ What is Vmax, with its units?
            ┃ ▍                                      ⌘/ help
              ›
```
```
            ┃ Task. The line meets the y-axis at 0.25 s/µM. …
            ┃ Vmax = 4 µM/s
            ┃ Marked right, first try, alone.
              Yes: the intercept is 1/Vmax …
            ┃ Task. Now the slope is 0.5 s. What is Km?
            ┃ ▍
              ›
```

**3. "Change the plan"** (`s3-before`, `s3-after`)
```
            ┃ Task. Now the slope is 0.5 s. What is Km?
            ┃ ·····························
              › I skipped three days and the midterm moved,
                change the plan
              Done. Midterm 1 is now on Tue 24 Nov, and Paper B▍
```
```
              Done. Midterm 1 is now on Tue 24 Nov, …
            ┃ The plan changed
            ┃ Midterm 1          Tue 17 Nov → Tue 24 Nov
            ┃ Cold redos         now before Paper B
            ┃ ⌘P the whole plan
              › ▍
              ↑ the task: What is Km?
```

**4. Off the plan, and back** (`s4-offplan`, `s4-return`)
```
              › By the way, why do some courses use a Hanes–Woolf plot?
              Good question, and it is not on this plan … (no mark, ever)
              › ▍
              ↑ the task: What is Km?
```
```
              › back to the task
              Back to it.
            ┃ Task. Now the slope is 0.5 s. What is Km?
            ┃ ▍
              ›
```

**5. After a week away** (`s5-week`): the rest surface of scenario 1 with the next thing as the plan now has it, and nothing about the week. Return (`s5-week-next`) and the lesson is under way.

**A layer** (⌘P, the plan)
```
        ┌────────────────────────────────────────────────┐
        │ › tell the tutor how to change it              │
        ├────────────────────────────────────────────────┤
        │ Midterm 1, Tuesday 24 November    moved        │
        │ ✓ Diagnostic paper                             │
        │ ● Repair: reading Vmax and Km                  │
        │ ○ Cold redos of the gap log        moved up    │
        │ ○ Paper B                          moved down  │
        │ ○ Paper C                                      │
        │ ↩ work on this   ⌘↩ open plan.md   esc         │
        └────────────────────────────────────────────────┘
```
Every layer is this one component: a line to type in, a list it filters, a hint row. Typing something that matches nothing offers, as the last row, to say it to the tutor.

### Keys

| Key | Does |
|---|---|
| ↩ | start the next thing; send what is on the line. ⇧↩ a new line |
| ↓ / Esc in a task | step out onto the conversation's line (talk) |
| ↑ on the conversation's line | step back into the open task (answer) |
| Esc | close a layer; else stop the tutor's reply; else step out of the task |
| ⌘K | everything: commands, courses, plan items, files; or say it to the tutor |
| ⌘O | courses (open, new, find, remove) |
| ⌘P | the plan as it stands, with a line to change it |
| ⌘E | the course's material and papers; add, paste |
| ⌘Y | your work: the record of your answers |
| ⌘, | settings |
| ⌘/ | help on the task: 1 a hint, 2 I'm lost, 3 I don't know |
| ⌥1 ⌥2 ⌥3 | how sure: sure, unsure, guess (added to the answer) |
| ⌘U | photos of your work, with the answer |
| ⌘. | stop the reply |
| ⌘= ⌘− ⌘0 | text size |
| ⌘N | a new lesson: what you type becomes its goal |

### Principles

1. **One surface; everything else is a layer.** No screen replaces the surface. A layer is summoned by a key, sits over where you are, and goes with Esc.
2. **The place is the label.** An answer is written in its task; talk is written on the conversation's line. The window sends each with its intent and never asks which it was.
3. **Two voices, two faces.** The material and the tutor read in a serif; you are in the face you type in. No bubbles, names or avatars.
4. **The record speaks small and in ink.** A mark or a plan change is a short line with an ink rule, in the app's face, and comes only from `marked`, `marks()` and `plan()`.
5. **Already chosen.** Whatever the window can decide (the next thing, the course, the stop line) it decides, and shows the choice so it can be overridden by typing.

## What the app stops showing

| Today | Here |
|---|---|
| Proof dots | Gone from every surface. ⌘Y says per skill, in words, on which days it was answered right, alone. |
| Levels | Gone. The runtime still keeps them (`magnesium progress`). |
| Due counts (checks, redos) | Gone. The tutor poses what is due at the start of a sitting. |
| Study days and their run | Gone. |
| Exam countdowns | Gone. The plan layer shows the exam's date as a date. |
| The library and the course page | Gone; ⌘O lists courses with each one's next thing. |
| The lesson's top strip (goal, steps, engine) | The goal is the lesson's heading; the steps are in ⌘P; the engine is in settings and ⌘K. |
| "Your answer" | No label anywhere: the task's own line is where an answer goes. |
| The end-of-lesson summary | Gone. Each mark is shown under its answer as it comes; ⌘Y keeps them. |

## Review against the brief

My first plan had five defaults I would have produced for any app, and each is changed:

- **The three courses listed on the first screen**, like Spotlight's results. Changed to one line: "already chosen" is the point, and the other courses are ⌘O.
- **A neutral grey with a link-blue accent.** Changed to one ink that is only the learner's caret and the record's rule, blue-black by default and theirs to change.
- **Chat bubbles**, with the learner's on the right. Changed to two faces for two voices, one column, no names.
- **Green ticks and red crosses for marks.** Changed to the record's words ("Marked right, first try, alone"), in the app's small face with an ink rule, the same for every mark.
- **The exam as "in 6 days".** Changed to its date, and only in the plan layer.

The brief's "the app recedes" also changed the type: my first pick was a bundled display serif for the next thing, a costume. All three faces are now the Mac's own, and the learner can pick each.

Calls made alone, with the reason:

- **Reopen.** The draft, the course, the line's place and the lesson's scroll are restored. After three hours away (the runtime's own gap for a new sitting) the surface opens at rest with the lesson directly above, out of view, instead of mid-sentence: a stale screen is a wall to climb when starting is the hard part.
- **Default place of the line.** When the tutor sets a task, the caret goes into it, because answering is then the next thing. Saying anything else is one key (↓ or Esc).
- **Help words** ("Can I have a hint?", "I'm lost", "I don't know") are sent without an intent, exactly as before, because the engine matches those words and records help or a decline from them.
- **Dropped files** go with the answer as photos of work when the line is in a task, and into the course's material otherwise: the place decides here too.
