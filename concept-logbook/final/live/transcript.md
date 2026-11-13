# Live lesson transcript

The real Claude Code, on the Magnesium engine; the learner's answers were given by a person who read each task.

**Learner** (intent a new lesson):

Let's work on Midterm 2.

**Tutor**:

We start with sampling distributions, the first item on your plan.

A coffee machine fills cups with a mean of 250 ml and a standard deviation of 12 ml. You take 36 cups at random and average their volumes.

**Roughly how far would you expect that average to land from 250 ml: about 12 ml, about 2 ml, or about 0.33 ml?** Say in a few words why.

*The plan changed: goal, steps.*

**Learner** (intent answer):

About 2 ml. The spread of an average of n cups is the spread of one cup divided by the square root of n: 12 / sqrt(36) = 12 / 6 = 2 ml.

**Tutor**:

Right: dividing by $\sqrt{n}$ is the move, and 2 ml is the standard error. Now a jump ahead.

You don't trust the machine's label, so you measure 16 cups yourself: the sample mean is 246 ml and the sample standard deviation is 8 ml. For 15 degrees of freedom, $t^* = 2.131$ at 95%.

**Build the 95% confidence interval for the machine's true mean, and say whether 250 ml is still a believable value for it.**

*The record marked it: right, help none, cold, "standard error of a sample mean".*

**Learner** (intent talk):

I skipped three days and the midterm moved, change the plan

**Tutor**:

Happy to change it. What is the new midterm date? The plan currently says Sunday 22 November, 10:00 to 11:15.

Once I have the date I'll move the exam and re-spread the five items across the days you have. The skipped days don't cost you anything in the plan, since it just runs from today.

**Learner** (intent talk):

It's on Sunday 29 November now, same time.

**Tutor**:

Done: Midterm 2 is now on Sunday 29 November, 10:00 to 11:15, and the five items are spread over the 17 days you have.

| # | Item | Day |
|---|---|---|
| 1 | Sampling distributions | Thu 12 Nov (today) |
| 2 | Confidence intervals | Mon 16 Nov |
| 3 | Hypothesis tests | Fri 20 Nov |
| 4 | Tests and intervals together | Tue 24 Nov |
| 5 | Practice midterm | Fri 27 Nov |

The order and the 9 hours are unchanged; tell me if you want different days. The confidence interval question for the 16 cups is still open whenever you want to pick it up.

*The plan changed: exam_date, items, text.*

**Learner** (intent talk):

By the way, why do we divide by n − 1 for the sample standard deviation?

**Tutor**:

Because you measure the deviations from the sample mean $\bar{x}$, not from the true mean $\mu$, and that makes them a little too small.

$\bar{x}$ is computed from the same data, so it sits in the middle of your sample, closer to the points than $\mu$ usually is. The squared deviations from $\bar{x}$ therefore add up to less, on average, than they would from $\mu$. Dividing by $n - 1$ instead of $n$ makes up for exactly that shortfall: with it, the sample variance $s^2$ averages out to the true variance $\sigma^2$.

Another way to see the $n - 1$: the deviations from $\bar{x}$ always sum to zero, so once you know $n - 1$ of them, the last one is fixed. Only $n - 1$ pieces of free information about spread are left, which is why they are called degrees of freedom. It is the same 15 that goes with $t^*$ for your 16 cups.

The extreme case makes it concrete: with one cup, the deviation from $\bar{x}$ is 0, yet a single cup tells you nothing about spread. Dividing by $n$ would give 0, a confident wrong answer, while dividing by $n - 1$ gives $0/0$, which honestly says "no information".

**Learner** (intent talk):

OK, back to the task.

**Tutor**:

Here it is again.

You measure 16 cups: the sample mean is 246 ml and the sample standard deviation is 8 ml. For 15 degrees of freedom, $t^* = 2.131$ at 95%.

**Build the 95% confidence interval for the machine's true mean, and say whether 250 ml is still a believable value for it.**
