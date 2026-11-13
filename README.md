# UI hill-climb screenshots

Screenshots for pull request review of issue #62, one folder per council round. Made-up data only. Not for merging.

## Issue #64: three new concepts

One folder per concept, `concept-<name>/round-<n>/`, made-up data only:

- `demo/`: `npm run shots` (the browser demo), 1x JPEG, `-dark` and `-light`. Stems follow the five scenarios: `s1-cold`, `s2-start`, `s2-answer`, `s3-before`, `s3-after`, `s4-offplan`, `s4-return`, `s5-week`, `s5-week-next`.
- `window/`: the release build of the real window, driven through the same scenarios against the stand-in model, with `perf.json` and `transcript.md`.
- `live/`: the same, against the real Claude Code in a scratch study folder, with `transcript.md` and the record's `attempts.json`.
- `judges/`: each judge's scores and notes, unedited. `scores.md` tabulates them.

The round-9 baseline for comparison is `round-9/`.

`concept-<name>/final/` holds each concept's final build: `demo/` and `window/` as above, and `live/`, one real lesson with the real Claude Code in which a person answered the tutor's first task correctly (`c3-answer-1` shows the record's mark), then asked to change the plan, went off-plan and came back. `head-to-head/` holds the four final judges, who ranked the three concepts against each other with no ties, and `ranking.md`, the table in the pull requests.
