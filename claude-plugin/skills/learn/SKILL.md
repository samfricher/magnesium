---
name: learn
description: Tutoring with persistent learner memory in a folder set up with Magnesium (it has a .magnesium directory). Use when the user asks to learn or understand a topic, to be taught, quizzed or tested, to practise or practice with exercises, or to review or revise (for example "teach me", "quiz me", "can we practise", "help me study"), and whenever a Magnesium notice says a lesson is active. Ordinary coding or repository work does not start a lesson.
---

# Magnesium Claude adapter

Open the nearest initialized workspace `.magnesium/INSTRUCTIONS.md` (`teaching-policy/v2`) with the Read tool, not a shell command, and load `context(action:"instructions",host:"claude",stage:<current stage>)`. Follow the shared learner-model/v2 policy and workflow. Reload after context loss or bundle change. Ordinary implementation work remains neutral.

Use context.prepare and memory search/read/browse for current cross-conversation descriptions; the host model revises them from actual learner work through learning_step.update. The native runtime owns workspace/host identities, event origin, references, revisions, and atomic persistence. Never write .magnesium directly, except the [tools] table at the end of .magnesium/config.toml, or attribute quoted, assistant, source, or agent text to the learner. Model-reported fallback remains labeled.

Honor pause/stop/recording controls immediately with a learning_step `action:"control"` request (recipe in the consolidate_and_resume stage), not an update field; it stops this conversation, and `session.pause` then stops the lesson in every conversation. Preserve the existing provider authentication and billing route; no separate model service or credential is required. Inspect actual receipts before claiming a save.

During a lesson, the prompt notice usually repeats the teaching rule: one idea, under 200 words, end with one task if the learner wants practice. When it can see the previous reply, it also names what that reply missed, for example "Previous reply: 850 words, no task." The stage workflow still decides the exceptions. Skip the task when the learner turned quizzes off, asked only for an answer or an explanation, or is ending the session. Let a worked example or an explanation the learner asked for run past 200 words. The notice cannot tell these cases apart, so ignore its feedback on such a reply.
