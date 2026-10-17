# Retrieval regression corpus

Fixed queries and independently authored relevance judgments for
course material, code, and learner-journal sources. `workspaces/` supplies the
source files; `corpus.json` defines the queries and expected relevance;
`baseline-policy.json` sets quality and workspace-separation bounds. Some queries deliberately
use paraphrases that lexical search may miss.

Run from the repository root:

```sh
cargo test --locked --lib retrieval::evaluation::tests::structural_fts5_regression_meets_checked_in_quality_and_operational_bounds
```

The test checks retrieval quality, workspace separation, recovery,
and bounded reconciliation. It doesn't emit a standalone benchmark report.
`wrong_workspace_rate` counts boundary violations, including accepting an
adversarial request that combines one workspace with another's index even if
no hits are returned.

When changing the corpus, keep relevance judgments independent of the backend's
ranking signals. The checks live in
[the evaluation harness](../../../src/retrieval/evaluation.rs).
