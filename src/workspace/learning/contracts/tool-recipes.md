# Tool recipes

Use actual $values/revisions and unique client_ref names. Do not drop the checkpoint on errors; guard its session.

## Start

native_capture:{ticket,text}: exact bytes, start/resume only. Start needs session.client_ref. Fallback:

<!-- executable-recipe: start_capture -->
```json
{"schema":"learner-model/v2","action":"update","request_id":"monday-start","session":{"operation":"start","client_ref":"monday_session","goal":"Coefficients"},"reported_events":[{"external_event_key":"monday-1","speaker":"learner","kind":"message","text":"x² − 5x + 6 = 0: a=1, b=−5, c=6; roots 2,3."}]}
```

$MONDAY_EVENT_ID=captured_event_ids[0]; $MONDAY_SESSION_ID=references.monday_session.

## Observe

<!-- executable-recipe: observe_create -->
```json
{"schema":"learner-model/v2","action":"update","request_id":"monday-interpret","observations":[{"client_ref":"monday_work","kind":"learner_work","description":"Correct coefficients and roots.","events":[{"event_id":"$MONDAY_EVENT_ID","role":"learner_response"}],"conditions_text":"One equation.","assistance_text":"Earlier help unknown."}],"memory_edits":[{"operation":"create","client_ref":"quadratic_memory","record":{"title":"Coefficients","scope":"Quadratics","summary":"One correct substitution.","state_text":"Monday: correct substitution; transfer untested.","current_basis":[{"new_observation":"monday_work"}]},"reason":"Record work.","change_basis":[{"new_observation":"monday_work"}]}],"event_dispositions":[{"event_id":"$MONDAY_EVENT_ID","outcome":"processed"}],"session":{"operation":"use","session_id":"$MONDAY_SESSION_ID","expected_revision":1},"checkpoint":{"current_problem":"Coefficients","working_diagnosis":"One success","next_action":"Confirm the correct work; ask why.","memory_refs":[{"memory_id":"quadratic_memory","revision":"after_edit"}]}}
```

Require committed. $MONDAY_OBSERVATION_ID=references.monday_work; $MEMORY_ID=references.quadratic_memory.

## Another conversation

Prepare context first.

<!-- executable-recipe: new_conversation_capture -->
```json
{"schema":"learner-model/v2","action":"update","request_id":"tuesday-start","session":{"operation":"start","client_ref":"tuesday_session","goal":"Coefficients"},"reported_events":[{"external_event_key":"tuesday-1","speaker":"learner","kind":"message","text":"2x²+7x+3=0: b is unknown x."},{"external_event_key":"tuesday-2","speaker":"learner","kind":"message","text":"x²−4x+1=0: b is x, not −4."}]}
```

$TUESDAY_EVENT_1_ID/$TUESDAY_EVENT_2_ID=captured_event_ids; $TUESDAY_SESSION_ID=references.tuesday_session.

<!-- executable-recipe: revise_memory -->
```json
{"schema":"learner-model/v2","action":"update","request_id":"tuesday-revise","observations":[{"client_ref":"tuesday_work","kind":"learner_work","description":"Twice calls b unknown x.","events":[{"event_id":"$TUESDAY_EVENT_1_ID","role":"learner_response"},{"event_id":"$TUESDAY_EVENT_2_ID","role":"learner_response"}],"conditions_text":"Two equations.","assistance_text":"No correction; other help unknown."}],"memory_edits":[{"operation":"revise","memory_id":"$MEMORY_ID","expected_revision":1,"record":{"title":"Coefficients","scope":"Quadratics","summary":"One formula success; b/x confusion.","state_text":"Monday: one success; transfer untested. Tuesday: b/x confusion. Next distinguish roles.","current_basis":[{"observation_id":"$MONDAY_OBSERVATION_ID"},{"new_observation":"tuesday_work"}]},"change_kind":"new_observation","reason":"Reconcile both days.","change_basis":[{"new_observation":"tuesday_work"}]}],"event_dispositions":[{"event_id":"$TUESDAY_EVENT_1_ID","outcome":"processed"},{"event_id":"$TUESDAY_EVENT_2_ID","outcome":"processed"}],"session":{"operation":"use","session_id":"$TUESDAY_SESSION_ID","expected_revision":1},"checkpoint":{"current_problem":"Coefficients","working_diagnosis":"b/x confusion","next_action":"Contrast b and x.","memory_refs":[{"memory_id":"$MEMORY_ID","revision":"after_edit"}]}}
```

