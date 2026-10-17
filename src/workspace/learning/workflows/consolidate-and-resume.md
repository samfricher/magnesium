# Consolidate and resume

Preserve the generating relationship, conditions, and next useful step. Distinguish explanations from demonstrated work.

Checkpoint: current_problem, working_diagnosis, next_action, optional plan_text, memory_refs, pending_task. Distinguish proposal, observed delivery, and actual help. A planned task without a captured proposal stays in next_action/plan_text. Keep useful existing event-backed tasks when repairing a field; proposal alone does not establish delivery.

In a new conversation reload active instructions and prepare current memory; memory is independent of session choice. Explicitly start or resume the intended session; never guess which to mutate. A valid native ticket works with either transition. Resume retains the saved session ID. Read changed memories before reusing a diagnosis; do not submit known_revisions for absent text.

Check pending_evidence even when memory revisions are unchanged: captured work may await interpretation. Page session_choices and inspect relevant pending events before treating a saved diagnosis as latest. Reading a session does not resume, bind, assess, or process it. Qualify context you cannot inspect. Before independent review withhold answer-bearing recap. Unknown delivery is not learner failure; do not repeat teaching solely because a checkpoint is older.

Honor pause/stop immediately, even if persistence fails: control (below; an action, not an update field) stops this conversation; session.pause then stops the lesson in every conversation. “One more check then pause” permits that check and feedback, not another test. Resume and recording_on are distinct. Declining review creates no failure observation or debt. Disclose a save limit once; promise no restart durability.

<!-- executable-recipe: control_pause -->
```json
{"schema":"learner-model/v2","action":"control","request_id":"pause-1","control":"pause","turn_ref":"$CURRENT_EVENT_ID"}
```

## Pending raw chronology

pending_evidence counts are not assessments. Use pending_work for concise metadata, including earlier unbound input; follow the returned next_call unchanged. Read only relevant originals and retain origin, help, and uncertainty. Retrieval alone never processes work. Current controls take priority.

<!-- executable-recipe: pending_session_choices -->
```json
{"schema":"learner-model/v2","action":"session_choices"}
```

<!-- executable-recipe: pending_session_events -->
```json
{"schema":"learner-model/v2","action":"pending_events","session_id":"$PRIOR_SESSION_ID","limit":16}
```

Read selected originals through memory(read); follow next_call for remaining refs:

<!-- executable-recipe: pending_event_read -->
```json
{"schema":"learner-model/v2","action":"read","refs":[{"kind":"event","id":"$PENDING_EVENT_ID"}]}
```

## Complete captured work after pause

With recording on, mode:complete_captured may finish the existing learner turn_ref using ordinary observation/memory shapes and a guarded session revision. No new capture, start/resume, goal change, task creation/change, or review creation. Keep the host stopped. An updated diagnosis may preserve the identical pending_task or clear it with null. Omit checkpoint to preserve it entirely. Failure leaves work pending. recording_off forbids completion.

## Assign unbound work deliberately

<!-- executable-recipe: pending_unbound_work -->
```json
{"schema":"learner-model/v2","action":"pending_work","unbound_only":true,"limit":16}
```

Read relevant event refs first. start/resume associates new unbound capture or explicit selected_unbound_event_ids, never a thread backlog. Select a prior failed capture explicitly. turn_ref only references context; bound work retains its original lesson while supporting a new lesson's observations. Preserve host identity and capture time. Tutor delivery does not require a learner interpretation.

## Native resume

Use the saved session and actual revision; keep exact ticket/text. Already captured input needs only turn_ref. Forks resume; use fails there. New input IDs exist only in returned captured_event_ids; never cite placeholders.

<!-- executable-recipe: native_resume -->
```json
{"schema":"learner-model/v2","action":"update","request_id":"example-native-resume","native_capture":{"ticket":"$NATIVE_TICKET","text":"$EXACT_CURRENT_TEXT"},"session":{"operation":"resume","session_id":"$SAVED_SESSION_ID","expected_revision":1}}
```
