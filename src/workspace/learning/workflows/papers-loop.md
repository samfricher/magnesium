# Papers loop

Each turn, first start or resume the Magnesium session for the target before reading materials. Read `plan.md`, `gap-log.md`, `graph.yaml` and the tag memories, then act on the learner's verb in any phrasing: mark my PAPER, teach me TAG, repair TAG, cold redo, re-time today, fix my plan files (correct each `unreadable` item in `magnesium digest` at its `file` and `line`, keeping the learner's content, and rerun it until none is left). New mock and rebuild use stage papers. If `graph.yaml` does not exist, write it from the plan, the gap log's tags and the keys before the first marking, and add one `Tags:` line per key question, not per part: the primary tag first, then the other tags it fully exercises. Do not rebuild the plan.

## The loop

A sitting is a timed paper the learner works away from the chat under stated conditions, writing `?` instead of looking anything up, and returns whole. Its value is an unassisted baseline. Mark straight after, strictly to the mark scheme, not your preferred wording. Each part that lost marks becomes one gap-log row with a tag and a type: K did not know, S slow, N notation or missing step, A arithmetic, R misread. Log stretch apart. In papers and cold redos, ask for S (sure), U (unsure) or G (guess) after each answer, or before marking a paper without letters. Log a decline under Changes and do not ask again. One letter on a question covers its parts. Send each letter as its answer's confidence, or none, as for a lone `?`. After marking a paper, show S, U and G against right (full marks) and wrong answers, per part and without stretch, in a table. Keep the counts in its Scores Notes. Conf is not evidence and never passes or closes a row or changes a tag's reading. When you choose a repair, take Conf S rows first, after their unrepaired key_need tags (no Next redo). Rule 6 still orders a repair's steps after two fails.

## Read the marks through the graph

Each tag has one reading: untested (no cold evidence), open (a gap row at `open`), conditional (a gap row at `1 of 3` or `2 of 3`, passed only slow, partly, helped or not cold, or a key_need tag open) or sound (passed cold, unaided, in full and in time, itself or through contains, with nothing in needs open). Magnesium works each out from the gap log, the attempts and `graph.yaml`, and states them, with why and any tag's help level, in readings when you load this stage, start or resume the session, or record marks. Use those, and never write a reading into a memory. After marking, apply these in order:

1. Label each part's attempt with its key's primary tag, or for a part that lost marks the tag where its first consequential error lies, which can be in the question's needs.
2. A cold, unaided, full-mark, in-time pass is evidence for the primary tag and every tag in its contains; touches get nothing. A slow pass (Type S) counts for the primary tag only, as conditional.
3. When a tag gets an open row, move the planned questions of each tag with it in key_need after that tag's repair.
4. When a `Tags:` line names a tag that the graph does not connect to the primary tag, decide which file is wrong, fix it and log it under Changes, since readings follow the graph. Set the ramp from the readings, not from the question count alone.
5. A row for tag T gets a pass, without asking, when a later cold sitting passes in full and in time a question whose primary tag has T in contains: name the row in that answer's counts_for, and Magnesium passes it if its Next redo has come. Never count a Type S row this way, since slowness is retested directly. A touches edge only lets you skip the drills and go straight to the cold redo.
6. When a tag fails in two sittings, or fails a cold redo, start the next repair with its key_need tags (drills and one check each), then the tag. Recheck the question and its key. Then rewrite the next planned question on that tag, in its paper or redo file, with smaller steps or a worked example first. The mark scheme and the pass bar do not change.
7. When the first consequential error on tag Y lies in a tag X that is not in Y's needs, in two separate sittings, add X to Y's needs (and to key_need when the work runs through it).
8. Revise a tag memory only when its description changes. When an open tag needs a skill a fully correct part shows, answered with no hint, step or solution from you, add an "anchor:" line to that skill's tag memory, unless an anchor there already covers it. The readings choose the next block.

Log each move, rewrite and new edge as a line under Changes.

Size each repair:

