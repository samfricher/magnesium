# Learner API reference

For developers integrating Magnesium's tools or writing adapters. The [architecture](learner-model.md) explains the design; the linked Rust types define every field and variant. The [tool catalog](../../src/mcp/catalog.rs) generates MCP schemas from those types.

## Tools and requests

Magnesium exposes five tools. Actions are request fields, not separate tool names.

| Tool | Action | Purpose and main inputs |
|---|---|---|
| `context` | `instructions` | Packaged teaching instructions: `host`, `stage`, optional `operation` such as `tool_recipes` |
| `context` | `status` | Store, binding, checkpoint and pending/index status; optional `session_id` |
| `context` | `prepare` | Lesson context: `goal`, `original_query`, optional `mode`, `session_id`, `memory_ids`, `known_revisions`, `intents`, `literal_terms` |
| `context` | `session_choices`, `pending_events` | Session choices or unfinished input for the selected/bound session |
| `context` / `memory` | `pending_work` | Workspace pending work; optional host/session filters and `unbound_only` |
| `memory` | `search` | Same inputs as `prepare`, defaulting to focused retrieval |
| `memory` | `browse`, `topics` | Active memory cards, optionally filtered by exact `topic`; or flat topic counts |
| `memory` | `read` | Full selected records through `refs` |
| `memory` | `history`, `reviews` | Historical revisions for `memory_id`; or suggested reviews, optionally for a memory |
| `memory` | `observations` | Support records/relationships; `kind` selects `observation`, `event`, `memory_refs`, `event_refs` or `event_memory_refs` |
| `learning_step` | `update`, `control` | Atomic interpretation or explicit learner controls; see below |
| `search_sources` | — | Subject-file search: `query`, optional `limit` and workspace-relative `path_prefix` |
| `sources` | `inspect`, `status` | Source access using `path`, `content_sha256`, `anchor_kind`, `anchor_start`, `anchor_end` |

`sources` returns the current file hash, requested physical range, source authority, extraction method/version, coverage, warnings, and missing ranges. An omitted `anchor_kind` resolves to text lines or PDF pages after authorization; an incompatible explicit kind is rejected. An omitted `anchor_end` remains open-ended, distinct from an explicit numeric limit.

`inspect` returns bounded excerpts with their actual source ranges. Verified original text or PDF page references cover only the intersection with the known source extent; a range wholly beyond that extent has no original. Coverage distinguishes missing extraction, unreadable extraction, and ranges outside a known extent. Converter output with no physical end proof reports an unknown tail for an open-ended request. Empty sources have no invented range. `status` returns no excerpt or original bytes. Text-layer extraction never asserts visual analysis or verified formulas. External URLs remain labeled citations with snapshot guidance and are never fetched automatically.

`sources` prepares only the selected filename, without a search query or rank. Access uses current source policy, managed cache identity, coherent manifest and extraction facts, and verified current bytes. When preparation runs, `preparation` reports `complete`, `deferred_budget`, `deferred_concurrent_update`, or `pdf_content_failure`. Complete preparation does not guarantee complete coverage of the requested range. Deferred pending files never report complete extraction.

A PDF content failure can return a hashed original-page reference only after successful guarded publication and matching current facts. That reference returns no PDF bytes and does not establish that the requested page exists. Source copies require an indexed, authorized PDF and an exact current hash. Cancellation, storage, identity, policy I/O, worker failures, and superseded publication remain errors.

The MCP server advertises tools only. It exposes no UI resources or navigation links.

Learner-state requests require `schema:"learner-model/v2"`. `context.instructions` uses its own [instruction request](../../src/adapter/mod.rs); source tools use separate [source request types](../../src/adapter/types.rs). Old learner payloads/stores are rejected without migration.

`context.instructions` serves the caller-selected workflow and host adapter, plus the requested optional contract. `recommended_stage` echoes that routing choice. The response identifies the validated bundle and served files, reports delivery bounds, and returns their text. Authored workflows explain teaching decisions; Rust request types define accepted operations and fields. An unavailable response retains the routing choice and packaged version/hash when available, with a specific error instead of serving unvalidated instructions.

Workspace and host/thread identity come from the adapter. Persistent IDs are runtime-issued workspace references; do not invent them. Unknown fields and invalid variants are rejected. Keep original Unicode text and distinguish unknown timing, authorship, assistance and delivery from known facts.

## Read context and memory

Call `context` before teaching:

```json
{
  "schema": "learner-model/v2",
  "action": "prepare",
  "goal": "Understand coefficients",
  "original_query": "Which letters should I substitute?",
  "intents": [{
    "id": "symbol-roles",
    "purpose": "Find related work",
    "queries": [{"any_of": ["coefficient", "fixed parameter"], "all_of": []}]
  }]
}
```

`prepare` defaults to `auto`: return the complete compact directory if it fits the reserved directory budget, otherwise use focused retrieval. `overview` pages the directory; `focused` skips that attempt. Explicit memory IDs take priority. An omitted/null session uses this host's binding; an unbound host gets choices, not an automatically resumed session.