## Reselect support

Select 1–8 supports, not history. Editorial $O1–$O8 must already support this memory; history remains.

<!-- executable-recipe: reselect_eight_supports -->
```json
{"schema":"learner-model/v2","action":"update","request_id":"reselect-support","session":{"operation":"use","session_id":"$SESSION_ID","expected_revision":1},"memory_edits":[{"operation":"revise","memory_id":"$MEMORY_ID","expected_revision":1,"record":{"title":"Coefficients","scope":"Quadratics","summary":"$CURRENT_SUMMARY","state_text":"$CURRENT_DESCRIPTION","current_basis":[{"observation_id":"$O1"},{"observation_id":"$O2"},{"observation_id":"$O3"},{"observation_id":"$O4"},{"observation_id":"$O5"},{"observation_id":"$O6"},{"observation_id":"$O7"},{"observation_id":"$O8"}]},"change_kind":"editorial","reason":"Reselect support.","change_basis":[{"observation_id":"$O8"}]}]}
```

## Preference

Preference: self_report, not learner_preference.

<!-- executable-recipe: observe_preference -->
```json
{"schema":"learner-model/v2","action":"update","request_id":"preference","observations":[{"client_ref":"preference","kind":"self_report","description":"Learner asks for one question at a time.","events":[{"event_id":"$EVENT_ID","role":"learner_response"}]}],"event_dispositions":[{"event_id":"$EVENT_ID","outcome":"processed"}]}
```

## No change

No reason field:

<!-- executable-recipe: no_change -->
```json
{"schema":"learner-model/v2","action":"update","request_id":"no-change","event_dispositions":[{"event_id":"$EVENT_ID","outcome":"no_change"}]}
```

## Image transcription

Use attachments/source_refs; fallback is model_reported.

<!-- executable-recipe: report_image_transcription -->
```json
{"schema":"learner-model/v2","action":"update","request_id":"image","reported_events":[{"external_event_key":"image-transcription-1","speaker":"learner","kind":"image_transcription","text":"x = 2","metadata":{"origin":"model_reported","attachments":[{"path":"$IMAGE_PATH","media_type":"image/png","available_form":"model_reported"}],"coverage":"Transcribed answer; other marks not verified."}}]}
```

## Failed capture

No proposal: use next_action/plan_text. Select captured input explicitly.

<!-- executable-recipe: planned_task_after_failed_start -->
```json
{"schema":"learner-model/v2","action":"update","request_id":"plan-repair","turn_ref":"$CURRENT_EVENT_ID","session":{"operation":"start","client_ref":"halves_session","goal":"$CURRENT_GOAL","selected_unbound_event_ids":["$CURRENT_EVENT_ID"]},"checkpoint":{"current_problem":"$CURRENT_PROBLEM","working_diagnosis":"$WORKING_DIAGNOSIS","next_action":"$PLANNED_NEXT_ACTION","plan_text":"$PLANNED_TASK_CONDITIONS_AND_HELP","pending_task":null},"event_dispositions":[{"event_id":"$CURRENT_EVENT_ID","outcome":"no_change"}]}
```

## Recovery

Unknown outcome: replay identical key/payload. Rejection: keep capture IDs/checkpoint; repair once, new key, no recapture. Disclose failure; cancellation is not rollback.

Omit max_bytes normally; follow error guidance without truncation or retry loops. Execute next_call unchanged. Controls: consolidate-and-resume.
