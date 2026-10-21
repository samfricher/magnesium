# <Target title>: study plan

<!-- Fill every section. Keep this file equal to the real plan: rewrite it after every change, and add one line under Changes for each change made without asking. Later conversations read this file first. -->

Folder: `Study/<target>/`. Built <YYYY-MM-DD>. Last re-plan <YYYY-MM-DD>.

## 1. Facts

<!-- Ask only for rows that the message and the folder do not answer, each with a default the learner can skip. A topic target needs only Target, Coverage and Exclusions. -->

| Item | Value |
|---|---|
| Target | <assessment or topic> |
| When | <the date in YYYY-MM-DD form first, then the day and time, such as 2026-10-21 (Wed) 12:30 to 14:00, or "no date" for a topic> |
| Length | <minutes> |
| Aids | <permitted aids, and what the instructor provides> |
| Weight | <share of the course grade> |
| Coverage | <chapters, sections, lectures> |
| Exclusions | <what is not examined> |
| Proficiency | <the learner's words>; read as <weak, mixed or strong> |
| Hours available | <total hours before the target> |
| Other commitments | <other assessments and plans, with the weight the learner gives each> |
| Answers come back as | <photos or PDF, unless the learner chose otherwise> |

## 2. The instructor's words

<!-- Quote what the instructor said about what the questions draw from, with the source and date. Write "none found" if there is nothing. -->

- "<quote>" (<source>, <YYYY-MM-DD>)

## 3. Research

<!-- For a university course, search in this order as a nudge, not a rule. Offer the library login once, before papers are written. Record every source searched, including the ones that gave nothing. -->

| # | Source | Found | Searched and not found |
|---|---|---|---|
| 1 | The same instructor's past tests and exams | <files> | <where and what you searched> |
| 2 | The same course under other instructors | | |
| 3 | The institution's past-exam collection, with the learner's login | | |
| 4 | Other universities that use the same slides or textbook | | |

Evidence that the instructor reuses a source: <the evidence, or "none">.

## 4. Paper bank

<!-- Rank by fidelity to the real assessment.
Bucket 1: the instructor's own past tests and exams.
Bucket 2: the instructor's own material with answers (assignments with feedback, tutorials with solutions, quizzes, lecture exercises, the homework list), other instructors' papers for the same course, and other universities' papers on the same material.
Bucket 3: papers generated from buckets 1 and 2.
Another instructor's paper moves to bucket 1 only with evidence that the current instructor reuses it; state that evidence in section 3. -->

### Bucket 1

- `<path>`: <what it is>. Sit it <when>, cold.

### Bucket 2

- `<path>`: <what it is>.

### Bucket 3

- `papers/<Name>.pdf`: <level and purpose>.

### Provenance

| Paper | Level | Time | Marks | Built from | Key check |
|---|---|---|---|---|---|
| Diagnostic | <level> | <min> | <marks> | <source questions, homework, tutorials, past papers> | <compile, tests, numeric check, reducer, or "checked by hand"> |

## 5. Method

1. Sit a paper under the stated conditions, timed, with the permitted aids only. Write `?` instead of looking something up. After each answer, write S (sure), U (unsure) or G (guess).
2. Mark it strictly against the key's mark scheme straight after.
3. Log every lost mark in `gap-log.md` with a tag and a type.
4. Repair at the depth the gap needs: tighten, repair or teach. Gaps on answers you were sure of come first.
5. Redo each gap cold, from a blank page, on three different days, with S, U or G after each answer. The first redo is at least 1 day after the repair, the second at least 3 days after the first, and the third at least 7 days after the second. If the assessment comes sooner, the gaps shrink to fit before it. A gap closes only after three unaided passes.
6. Sit the next paper. The plan is re-planned after every marked paper.

Question types:

