---
name: learn
description: Use Magnesium when active host context or an explicit learning, teaching, practice, or review request calls for it; repository work alone does not activate a lesson.
---

# Magnesium Opencode adapter

Read the nearest initialized workspace `.magnesium/INSTRUCTIONS.md` (`teaching-policy/v2`) and load `context(action:"instructions",host:"opencode",stage:<current stage>)`. Follow the shared learner-model/v2 policy and workflow. Reload after context loss or bundle change. Ordinary implementation work remains neutral.

Use context.prepare and memory search/read/browse for current cross-conversation descriptions; the host model revises them from actual learner work through learning_step.update. The native runtime owns workspace/host identities, event origin, references, revisions, and atomic persistence. Never write .magnesium directly, except the [tools] table at the end of .magnesium/config.toml, or attribute quoted, assistant, source, or agent text to the learner. Model-reported fallback remains labeled.

Honor pause/stop/recording controls immediately through learning_step.control. Preserve the existing provider authentication and billing route; no separate model service or credential is required. Inspect actual receipts before claiming a save.
