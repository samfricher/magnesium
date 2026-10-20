# Gap log: <target>

One row per lost mark. A row closes after three cold passes on three different days. The first redo is at least a day after the repair, never on the same day. The next two are at least 3 and then 7 days apart, or closer when the assessment is near. Marks and scores live here; the learner record holds the interpreted state.

Status: open, 1 of 3, 2 of 3 or closed. Passes: the date and result of each redo, such as 2026-10-17 pass, or 2026-10-20 pass (MockA Q3) for a pass in a sitting. A fail starts the list again, such as 2026-10-24 fail. Next redo: the date the next redo is due. It is empty after a fail until the new repair, and once the row closes.

Type: K did not know, S slow, N notation or missing step, A arithmetic, R misread.

Conf: S sure, U unsure or G guess, the letter the learner wrote after the answer that lost the mark. It is blank when they wrote none. A failed redo sets it to the redo's letter, or to a blank if the redo has none. Conf is not evidence, so it never passes or closes a row. For each paper, the Scores Notes count the right answers out of the rated ones for each letter, such as Conf right: S 9/12, U 4/6, G 1/2. An answer is right when it gets full marks. Count answers, not rows, so each part of a question is one answer, and leave stretch items out.

## Tags

The tags, what each covers and the pages to re-read are in `graph.yaml`; the drills per tag are in `drills.md`.

## Gaps

| # | Date | Paper | Q | Tag | What went wrong | Type | Conf | Repair done | Passes | Next redo | Status |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | <YYYY-MM-DD> | <PAPER> | <Q> | <TAG> | <what was written, and what the mark scheme needed> | <K, S, N, A or R> | <S, U, G or blank> | <tighten, repair or teach, and what was done> | <empty until the first redo> | <YYYY-MM-DD> | open |

## Scores

| Paper | Date | Score | Time used | Notes |
|---|---|---|---|---|
| Diagnostic | <YYYY-MM-DD> | <score>/<marks> | <used>/<min> min | <marks per question; right answers per Conf letter; strengths; open tags> |

## Stretch items

<!-- Beyond scope. Score them here, apart from in-scope gaps. -->

| # | Date | Paper | Q | What happened | Status |
|---|---|---|---|---|---|
| 1 | <YYYY-MM-DD> | <PAPER> | <Q> | <what was written> | open |