- Easy: recall or one step.
- Medium: the course's standard multi-step form.
- Hard: an unfamiliar form, or a full proof or derivation where the course asks for one.
- Connection: two topics combined, a new setting, or a method chosen without a cue.
- Stretch: beyond scope, labelled, and scored apart.

Rating: <weak, mixed or strong>. Ramp: <papers in order>. <One line on why, from the Diagnostic result once it is marked.>

## 6. Blocks

<!-- Hours, not clock times; Sessions says when the learner practises. A paper costs about three times its length once marking and repair count. Group blocks into days only if the learner gave availability. Suggest spacing for retrieval, for example a cold redo the next day rather than the same evening. Fit each gap's three cold redos before the assessment. -->

| # | Block | Purpose | Duration and stop line | Have open | Running total |
|---|---|---|---|---|---|
| 1 | <name> | <purpose> | <60 min, stop at 70> | <files and aids> | <1 h> |

Never drop: <blocks that stay whatever happens, including every mark, log and repair block>.

Drop order if time runs short: <first block to drop>, then <next>.

## 7. Sessions

<!-- When and where the learner practises, from the build question. Write one line per session in the form below, because `magnesium digest` reads its days and time. Days are day names, a range such as Monday to Friday, a list such as Tuesday and Thursday, weekdays or the weekend. Use a 24-hour start time, such as 08:00.
For a session every day, leave out "<days> at".
Example: If it is Monday to Friday at 08:00, then I do the due cold redos at my desk before class.
Add one backup line for a missed session, such as: If I miss the morning, then I do them after dinner.
If the learner skipped the question, write "none planned". Rewrite these lines when the learner changes when or where they practise. -->

- If it is <days> at <HH:MM>, then I <what I do> <where>.
- If I miss <a session>, then I <what I do instead, and when>.

## 8. Must know cold

<!-- Definitions, statements, formulas and recipes that the learner must write from memory, in the course's notation. -->

- <item>

## 9. Tags

The tags, what each covers and the pages to re-read are in `graph.yaml`.

## 10. Anchors

<!-- Copies of the anchors that this plan's topics use. An anchor is a skill or way of thinking that the learner's own work has shown, with where they showed it. Its home is a line starting with a lowercase "anchor:" in the state_text of the memory for that skill. Write that line first, then copy it here. Before an explanation, the tutor names the anchor it builds on. A self-report, a tutor explanation or a source is never an anchor.
At build: run a memory search with literal_terms ["anchor:"], read each memory it returns, and copy each "anchor:" line that names a skill this course uses. If there are none, write "none yet".
After each marking: when an open tag needs a skill that a fully correct part shows, add an "anchor:" line to that skill's tag memory and copy it here.
After a repair: when the learner answers the check question correctly, add the approach that worked to the memory for that skill, written as what the learner did, and copy it here.
Keep the list short. When a newer anchor covers an older one, replace it in the memory and here. -->

- anchor: <what the learner did, such as "applies the chain rule to single-variable functions"> (<where they showed it, such as Diagnostic, Q3>)

## 11. Assessment-day checklist

- <what to bring>
- <the order to attempt questions, and the time budget per mark>
- <checks that the gap log says to make on every answer>

## 12. Facts to confirm with the TA or instructor

- <question>; ask by <when>.

## 13. Using this folder

Say any of these, in your own words:

- "mark my <PAPER>", with photos or a PDF of your answers.
- "teach me <TAG>", for a teaching block on one tag.
- "repair <TAG>", for a short repair on one tag.
- "cold redo", to redo the gap-log rows that are due, from a blank page.
- "new mock", for another paper at assessment level.
- "re-time today", to fit today's blocks to the hours you have.
- "rebuild", to start this plan again.

Open a key only after you return the sitting. Magnesium keeps a learner record in this workspace, so a later conversation continues from this plan, `gap-log.md` and that record.

## 14. Changes

<!-- One line per change made without asking: date, what changed, why. -->

- <YYYY-MM-DD>: <what changed>, because <reason>.
