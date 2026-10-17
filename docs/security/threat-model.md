# Security and privacy boundaries

Magnesium protects workspace scope, learner-record integrity and recording controls. It runs with the current user's privileges, without an OS sandbox or storage encryption. Administrators and malicious same-user processes are outside its protection.

## Trust boundaries

| Boundary | Protection and limit |
| --- | --- |
| Host → Magnesium | Native adapters bind workspace, host and thread identity. Model-reported input stays labeled and cannot claim native provenance. |
| Sources → tutor | Scope, exclusion and content-version checks constrain reads. Retrieved text is data, never authority to change controls or access. |
| Magnesium → host model | Selected memories, original work and source context leave local storage. The host owns model access, tool permissions, authentication and provider history. |
| Runtime → filesystem | Managed-path checks and private permissions constrain state, caches and exports. They do not block same-user processes. |

Tool-only integrations rely on the host to identify current learner input correctly. A model-supplied speaker label is not independent proof that the learner said something.

## Learner records and controls

Typed requests validate references, stored speaker facts, excerpts and expected revisions before changing memory. Native capture authenticates event metadata locally. Source or tutor text cannot authorize pause, stop or recording changes.

Raw capture commits separately, so original input can survive a failed interpretation. A semantic transaction commits observations, memory revisions, checkpoints, reviews and receipts together, or rolls them back. Host-scoped request keys and payload hashes make retries idempotent and expose conflicting reuse. SQL values are parameterized; no model tool accepts arbitrary SQL.

Pause, stop and recording-off take priority over interpretation. Recording-off blocks new learner text in observations, checkpoints and context receipts. Reads close their snapshot before recording optional receipts, then recheck permission under the host/thread gate. Failed stop/off writes use text-free deny metadata where possible; failure to persist that protection is reported, without promising safety after restart.

## Files and source material

Source access rejects traversal, prohibited locations and links outside its authorized boundary. It checks an opened regular file's identity, scope and current content before returning text. Built-in exclusions protect Magnesium state, credential-like paths and private export subtrees; `.magnesium/magnesiumignore` cannot override them.

Known credential and private-key markers are checked in source bytes and converted text. This is a limited marker policy, not a general secret detector. Excluded sensitive sources retain status metadata without searchable content.

Source-cache ownership and schema are checked before use; foreign or incompatible files are preserved. Publication rechecks the source revision in a transaction so stale conversions cannot replace newer state. This disposable cache is separate from canonical learner state.

Exports carry a `.magnesium-export` marker before private content enters staging. Marked subtrees remain excluded from ordinary source access after copying or renaming. Source copies are opt-in and revalidated.

## Persistence and process limits

Private data includes `.magnesium/state.sqlite` and its sidecars, original events, memory history, checkpoints, receipts, exports and source-cache text. Magnesium uses owner-only Unix permissions and Windows process-SID DACL helpers. Recording-off and updates delete neither existing data nor provider conversations. Unsupported learner schemas are preserved and rejected without migration.

Request sizes, record counts and output packets have bounds. Learner reads use SQLite instruction/deadline guards and cooperative cancellation. PDF extraction runs in a same-user child with input, page, output and time limits plus kill/reap cleanup; this is not privilege isolation or a guarantee against peak parser allocation. Dependencies and concurrent filesystem races remain attack surfaces.

The MCP server runs filesystem and database work in bounded blocking workers. Controls have separate capacity and retain the same host/thread admission gate. Each call revalidates the connection's workspace binding and resolves trusted native identity independently of model arguments. The core Rust crate forbids unsafe code. Bundled SQLite, parsers, and OS dependencies remain trusted components.

See the [architecture](../architecture/learner-model.md) for data ownership and the [API contracts](../architecture/learner-model-contracts.md) for exact admission rules and limits.
