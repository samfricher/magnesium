# Gap log: <target>

One row per lost mark. A row closes after three cold passes on three different days. A pass is a correct answer from a blank page, without help, and in time for an S row. The first redo comes at least 1 day after the repair, the second at least 3 days after the first pass, and the third at least 7 days after the second. If the assessment comes before that, shorten the gaps so each redo left falls on its own day before it. With fewer days left than redos, use every day left; the row stays short of closed. A pass counts only once the row's Next redo has come. A later cold sitting that passes a question fully containing the row's tag counts as one pass, except for an S row. A redo that is not a pass fails the row. In a question that covers several rows, or in a later sitting, a row fails only when the first error lies in its tag. A fail sets the row back to open and clears its passes, and the row needs a new repair. Marks and scores live here; the learner record holds the interpreted state.

Status: open, then 1 of 3 after the first pass, 2 of 3 after the second, and closed after the third. Passes: the date and result of each redo, such as 2026-10-17 pass, or 2026-10-20 pass (MockA Q3) for a pass in a sitting. A fail starts the list again, such as 2026-10-24 fail. Next redo: the date the next redo is due. Set it after each repair and after each pass. Leave it empty after a fail, until the new repair, and once the row closes.

Type: K did not know, S slow, N notation or missing step, A arithmetic, R misread.

## Tags

The tags, what each covers and the pages to re-read are in `graph.yaml`; the drills per tag are in `drills.md`.

## Gaps

| # | Date | Paper | Q | Tag | What went wrong | Type | Repair done | Passes | Next redo | Status |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | <YYYY-MM-DD> | <PAPER> | <Q> | <TAG> | <what was written, and what the mark scheme needed> | <K, S, N, A or R> | <tighten, repair or teach, and what was done> | <empty until the first redo> | <YYYY-MM-DD> | open |

## Scores

| Paper | Date | Score | Time used | Notes |
|---|---|---|---|---|
| Diagnostic | <YYYY-MM-DD> | <score>/<marks> | <used>/<min> min | <marks per question; strengths; open tags> |

## Stretch items

<!-- Beyond scope. Score them here, apart from in-scope gaps. -->

| # | Date | Paper | Q | What happened | Status |
|---|---|---|---|---|---|
| 1 | <YYYY-MM-DD> | <PAPER> | <Q> | <what was written> | open |