Each query means `(any_of alternatives) AND every all_of phrase`. Phrases match within one field, but required terms may span fields of the same memory. Queries are literal phrases, not caller-supplied FTS syntax. Ranking combines lexical matches across distinct formulations; it does not promise semantic completeness. `literal_terms` provides case-sensitive canonical substring matching when notation needs exact matching. Dirty/missing FTS leaves this, browsing and direct reads available.

`known_revisions:[{"memory_id":"…","revision":3}]` means that full text is still in the host's current context. Unchanged bodies may be omitted. Do not use it for text lost during compaction. `checkpoint_basis` compares the saved lesson's memory revisions with current state; changed, retired or review-qualified dependencies need rereading. It does not validate the saved diagnosis.

Call `memory` to read a returned reference, replacing `$MEMORY_ID` and the revision with actual returned values:

```json
{
  "schema": "learner-model/v2",
  "action": "read",
  "refs": [{"kind": "current_memory", "id": "$MEMORY_ID", "expected_revision": 1}],
  "max_bytes": 65536
}
```

Selected kinds are `current_memory`, `observation` and `event`. Reads return a fitting ordered prefix of whole records. Events alone support UTF-8-safe `start_byte`/`end_byte` ranges; large event text pages automatically. Descriptions and observations are never silently clipped.

List actions accept `limit`, `cursor` and `max_bytes`. Submit returned `next_call.tool` and `next_call.arguments` unchanged. Each call opens a new snapshot. Cursors bind the workspace, action, filters and relevant generations; `cursor_stale` requires a restart, while `invalid_cursor` rejects malformed/tampered input. Pages never advance past an unreturned record. `catalog_complete` means the whole active compact directory is present, not that all relevant knowledge was retrieved.

See [read request types](../../src/learner/read/requests.rs) for action-specific filters and [response types](../../src/learner/read/pages.rs) for packets and continuations.

## Record a learning step

`learning_step` with `action:"update"` requires a unique `request_id`. Arrays default empty. [Request fields](../../src/learner/model.rs):

| Field | Meaning |
|---|---|
| `native_capture` / `reported_events` | Verified `{ticket,text}` from the adapter or visibly model-reported fallback. Claude text is signed, compared and stored without the desktop app's matched `pasted_content` tag lines, leading blank lines or trailing whitespace |
| `observations` | Descriptions with existing event IDs, evidence roles, conditions and assistance |
| `memory_edits` | `create`, `revise`, `flag`, `retire`; existing edits need `expected_revision` |
| `session` / `checkpoint` | Lesson transition and optional complete checkpoint replacement |
| `event_dispositions` | Mark input `processed` or `no_change` |
| `review_updates` | Create a suggestion or update its status at an expected revision |

Here is a two-call fallback example. First, start a lesson and capture an exact learner message:

```json
{
  "schema": "learner-model/v2", "action": "update", "request_id": "example-start-1",
  "session": {"operation": "start", "client_ref": "lesson", "goal": "Identify coefficients"},
  "reported_events": [{
    "external_event_key": "answer-1", "speaker": "learner", "kind": "message",
    "text": "In 2x + 3, the coefficient of x is 2."
  }]
}
```

Use its `captured_event_ids[0]` as `$EVENT_ID` below. Then describe the work and create a memory in the same host conversation:

```json
{
  "schema": "learner-model/v2", "action": "update", "request_id": "example-interpret-1",
  "observations": [{
    "client_ref": "work", "kind": "learner_work",
    "description": "Identified the coefficient in one expression.",
    "events": [{"event_id": "$EVENT_ID", "role": "learner_response"}],
    "conditions_text": "One expression; transfer untested.",
    "assistance_text": "Earlier help unknown."
  }],
  "memory_edits": [{
    "operation": "create", "client_ref": "coefficient-memory",
    "record": {
      "title": "Coefficient roles", "scope": "Linear expressions",
      "summary": "One correct identification.",
      "state_text": "Identified 2 as the coefficient in 2x + 3. General understanding remains untested.",
      "current_basis": [{"new_observation": "work"}]
    },
    "reason": "Record this limited example.",
    "change_basis": [{"new_observation": "work"}]
  }],
  "event_dispositions": [{"event_id": "$EVENT_ID", "outcome": "processed"}]
}
```

The result maps local names to persistent IDs in `references` and returns `revisions`. New definitions share one unique `client_ref` namespace. Each existing memory/review/disposition target gets one command at most; support may be shared. Reported events reuse a stable `external_event_key`: identical payloads replay; changed payloads conflict.

`record.current_basis` selects 1–8 observation references supporting the complete description; older support remains in revision history. It is not an append-only list. `change_basis` separately explains this edit and may also cite events. Excerpts must occur in the original event, allowing only line-ending normalization. `learner_response` requires learner-authored input. Editorial changes reuse evidence rather than creating a new performance date.