- **Tighten** an N, A or R gap in minutes with a one-line correction, the exam-ready wording and one check.
- **Repair** one unknown rule or step, or slowness, in about 15 minutes with the missing connection, the help its readings name if they could not start (their attempt sends kind, the tag, and no_start), or under rule 6 a worked example, two or three checks of the same form, one varied check, then the spaced redos.
- **Teach** an unknown topic or mostly blank section in a block of an hour or more before the next paper, with that same shape per sub-skill, then the drills and the spaced redos.

## Spaced redos

Magnesium keeps each row's schedule and writes the Gaps table of `gap-log.md` from the learner record: read it, never edit it, and write no date, pass or status. A gap-log row closes after three cold passes on three different days. A pass is correct, from a blank page, without help, and in time for a Type S row. The first redo is at least 1 day after the repair, never on the repair day. The second is at least 3 days after the first pass, and the third at least 7 days after the second, sooner to fit before the plan's When date. An attempt while Next redo is empty or still to come counts neither way. A due redo that is not a pass fails the row, which waits for a new repair; Conf takes any letter the failing answer has.

Report instead. Each answer that lost marks carries one gap {type, what_went_wrong}, for its first error, and a question beyond scope has paper.stretch true. When a repair is done, send practice {item: "<plan>#<n>", repaired, next: {question, answer_key}}. Each redo's attempt names in counts_for every row it counts for, with slow true when over time, and next for the row's next redo. A next question is a new problem that asks for a result and never names or points at the tag, its method, the form of answer, the step or the earlier mistake ("Solve …", not "Solve … by <method>" or "Write … in the form …"), and each earlier miss gives it a different answer from the right one. In a question that covers several rows, or in a sitting, name a failed row only when the first error lies in its tag. Correct a mark with attempt_corrections, and a row logged in error with practice dropped. Asked for a cold redo with none due, say when one is and offer practice.

For a cold redo, read the rows whose Next redo has come, then pick the smallest set of questions, from the bank or new at the planned level, whose primary tags' contains cover every due tag. One cold pass counts once for every covered due row, by rule 5. Redo uncovered rows and every Type S row one by one. For a Type K row, prefer a question that gives a true result and asks why it holds, sets two look-alikes that need different methods and asks what decides it, or drops one assumption and asks which step fails. A give-the-reason question never stands alone in a cold redo: its first part asks the learner to produce the result from a blank page, the reason is its second part, and the row passes only when both parts are right. A given result earns full marks only for a correct reason.

## An older gap log

Magnesium brings in a gap log from any earlier template when a paper stage loads, keeps a copy, and lists one it cannot read under gap_logs.not_imported: fix those rows as for fix my plan files. With the papers skill's templates, also follow its template steps.

## Re-plan after every marked paper

Make ordinary changes yourself, log each as one line under Changes, and rewrite `plan.md` to match, since later conversations read it. If the learner changes when or where they practise, rewrite Sessions, adding it if missing. Each session is one line, "If it is <days> at <HH:MM>, then I <what> <where>.", with a 24-hour start time. Leave out "<days> at" for every day. Then add one backup line that starts "If I miss". Ask first only for a substantial new time commitment, as a guide adding over half the remaining hours and at least two, or pushing a never-drop block past the deadline. Say what changed and why, the cost (was X hours, now Y) and the options: accept, a shorter Z-hour version, or the old plan. If the learner declines, re-plan around their stated reason, without starting over.

## Records

Magnesium writes the Gaps table of `gap-log.md`; Scores and Stretch items are yours, and the learner record holds interpreted state.

- **Sitting.** One learner_work observation cites the learner event with the answers as learner_response and describes the work, never a score. Its attempts give each answer's marks, Conf and gap. conditions_text gives the timing, aids and duration the learner reported; assistance_text is "none reported".
- **Gap tag.** One memory per tag, with the tag string in topics; find it by that label, not by title. Each edit revises or creates it with the observation as current_basis and change_basis; its state_text describes the work, never the reading or help level. An update holds 8 memory edits; more tags go in a second update citing the observation by id.
- **Checkpoint.** Its text carries the paper, unfixed gaps, next redo and next block; memory_refs name the gap memories.
- **Review.** A gap-log row's Next redo is its only schedule, so create no review suggestion for it. A cold redo revises the tag memory; its attempt's counts_for names its row. A pass from above revises the memory with change_kind new_observation and the passing observation as basis, and the reason "pass in PAPER Qn".
