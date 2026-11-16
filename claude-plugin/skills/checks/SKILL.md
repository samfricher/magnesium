---
name: checks
description: Just the quick checks in a folder set up with Magnesium (it has a .magnesium directory). Use for /magnesium:checks, and when the learner asks for only the quick checks that are due and nothing else, for example "just the checks", "only my quick checks", "just the questions from earlier", "a few minutes of checks, nothing new". Not when they also want a lesson, a plan or a paper.
---

# Just the quick checks

The learner wants the quick checks that are due, and nothing else today.

Open the nearest initialized workspace `.magnesium/INSTRUCTIONS.md` with the Read tool, and load `context(action:"instructions",host:"claude",stage:"review_and_transfer")`. Send learning_step `{"schema":"learner-model/v2","action":"just_checks"}` once, before your first reply, and follow what it returns:

- With checks to pose, its `sitting_opener` is this sitting's whole plan. Start or resume the Magnesium session as the instructions say, so each answer can be recorded, then pose the checks one at a time. Teach nothing new, set no paper redo and no other task. Magnesium ends the sitting when the last check is answered or declined, and shows the learner what their record says of today under your reply: never say it yourself, set no task and ask no question.
- With none, say in one line that no quick check is due now, and when the next is, as its note gives it. Set no task and ask no question.

If the learner then asks for more, teach as in any lesson.
