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
| `.magnesium/state.sqlite` | Canonical learner events, observations, current memories, history, sessions and receipts, one row per marked answer in `attempts`, and the practice items (quick checks and gap-log rows) with what happened to each |
| `.magnesium/backups/` | The copy of the learner store made before it was upgraded from schema 2 or 3, kept as the way back |
| `.magnesium/index/source-search.sqlite` | Rebuildable search cache for subject files; separate from learner memory |
| Search indexes | Derived from stored state; never new evidence about the learner |
| `.magnesium/sittings.sqlite` | Sittings, the quick checks each took, and what became of each. Before schema 4 it also held the quick checks; they move to the learner store the first time this runtime opens it |
| `Study/<plan>/gap-log.md` | The learner's page of a plan's gap-log rows. The runtime writes its Gaps table from the learner store after each change; the tutor writes Scores and Stretch items. The original of a log brought in, and any Gaps table edited by hand, are saved in `.magnesium/cache/gap-log-backups/` |
| `.magnesium/sittings.log` | One JSON line per sitting, rewritten from `sittings.sqlite`: what was due, posed, answered, declined, passed or failed, and how many more checks were due than the sitting took |

Learner payloads use `learner-model/v2` and database schema 4. The [SQL schema](../../src/learner/schema.sql), the [tables schema 3 added](../../src/learner/schema_v3.sql) and the [tables and columns schema 4 added](../../src/learner/schema_v4.sql) define the fresh learner database.

The first time this runtime opens a schema 2 store, from the last release, or a schema 3 store, made only by the main branch, it upgrades it in place, once. Under the store's write lock it copies the store to `.magnesium/backups/state-schema-<version>-<time>-<id>.sqlite`, checks the copy, then adds the tables of each later schema and raises the version in one transaction. If any step fails, the store stays as it was. The copy is a complete store of its version: to go back to the runtime that made it, put it in place of `state.sqlite`. Other versions are preserved and refused.

Schema 3 adds the tables for issues #29, #30 and #31 at once, so a study folder is upgraded one time. `attempts` holds one row per marked answer: the question or which paper and question, a link to the observation and to the learner's own message, the mark (with marks out of marks on a paper), when, the time taken where a clock ran, the help given, the confidence letter, the skill it tests and whether it was cold. A helped answer is never stored as cold, and neither is a later answer on a label that already has one earlier the same day. `help_levels` and `anchors` stay empty until the runtime keeps help levels itself (#31). `magnesium attempts` lists every answer, and `--json` prints the rows.

Schema 4 makes `practice_items` again (it was empty in every store) for #30. A practice item is something the learner got wrong, to be reproduced cold on later days: a quick check from chat or a gap-log row from a paper. `practice_events` lists what happened to each, in order: made by an attempt, repaired by the tutor, or answered again by an attempt. One attempt can count for several items. No pass, status or date is stored. The runtime works them out from the events, the attempts they name and the plan's test date, the When row of its Facts table. The rule is the papers stage's: three cold passes on three different days, the first at least a day after the repair, then at least 3 and 7 days apart, sooner to fit before the test, never below a day. A due redo that is not a pass fails a gap-log row, which waits for a new repair. A quick check's miss restarts it for the next day. An item brought in from a gap log or the earlier quick-check store starts from the state it had there. `gap_logs` records each plan whose gap log the runtime has taken over, and a hash of the Gaps table it last wrote, so a hand edit is noticed. `attempt_corrections` keeps an attempt as it was before the tutor corrected it. `attempts.slow` marks a right answer over the time allowed, which never passes a type S row. `attempts.item_id` from schema 3 is not used.

A memory describes strengths, difficulties, assistance and uncertainty rather than a mastery score. Observation references connect it to original events; history retains earlier descriptions and reasons for change. New observations can correct an interpretation while preserving relevant earlier work.

A session holds the problem, working diagnosis and next step. Its checkpoint records the memory revisions used. Planned tasks stay in planning text until a proposal event exists. Reading or dismissing a review suggestion does not establish performance.

## Consistency and control

Reads use bounded SQLite snapshots. Small collections return a complete compact directory; larger ones use lexical retrieval and whole-record paging. Search cues are not evidence. Dirty indexes leave browsing, direct reads and literal matching available; `magnesium memory reindex` rebuilds them.

Writes check expected revisions. Identical successful retries return the saved receipt; competing edits require rereading and reconciliation. Cancellation can leave raw input pending, and a lost response cannot undo an already committed update.

A sitting begins with the first learner message after three hours without one. A prompt hook notes the time of each message in a conversation that records, never its text. A host without one counts a committed update that carries learner input. An `attempt` on an observation is stored as a row of `attempts`, and in the same transaction a chat attempt makes or answers the quick check on its label. A wrong or partly right first try, a right one reached only through a full solution, or a request for the full solution before any answer, makes or restarts one check per label, due the next day. A right, cold, unaided first try on a due check passes it, and it returns after 3 and then 7 days. The third pass, on a third day, closes it. A paper attempt with `gap` makes a gap-log row, and an attempt with `counts_for` redoes the rows it names. After the update commits, the runtime brings in any gap log it has not taken over and writes each plan's Gaps table. The first tool result of a sitting carries a `sitting_opener` with up to three due checks and the paper redos due, each with its stored next question. Every turn's notice gives the counts until each check is posed or declined. In Claude Code and Codex, a reply that says "Quick check n" poses check n; on OpenCode, an answer or decline does. These facts schedule practice only. The observations are the evidence.

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
