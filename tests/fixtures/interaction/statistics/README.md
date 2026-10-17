# Statistics teaching fixtures

Synthetic statistics sources and paired learner scenarios for checking how Magnesium
responds to different reasoning about the same question. They contain no private
transcripts or course material.

## Prepare case inputs

From the repository root:

```sh
python3 ci/prepare_learning_effectiveness.py --output /tmp/statistics-inputs.json
```

The output must be a new file. Give the tutor only each case's `tutor_prompts`
and named source. Keep case/pair IDs, `evaluator` annotations, expected state,
and the full preparation manifest outside its workspace and context. Learner
histories are scripted statements, not seeded mastery records.

Start with `sum-locus` and its follow-ups, continue in a fresh process, then
compare `sum-weighting`. For CLT behavior, compare `clt-foundations` with
`clt-repetitions`. Use the same surface question in each pair and evaluate the
response to the differing reasoning, allowing multiple sound explanations.
Honor direct-answer, no-quiz, and stop requests. Preparation doesn't start a
model or establish teaching quality; scripted behavior doesn't measure human
retention.

## Files and generation

- `cases.json`: paired scenarios and private evaluator criteria
- `sources.md`: authored sources for those scenarios
- `chapter7_sources.md` and `chapter7_sparse.json`: sample-mean, discrete-count, and sparse-history regressions
- `proof_contracts.json`: exact/approximate numerical oracles and contract boundaries
- `source-pages.pdf`: four physical pages covering sum geometry, a visual-only integral bound, Normal parameter conventions, and a proof-first CLT outline

Regenerate the PDF with Python and ReportLab installed:

```sh
python3 tests/fixtures/interaction/statistics/generate_sources.py
```

The generator also checks rational endpoints, pentagon area, and a dependent
zero-covariance example. Page 2 deliberately draws a lower bound without text
operators. Its extracted text cannot establish what the visible formula says;
inspect the page or state that visual verification is unavailable.

The JSON and Markdown fixtures are authored inputs. Keep evaluator-only
information separate when changing a case; regenerate the PDF rather than
editing its bytes.

## Run deterministic regressions

```sh
cargo test --locked --test chapter7_teaching_regression --test proof_contracts
cargo test --locked --test sources_view synthetic_math_pdf_inspection_keeps_physical_page_scope
```

The first command checks fixture and scoring contracts; the source-view test
checks actual source bounds. Assess installed-host behavior against the private
criteria using its saved messages and tool results.