Only session `start`/`resume` activates learning. Existing operations (`use`, `resume`, `pause`, `complete`) require the session's own expected revision. Resume binds without copying state; several conversations, such as a fork and its original, may share an active lesson. Pause or complete stops capture in every conversation bound to it. `selected_unbound_event_ids` explicitly attaches pending input; `turn_ref` never reassigns it.

Omitting `checkpoint` preserves it. Supplying one replaces it completely and requires a guarded `session` operation. Memory refs may use `revision:"after_edit"`; `pending_task:null` clears a task. Tasks need actual proposal events; observed delivery requires tutor events marked `delivery:"observed"`. Plans, native provenance and model reports do not establish receipt or attention.

## Controls and recovery

`action:"control"` accepts `pause`, `stop`, `recording_off`, `recording_on` for the current host/thread (a pause or stop receipt reports other conversations where the lesson is still active; `session.pause` stops them all), with a request ID and exactly one current native `turn_ref` or learner `reported_event`. Controls carry no semantic edits. Admitted pause/stop/off finishes despite caller cancellation; recording-on can cancel before permission changes. Recording-on and resume are separate.

With recording enabled, `mode:"complete_captured"` can finish a scoped pending learner `turn_ref` after stopping. It requires its disposition and cannot capture input, reactivate, change the goal, introduce/change a task or create a review. Each memory edit needs support from that turn. A checkpoint may preserve the identical task or clear it. Recording-off blocks completion and rich context receipts; existing data and provider history remain separate.

| Situation | Caller action |
|---|---|
| Lost response or uncertain commit | Replay the identical request ID and payload to recover its host-scoped successful receipt |
| `request_key_conflict` | A changed payload needs a new request ID |
| Revision conflict / already processed event | Read returned recovery references; reconcile current state before a new coherent update |
| Semantic rejection or cancellation | Keep returned capture IDs; raw input may remain pending although interpretation rolled back. A rejected start/resume that captured input returns `recovery`: repeat the same operation without new capture, selecting only its `retry_attachment_ids` (pending unbound learner input); other captured IDs are cited context |
| Packet too small | Follow the narrower/larger-budget recovery call; never strip qualifiers to force a fit |
| Dirty index | Use direct reads/browse/literal matching; deliberately rebuild with `magnesium memory reindex` |
| `source_superseded` | Repeat the source request against current state if still useful; no stale result was published |

Raw capture commits separately. Semantic records, dispositions and the successful receipt commit atomically. A recoverable index-only failure may leave canonical data committed with a dirty index; storage failure is not an empty result. Cancellation or response loss after commit does not imply rollback. Optional context receipts record packet preparation, not delivery or learning.

## Host adapter waits

The OpenCode adapter bounds each session metadata lookup to five seconds. It passes an abort signal through the supplied SDK client and also settles locally if the client ignores abort. Unverified results skip capture and remain retryable on the next turn. Native hook execution has its own five-second limit. Queue wait and separate phases can make the total callback take longer. Failure telemetry has a one-second limit and does not delay host callbacks.

The SDK client accepts a per-call `signal` through the [request options](https://github.com/anomalyco/opencode/blob/aec0b9a6d8898f68f923aaf08b7306d931fd9d76/packages/sdk/js/src/gen/client/types.gen.ts#L9-L64). The [session lookup](https://github.com/anomalyco/opencode/blob/aec0b9a6d8898f68f923aaf08b7306d931fd9d76/packages/sdk/js/src/gen/sdk.gen.ts#L455-L460) and [telemetry](https://github.com/anomalyco/opencode/blob/aec0b9a6d8898f68f923aaf08b7306d931fd9d76/packages/sdk/js/src/gen/sdk.gen.ts#L790-L799) definitions in OpenCode v1.18.34 forward those options.

## Resource limits

All sizes are UTF-8 bytes; encoded records must also fit their response envelope. Learner-state reads are bounded by cancellation, a two-second deadline and a SQLite instruction budget.

| Item | Bound |
|---|---|
| Request / inline event | 1 MiB / 256 KiB |
| Learner packet, default / maximum | 24 / 64 KiB |
| Instruction load response (core + workflow + operation contract) | 28 KiB |
| Title / scope / summary / review note | 256 / 512 / 1,024 / 1,024 bytes |
| Memory description / observation description | 16 / 8 KiB |
| Conditions / assistance / checkpoint | 2 / 2 / 16 KiB |
| Observations / memory edits / review edits / dispositions per update | 16 / 8 / 16 / 32 |
| Selected read refs / list page | 32 / 32 |
| Search intents / queries per intent / focused cards | 4 / 2 / 16 |

Complete bounds live in [`limits.rs`](../../src/learner/limits.rs) and [read validation](../../src/learner/read/requests.rs). Storage layout is in the [SQL schema](../../src/learner/schema.sql). Source tools enforce scope, exclusions and version checks independently; export-marked private trees are excluded. See the [security model](../security/threat-model.md) for those boundaries.
