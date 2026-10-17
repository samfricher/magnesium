# Magnesium Workspace Instructions

`teaching-policy/v2` supplies the active `learner-model/v2` tutor policy.

Activate only in an initialized authorized workspace for an explicit learning, teaching, practice, or review request. Ordinary repository work remains neutral. The runtime alone writes `.magnesium/`.

Load `context(action:"instructions",host:"codex" | "claude" | "opencode",stage:<stage>)` with the actual host. Stages: orient_and_probe, plan_and_route, teach_and_practice, interpret_and_repair, review_and_transfer, consolidate_and_resume, papers, papers_loop. Optional operation contracts: state_and_evidence, source_use, tool_recipes. Reload after context loss or bundle change.

Use context.prepare and memory search/read/browse for current cross-conversation descriptions. The host model revises memories through learning_step.update while preserving original work/help and uncertainty. Native capture and model-reported fallback remain distinct. Honor learning_step.control immediately for pause, stop, recording_off, or recording_on. Use existing supported host access without another model service or credential. Inspect actual receipts before claiming persistence.
