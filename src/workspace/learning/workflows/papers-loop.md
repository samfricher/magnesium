# Papers loop

Each turn, first start or resume the Magnesium session for the target before reading materials. Read `plan.md`, `gap-log.md` and course_map read for the target, then act on the learner's verb in any phrasing: mark my PAPER, teach me a section or item, repair an item, cold redo, re-time today, fix my plan files (correct each `unreadable` item in `magnesium digest` at its `file` and `line`, keeping the learner's content, and rerun it until none is left). A new mock uses stage papers; a re-plan or rebuild starts the planning run. A key question with no `Items:` line gets one before the first marking: the course map items it uses, the item it comes from first.

## The loop

A sitting is a timed paper the learner works away from the chat under stated conditions, writing `?` instead of looking anything up, and returns whole. Its value is an unassisted baseline. Mark straight after, strictly to the mark scheme, not your preferred wording. Each part that lost marks becomes one gap-log row with its item and a type: K did not know, S slow, N notation or missing step, A arithmetic, R misread. Log stretch apart. In papers and cold redos, ask for S (sure), U (unsure) or G (guess) after each answer, or before marking a paper without letters. Note a decline in the learner's memory and do not ask again. One letter on a question covers its parts. Send each letter as its answer's confidence, or none, as for a lone `?`. After marking a paper, show S, U and G against right (full marks) and wrong answers, per part and without stretch, in a table. Keep the counts in its Scores Notes. Conf is not evidence and never passes or closes a row or changes an item's reading. When you choose a repair, take Conf S rows first. Rule 4 still orders a repair's steps after two fails.

## Read the marks by item

Each label has one reading: untested (no cold evidence), open (a gap row at `open`), conditional (a gap row at `1 of 3` or `2 of 3`, or passed only slow, partly, helped or not cold) or sound (passed cold, unaided, in full and in time). Magnesium works each out from the gap log and the attempts, and states them, with why and any kind's help level, in readings when you load this stage, start or resume the session, or record marks. Use those, and never write a reading into a memory. What was never asked is on the course map. After marking, apply these in order:

1. Label each part's attempt with the course map item it comes from, the first on its `Items:` line; for a part that lost marks, use the item where its first consequential error lies, which can be an item the question uses.
2. A cold, unaided, full-mark, in-time pass is evidence for that item only. Nothing passes along a use link. A slow pass (Type S) counts as conditional.
3. A row for item T gets a pass, without asking, when a later cold sitting has the learner do T's part of a larger question, unaided, in full and in time: name the row in that answer's counts_for, and Magnesium passes it if its Next redo has come. Never count a Type S row this way, since slowness is retested directly.
4. When an item fails in two sittings, or fails a cold redo, start the next repair where it broke, inside the problem: a worked case or a smaller example, one check, then a changed version of the same problem. Go back to an earlier item only when the same step breaks again or there is no start, then return. Recheck the question and its key against the course material. The mark scheme and the pass bar do not change.
5. When the first consequential error on one item lies in an item that the map does not link it to, in two separate sittings, add that use link with course_map write.
6. Revise a memory only when its description changes. When an open item needs a skill a fully correct part shows, answered with no hint, step or solution from you, add an "anchor:" line to that skill's memory, unless an anchor there already covers it. The plan's blocks choose the next block.

Size each repair:

- **Tighten** an N, A or R gap in minutes with a one-line correction, the exam-ready wording and one check.
- **Repair** one unknown rule or step, or slowness, in about 15 minutes with the missing connection, the help its readings name if they could not start (their attempt sends kind, the item, and no_start), or under rule 4 a worked example, two or three checks of the same form, one varied check, then the spaced redos.
- **Teach** an unknown topic or mostly blank section in a block of an hour or more before the next paper, with that same shape per sub-skill, then the drills and the spaced redos.

## Spaced redos

Magnesium keeps each row's schedule and writes the Gaps table of `gap-log.md` from the learner record: read it, never edit it, and write no date, pass or status. A gap-log row closes after three cold passes on three different days. A pass is correct, from a blank page, without help, and in time for a Type S row. The first redo is at least 1 day after the repair, never on the repair day. The second is at least 3 days after the first pass, and the third at least 7 days after the second, sooner to fit before the plan's When date. An attempt while Next redo is empty or still to come counts neither way. A due redo that is not a pass fails the row, which waits for a new repair; Conf takes any letter the failing answer has.

Report instead. Each answer that lost marks carries one gap {type, what_went_wrong}, for its first error, and a question beyond scope has paper.stretch true. When a repair is done, send practice {item: "<plan>#<n>", repaired, next: {question, answer_key}}. Each redo's attempt names in counts_for every row it counts for, with slow true when over time, and next for the row's next redo. A next question is a new problem that asks for a result and never names or points at the item, its method, the form of answer, the step or the earlier mistake ("Solve …", not "Solve … by <method>" or "Write … in the form …"), and each earlier miss gives it a different answer from the right one. In a question that covers several rows, or in a sitting, name a failed row only when the first error lies in its item. Correct a mark with attempt_corrections, and a row logged in error with practice dropped. Asked for a cold redo with none due, say when one is and offer practice.

For a cold redo, read the rows whose Next redo has come, then pick the smallest set of questions, from the instructor's exercises or new at the planned level, whose parts ask every due item; reach counts the items each candidate uses. One cold pass counts once for every due row whose part the learner did unaided, by rule 3. Redo uncovered rows and every Type S row one by one. For a Type K row, prefer a question that gives a true result and asks why it holds, sets two look-alikes that need different methods and asks what decides it, or drops one assumption and asks which step fails. A give-the-reason question never stands alone in a cold redo: its first part asks the learner to produce the result from a blank page, the reason is its second part, and the row passes only when both parts are right. A given result earns full marks only for a correct reason.

## An older gap log

Magnesium brings in a gap log from any earlier template when a paper stage loads, keeps a copy, and lists one it cannot read under gap_logs.not_imported: fix those rows as for fix my plan files. With the papers skill's templates, also follow its template steps.

## Re-plan after every marked paper

Record each finished block with course_map write blocks_done (its number, the date and the minutes it took). If the learner changes when or where they practise, write them with course_map write facts sessions. Each session is one line, "If it is <days> at <HH:MM>, then I <what> <where>.", with a 24-hour start time. Leave out "<days> at" for every day. Then add one backup line that starts "If I miss". Then start the planning run with what changed: the paper's marks by item, the minutes, and the learner's words. Ask first only when its report proposes a new cut or a substantial new time commitment: say what changed and why, the cost (was X hours, now Y) and the options, and record the learner's answer on the map.

## Records

Magnesium writes the Gaps table of `gap-log.md`; Scores and Stretch items are yours, and the learner record holds interpreted state.

- **Sitting.** One learner_work observation cites the learner event with the answers as learner_response and describes the work, never a score. Its attempts give each answer's marks, Conf and gap. conditions_text gives the timing, aids and duration the learner reported; assistance_text is "none reported".
- **Gap item.** One memory per item that keeps breaking, with the item id in topics; find it by that label, not by title. Each edit revises or creates it with the observation as current_basis and change_basis; its state_text describes the work, never the reading or help level. An update holds 8 memory edits; more go in a second update citing the observation by id.
- **Checkpoint.** Its text carries the paper, unfixed gaps, next redo and next block; memory_refs name the gap memories.
- **Review.** A gap-log row's Next redo is its only schedule, so create no review suggestion for it. A cold redo revises the item's memory; its attempt's counts_for names its row. A pass from above revises the memory with change_kind new_observation and the passing observation as basis, and the reason "pass in PAPER Qn".
