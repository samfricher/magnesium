# UI hill-climb screenshots

Screenshots for pull request review of issue #62, one folder per council round. Made-up data only. Not for merging.

## Issue #64: three new concepts

One folder per concept, `concept-<name>/round-<n>/`, made-up data only:

- `demo/`: `npm run shots` (the browser demo), 1x JPEG, `-dark` and `-light`. Stems follow the five scenarios: `s1-cold`, `s2-start`, `s2-answer`, `s3-before`, `s3-after`, `s4-offplan`, `s4-return`, `s5-week`, `s5-week-next`.
- `window/`: the release build of the real window, driven through the same scenarios against the stand-in model, with `perf.json` and `transcript.md`.
- `live/`: the same, against the real Claude Code in a scratch study folder, with `transcript.md` and the record's `attempts.json`.
- `judges/`: each judge's scores and notes, unedited. `scores.md` tabulates them.

The round-9 baseline for comparison is `round-9/`.
