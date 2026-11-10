# Magnesium architecture

Magnesium keeps a private learner model in each workspace so lessons can build on earlier work. This guide is for runtime contributors; see the [user guide](../user-guide.md) for setup and [API reference](learner-model-contracts.md) for tool calls.

## From a lesson to lasting context

The host model teaches and interprets. Magnesium's Rust runtime handles persistence, evidence, workspace boundaries and controls, using the host's existing model and authentication. There is no separate model service or vector store.

```text
Host reads context ──► teaches ──► learner responds
        ▲                              │
        │                     capture original input
        │                              │
        └── current memories ◄── interpret and commit
                                     │
                           observations + checkpoint

Workspace sources ──► separate source search/inspection ──► host
```

Original input is captured before optional interpretation. One transaction saves observations, memory changes, the checkpoint, review updates and a successful request receipt. Failed interpretation leaves captured input available without a partially updated profile.

## What is stored

| Location | Purpose |
|---|---|
| `.magnesium/state.sqlite` | Canonical learner events, observations, current memories, history, sessions and receipts |
| `.magnesium/index/source-search.sqlite` | Rebuildable search cache for subject files; separate from learner memory |
| Search indexes | Derived from stored state; never new evidence about the learner |
| `.magnesium/sittings.sqlite` | Quick checks, the attempts that moved them, sittings and what each sitting posed. Beside the learner store, which refuses other schema versions |
| `.magnesium/sittings.log` | One JSON line per sitting, rewritten from `sittings.sqlite`: what was due, posed, answered, declined, passed or failed, and how many more checks were due than the sitting took |

Learner payloads use `learner-model/v2` and database schema 2. Incompatible old stores are preserved and rejected without migration. The [SQL schema](../../src/learner/schema.sql) defines the fresh learner database.

A memory describes strengths, difficulties, assistance and uncertainty rather than a mastery score. Observation references connect it to original events; history retains earlier descriptions and reasons for change. New observations can correct an interpretation while preserving relevant earlier work.

A session holds the problem, working diagnosis and next step. Its checkpoint records the memory revisions used. Planned tasks stay in planning text until a proposal event exists. Reading or dismissing a review suggestion does not establish performance.

## Consistency and control

Reads use bounded SQLite snapshots. Small collections return a complete compact directory; larger ones use lexical retrieval and whole-record paging. Search cues are not evidence. Dirty indexes leave browsing, direct reads and literal matching available; `magnesium memory reindex` rebuilds them.

Writes check expected revisions. Identical successful retries return the saved receipt; competing edits require rereading and reconciliation. Cancellation can leave raw input pending, and a lost response cannot undo an already committed update.

A sitting begins with the first learner message after three hours without one. A prompt hook notes the time of each message in a conversation that records, never its text. A host without one counts a committed update that carries learner input. An `attempt` on an observation (its label, whether the first try was right, and the help given) drives the quick checks once the update commits. A wrong first try, or a right one reached only through a full solution, makes or restarts one check per label, due the next day. A right, unaided first try on a due check passes it, and it returns after 3 and then 7 days. The third pass, on a third day, closes it. The first tool result of a sitting carries a `sitting_opener` with up to three due checks and the paper redos that gap logs make due. Every turn's notice gives the counts until each check is posed or declined. In Claude Code and Codex, a reply that says "Quick check n" poses check n; on OpenCode, an answer or decline does. These facts schedule practice only. The observations are the evidence.

Pause, stop and recording controls apply to the current host/thread independently of interpretation; pausing or completing the lesson itself stops every conversation sharing it. Recording-on does not resume a lesson. Recording-off blocks new learner text in semantic records and context receipts, leaving existing data and provider conversations intact. A context receipt records packet preparation, not delivery or attention.

Source access checks scope, exclusions and file versions independently. Extraction happens outside the writer lock; revision checks prevent stale results from being published. A search indexes only what fits its own budget. After it returns, the MCP server keeps indexing the folder on one background thread until nothing is left, and stops that work when the host disconnects. Export-marked private trees stay excluded from source discovery and inspection, including after copying or renaming.

## Where the code lives

| Area | Entry points |
|---|---|
| Types and storage | [`model.rs`](../../src/learner/model.rs), [`records.rs`](../../src/learner/records.rs), [`store.rs`](../../src/learner/store.rs) |
| Capture and atomic updates | [`learner/update/`](../../src/learner/update/) |
| Snapshot reads and retrieval | [`learner/read/`](../../src/learner/read/), [`search.rs`](../../src/learner/search.rs) |
| Host controls and tool dispatch | [`adapter/`](../../src/adapter/), [`mcp/`](../../src/mcp/), [`request.rs`](../../src/request.rs) |
| Subject-source search | [`retrieval/manifest/`](../../src/retrieval/manifest/), [`retrieval/search.rs`](../../src/retrieval/search.rs), [`retrieval/catch_up.rs`](../../src/retrieval/catch_up.rs) |

See the [security model](../security/threat-model.md) for trust boundaries and [CONTRIBUTING](../../CONTRIBUTING.md) for development and validation.
