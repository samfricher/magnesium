---
name: checks
description: Take only the quick checks that are due, and turn Magnesium on for this session.
disable-model-invocation: true
---

# Just the quick checks

The learner wants the quick checks that are due, and nothing else today.

Open the nearest initialized workspace `.magnesium/INSTRUCTIONS.md` with the Read tool, and load `context(action:"instructions",host:"claude",stage:"review_and_transfer")`. Send learning_step `{"schema":"learner-model/v2","action":"just_checks"}` once, before your first reply, and follow what it returns:

- With checks to pose, pose the ones its `sitting_opener` lists, one at a time, and record each answer as it says. When no lesson was active here, Magnesium has started one named "Quick checks", so answers are recorded. Teach nothing new, set no paper redo and no other task. Magnesium ends the sitting when the last check is answered or declined, and shows the learner what their record says of today under your reply: never say it yourself, set no task and ask no question.
- With none, say in one line that no quick check is due now, and when the next is, as its note gives it. Set no task and ask no question.

If the learner then asks for more, teach as in any lesson. You cannot start another Magnesium skill yourself: for a practice paper, a plan or marking, tell them to type `/magnesium:papers` followed by their request.
