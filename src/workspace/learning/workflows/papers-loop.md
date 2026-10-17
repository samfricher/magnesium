# Papers loop

Each turn, first start or resume the Magnesium session for the target before reading materials, since native capture expires within minutes. Read `plan.md`, `gap-log.md`, `graph.yaml` and the tag memories, then act on the learner's verb in any phrasing: mark my PAPER, teach me TAG, repair TAG, cold redo, re-time today. New mock and rebuild use stage papers. If `graph.yaml` does not exist, write it from the plan, the gap log's tags and the keys before the first marking, and add one `Tags:` line per key question, not per part: the primary tag first, then the other tags it fully exercises. Do not rebuild the plan.

## The loop

A sitting is a timed paper the learner works away from the chat under stated conditions, writing `?` instead of looking anything up, and returns whole. The one-task-then-wait rule governs in-chat practice and repair, not a sitting, whose value is an unassisted baseline. Keep keys private until the sitting returns. Mark straight after, strictly to the mark scheme, not your preferred wording. Each lost mark becomes a gap-log row with a tag and a type: K did not know, S slow, N notation or missing step, A arithmetic, R misread. Log stretch apart. Repair, then cold redo after a delay; only an unaided pass closes a gap. interpret-and-repair reads returned work, teach-and-practice runs teaching blocks, review-and-transfer runs cold redos, and consolidate-and-resume resumes later.

## Read the marks through the graph

Each tag has one reading, in its memory: untested (no cold evidence), open (an open gap row), conditional (passed only slow, partial, scaffolded or through touches, or a key_need tag is open) or sound (passed cold, unaided, at the planned level, with nothing in needs open). Derive it again at every marking from the gap log, the memories and the graph. After marking, apply these in order:

1. Tag a lost mark where the first consequential error lies, which can be a tag in the question's needs. The primary tag then reads conditional.
2. A cold, unaided, full-mark, in-time pass is evidence for the primary tag and every tag in its contains; touches get nothing. A slow pass (an S row) counts for the primary tag only, as conditional.
3. When a tag gets an open row, each tag with it in key_need reads conditional, and a sound reading that was inferred, not tested, is revoked. Move their planned questions after that tag's repair. Course tags that do not need it proceed.
4. Read the evidence in each marked question two ways, and name the kind in the memory. Direct: each tag on its key's `Tags:` line whose part was fully correct and in time reads sound, "from PAPER Qn part x" ("from PAPER Qn" for a one-part question), with or without a graph edge. Inferred: from those tags, follow contains edges only; a tag reached that way that is never a question's primary tag reads sound, "inferred from PAPER Qn via contains". A touches edge or a key_need link gives nothing. In a partly correct question, only the correct parts' tags get evidence; a tag used after the first error gets nothing. When a `Tags:` line names a tag that the graph does not connect to the primary tag, decide which file is wrong, fix it and log it under Changes. Set the ramp from the readings, not from the question count alone.
5. An open row for tag T closes, without asking, when a later cold sitting passes in full and in time a question whose primary tag has T in contains. Write "closed by PAPER Qn" in Repair done and set Status to closed. Never close an S row this way, since slowness is retested directly. A touches edge closes nothing; it only lets you skip the drills and go straight to the cold redo.
6. When a tag fails in two sittings, or fails its cold redo, start the next repair with its key_need tags (drills and one check each), then the tag. Recheck the question and its key. Then rewrite the next planned question on that tag, in its paper or redo file, with smaller steps or a worked example first. The mark scheme and the pass bar do not change.
7. When the first consequential error on tag Y lies in a tag X that is not in Y's needs, in two separate sittings, add X to Y's needs (and to key_need when the work runs through it).
8. Revise each affected tag memory with its new reading and reason. The readings choose the next block.

Log each move, rewrite, close and new edge as a line under Changes.

Size each repair:

- **Tighten** an N, A or R gap in minutes with a one-line correction, the exam-ready wording and one check.
- **Repair** one unknown rule or step, or slowness, in about 15 minutes with the missing connection, one worked example, two or three checks of the same form, one varied check, then the blank-page redo.
- **Teach** an unknown topic or mostly blank section in a block of an hour or more before the next paper, with that same shape per sub-skill, then the drills and the redo, since more papers do not help without a starting move.

For a cold redo, read the due review suggestions, then pick the smallest set of questions, from the bank or new at the planned level, whose primary tags' contains cover every due tag. One cold pass closes every covered row by rule 5. Redo uncovered rows and every S row one by one.

## Re-plan after every marked paper

Make ordinary changes yourself (reorder, change questions, add a topic or teaching block), log each as one line under Changes, and rewrite `plan.md` to match, since later conversations read it. Ask first only for a substantial new time commitment, as a guide adding over half the remaining hours and at least two, or pushing a never-drop block past the deadline. Say what changed and why, the cost (was X hours, now Y) and the options: accept, a shorter Z-hour version, or the old plan. If the learner declines, re-plan around their stated reason, without starting over.

## Records

Mark and repair inside the target's Magnesium session. `gap-log.md` is the question-level checklist with marks and scores; the learner record holds interpreted state.

- **Sitting.** One learner_work observation cites the learner event with the answers as learner_response and describes the work, never a score. conditions_text gives timing, aids and duration as the learner reported them; assistance_text is "none reported", since work away from the chat is unobservable.
- **Gap tag.** One memory per tag, with the tag string in topics; find it by that label, not by title. Each edit revises or creates it with the observation as current_basis and change_basis, and its state_text gives the reading in words, such as "conditional: needs TAG, which is open". An update holds 8 memory edits; more tags go in a second update citing the observation by id.
- **Checkpoint.** current_problem, working_diagnosis, next_action and plan_text carry the paper, open gaps and next block; memory_refs name the gap memories.
- **Review.** Each open gap tag gets one review_updates create (client_ref, memory_id, purpose, reason) with the spacing nudge as due_at_ms, a suggestion only, at most 16 review edits per update. A passed cold redo marks it done, with activity_event_id as the learner's redo event, and revises the memory. A close from above revises the memory with change_kind new_observation and the passing observation as basis, and the reason "closed by PAPER Qn", then marks the review done with that sitting's event as activity_event_id; a review update has no reason field.
