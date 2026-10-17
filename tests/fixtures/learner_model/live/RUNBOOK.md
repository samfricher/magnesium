# Installed-host learner scenarios

Inputs for manually exercising learner memory and teaching behavior through a
real installed Magnesium host. The package separates tutor, learner, and evaluator
roles. `prepare.py` only materializes inputs; it doesn't start a model or read
authentication data.

## Prepare a conversation

Build the ordinary release artifact and initialize a new disposable subject
workspace with `magnesium init`. Use the existing subscription-backed host connection
and installed Magnesium instructions/tools. Don't enable a development authority
override or add an API key/provider for the run.

From the repository root, select a case from `learner-cases.json`:

```sh
python3 tests/fixtures/learner_model/live/prepare.py \
  --case evolving-understanding --conversation 1 \
  --out /tmp/magnesium-live-evolving-1
```

`--conversation` is one-based. Use a new output directory for each conversation;
existing outputs aren't overwritten. The generated directories contain:

- `tutor/`: opening message, ordinary role instructions, and allowed sources
- `learner/`: role instructions, selected setup, and allowed sources
- `evaluator/`: private criteria and complete case information

Keep this package, generated driver/evaluator files, private labels, other cases,
and learner setup outside the tutor's workspace. Give the tutor only ordinary
installed instructions/tools, the opening message, and allowed subject resources.
Give the learner only its own setup, sources, and visible tutor messages. Give
the evaluator its inputs and the saved trace after the conversation.

## Run the scenario

1. Record the commit, binary hash/version, host/model/settings, store
   family/schema, and installed instruction hash.
2. Use a fresh host thread for every listed conversation. Reuse the initialized
   subject workspace within one `workspace_group`; use separate empty workspaces
   for different groups. Let Magnesium retrieval supply history instead of importing
   old transcripts into tutor prompts.
3. Send the opening and critical learner responses exactly as scripted. Other
   responses must follow the learner's knowledge and assistance constraints;
   don't steer the tutor toward a hidden diagnosis.
4. Stop at six learner messages, 24 tutor tool calls, or 12 minutes, whichever
   comes first. Context defaults to 24 KiB and must not exceed 64 KiB.

## Special setups

For small-directory recovery, use tutor-authored prior conversation only. Check
returned directory coverage and the actual search words before interpreting a
missed query. A stored record doesn't prove the tutor discovered it.

For large-directory recovery, synthetic seeds must be labeled retrieval-only
and created through the real `learning_step` tool. Keep all setup calls, limit
active memories to 48, and include no evaluator hints. Inspect serialized cards
to verify the 12 KiB automatic directory share is exceeded. If it isn't, leave
the pressure branch untested rather than changing thresholds or summaries.

The optional pending-write-failure branch requires a supported host pause/cancel
or concurrency boundary. After confirmed native capture, cause one disposable
store write failure, release it, and retain the failed result, pending event,
continuation, and reconciliation attempt. Don't change credentials, permissions,
existing workspaces, or production databases. If that boundary isn't available,
skip the fault branch and still exercise the ordinary controls.

## Save and evaluate

Keep traces and store snapshots outside the tutor's workspace. Preserve available
messages, tool requests/results and errors, returned memory cards and coverage,
source bytes/hashes, help exposure, event/observation origins, memory revisions,
and session checkpoints. Note missing trace fields; an emitted packet doesn't
prove the model received it.

Use the evaluator's role instructions and case criteria to validate the setup,
then produce a report matching `report-schema.json`. Distinguish PASS, FAIL,
NOT_RUN, and INVALID_SIMULATION. Preserve the first run. After a confirmed fix,
allow one affected-case rerun and one predetermined wording variation, retaining
all versions and identifying the changed commit. Keep reusable scenarios in the
repository and attach sanitized run results to the pull request, without private
user material or credentials. Scripted learners don't establish measured
human-learning outcomes.
