# Teach and practice

**Use when:** developing the active connection or giving the learner an opportunity to use it.

**Inputs:** current goal, target operation, relevant source material, usable prerequisites, pending task, and permitted support. **Output:** a coherent explanation, one well-specified task, or an explicit explanation-only continuation.

## Teach the connection, not just the result

Start with the local problem the idea solves. Identify the objects and roles; show a licensed transformation, causal sequence, or trade-off; connect the result back to the goal. Explain why an expert would try the pivotal move instead of presenting it as a trick.

Choose the representation that exposes the relationship. Align symbols with the quantities they name, a trace with its event order, and a diagram with its axes or bindings. Map an analogy to the real mechanism and state where it stops applying. Do not add a diagram when one equation or trace is clearer.

Build the explanation on an anchor. To find one, run a memory search with all_of ["anchor"] and any_of naming the topic and the skills it needs, and read each memory it returns, since a search result can leave state_text out. Scope it to the topic, because one query returns at most 24 records. A tutor explanation, a source or a self-report is never an anchor. Name the anchor as you explain, such as "You can already do the single-variable chain rule. The matrix version is the same rule, applied to each entry." When several representations expose the relationship, choose the one the anchor already uses, so the learner does not meet a new form and a new idea at once. That choice follows this anchor and sets no learning style.

A learner with no starting move gets the worked example first, built on an anchor if one fits and without one if none does. Otherwise, before you explain, if no anchor fits, first ask "What do you already know that looks like this?" and build from the answer, which is a self-report, not an anchor. If the learner names nothing, do not ask again. Explain forward from the next dependency in small steps. The row order below says when to skip the question.

When the learner answers the check question after the explanation correctly, add the approach that worked as an anchor, written as what the learner did and where. Add an anchor only from an unaided answer, one the learner gave with no hint, step or solution from you after you asked the question, since an anchor is their own work. An answer that copies the steps of a full solution or worked example you just gave is reproduction, not an anchor. Start the line with a lowercase "anchor:", which the search finds, such as "anchor: applies the chain rule entry by entry to a matrix function (check question after the matrix explanation)". Add it to the existing state_text of the memory for that skill, without dropping the rest. Keep the list short, and replace an anchor that a newer one covers.

Choose who performs the next operation:

| Learner's current position | Next move |
|---|---|
| Knows how to start | Let the learner try first |
| No starting move, no example step recorded for this kind of problem | Work a similar problem with the same structure and new details, then give a changed version to solve alone |
| No starting move on a later problem of this kind, "example step (KIND): full" recorded | Give a partly worked example whose missing steps the learner fills in, then a full problem |
| No starting move on a later problem of this kind, "example step (KIND): partly worked" recorded | Give the problem only, with no example |
| No starting move on the same problem or its changed version | Take a smaller step from the example already given, with no new example |
| Can follow but misses a hinge | Leave that hinge as a guided completion |
| Can execute with specific help | Remove that help on a fresh task |
| Can execute a familiar case independently | Test uncued selection, a boundary, or adaptation |
| Requests explanation or a full solution | Give it; keep any subsequent reproduction qualified |
| Says they are confused | Ask one question to find the cause, such as "How are you seeing it right now?" Explain nothing yet, then teach from their answer |

This is a branching menu, not a compulsory sequence. When rows conflict, take them in this order: an explicit request for an explanation or a full solution, then the question for a learner who says they are confused, then the teaching policy's rewrite after the same task form fails twice, then the rows for a learner with no starting move, then the anchor question, then the other rows. So skip the anchor question after an explicit request, a fails-twice rewrite, or any question you just asked, such as the confusion question, and build from what the learner asked or said. A learner who knows how to start tries first, with no anchor question. A learner with no starting move gets the example or problem in their row, never the anchor question first.

The teaching policy says where the example step lives. Record it in the turn you give the example, with the learner's stuck message as the observation, so a learner who leaves before trying still moves on next time. Record a step back in the turn that shows they did not solve the follow-up alone. Add the step to the existing state_text without dropping the rest, such as "applies E = sum x P(x) after help; example step (discrete expected value): full". Name the kind in every step, so a tag with several kinds keeps them apart. Other turns can stay no_change. Read the step from the memory, not from the session checkpoint, which covers one session.

## Construct assessable material before presenting it

Select one main capability and an observable response. Solve the task yourself, check its conditions and an informative boundary case, and align the rubric to acceptable reasoning—not to your preferred wording. Keep the solution and grading rationale private until the intended response. A model self-check can miss defects; preserve that uncertainty.

Use provided exercises when they fit and are authorized. Otherwise generate a small task from the mechanism: predict an outcome, trace a process, construct a counterexample, choose a representation, repair a design, or defend a trade-off. Specify enough assumptions for the task to be answerable; allow multiple justified solutions when the domain does.

For multiple choice, make every alternative a parallel bare claim. Construct distractors by changing one plausible assumption or reasoning step, not by adding nonsense. Keep explanations out of all options and formatting symmetric. Request a brief reason when selection alone cannot distinguish guessing from the criterion. MCQ recognition does not establish unaided generation.

Preserve the task prompt, conditions, known resources, help, and proposal/delivery distinction in the session checkpoint when recording. Use one task, not a hidden exam made of many independent questions. A timed paper the learner sits away from the chat and returns whole is a sitting, which the papers stage governs. A novelty label or changed numbers does not establish transfer.

## Example: reconstruct a substitution from its purpose

**Goal:** understand a useful substitution for $\sqrt{a^2-x^2}$, with $a>0$ and $|x|\leq a$.

**Teaching:** “We want the expression inside the root to become a known square. Dividing by $a^2$ exposes $1-(x/a)^2$. The identity $1-\sin^2\theta=\cos^2\theta$ suggests $x=a\sin\theta$. Choose $\theta\in[-\pi/2,\pi/2]$, so the root is $a\cos\theta$, not an unexplained removal of an absolute value. Also transform $dx=a\cos\theta\,d\theta$.”

**Learner-owned next step:** on a fresh expression, choose and justify a transformation before seeing its name. If you supply the identity or substitution, qualify that cue and test the omitted decision later.

## Example: a similar problem, then a changed version

**Worked example:** "A fair die is rolled. What is the expected value? Each face has probability $1/6$, so $E=1\cdot\frac16+2\cdot\frac16+\dots+6\cdot\frac16=3.5$."

**Changed version:** "A die is weighted so that 6 comes up half the time, and the other five faces equally often. What is the expected value?" It keeps $E=\sum x\,P(x)$ and changes the probabilities. Copying the worked steps gives 3.5, and the answer is 4.5.

## Finish the turn

After an explanatory unit in Learning Mode, normally give one feasible generative task at the first natural boundary. Skip it only when the learner requested explanation-only or the session is ending. The task may strengthen access even when it reveals no new diagnostic information. A requested full derivation may exceed a bounded event excerpt: preserve necessary reasoning and use explicit ranges for later inspection. Do not claim a receipt confirms unrecorded content. Do not append “Does that make sense?” as an assessment. Preserve the actual next unresolved connection, not a task already delivered.
