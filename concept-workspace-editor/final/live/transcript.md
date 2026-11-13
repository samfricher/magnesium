# Live lesson transcript

The real Claude Code, on the Magnesium engine; the learner's answers were given by a person who read each task.

**Learner** (intent a new lesson):

Sampling distributions

**Tutor**:

A bag holds three cards, numbered 2, 4 and 6. You draw one card, put it back, draw again, and take the mean of your two draws.

Which values can that mean take, and which value is the most likely?

*The plan changed: goal.*

**Learner** (intent answer):

The mean can be 2, 3, 4, 5 or 6. Of the 9 equally likely ordered draws, 1 gives 2, 2 give 3, 3 give 4 ((2,6), (6,2), (4,4)), 2 give 5 and 1 gives 6, so 4 is the most likely, with probability 3/9 = 1/3.

**Tutor**:

Right: counting the 9 ordered draws, rather than the 5 distinct means, is what gives 4 its probability of $1/3$. Step one, listing a sampling distribution, is done.

Now a much harder one. Waiting times at a call centre are strongly right-skewed, with mean 50 s and standard deviation 12 s. You take a random sample of 36 calls.

Roughly how likely is it that the sample's mean waiting time is above 53 s?

*The plan changed: steps.*

*The record marked it: right, help none, cold, "sampling distribution of the mean from a small population".*

**Learner** (intent talk):

I skipped three days and the midterm moved, change the plan

**Tutor**:

I can move it as soon as I know the new date: what day is Midterm 2 on now (and the time, if that changed too)?

Once you tell me, I'll set the date and re-space the five blocks to fit before it, starting from today with sampling distributions, which you're already partway into.

**Learner** (intent talk):

It's on Sunday 29 November now, same time.

**Tutor**:

Done: Midterm 2 is now on Sunday 29 November, 10:00 to 11:15, and the blocks are spread out to fit.

| Block | Day |
|---|---|
| Sampling distributions | today, 12 Nov |
| Confidence intervals | 16 Nov |
| Hypothesis tests | 20 Nov |
| Tests and intervals together | 24 Nov |
| Practice midterm | 27 Nov |

The hours are unchanged at 9. The call-centre question is still open whenever you want to pick it up.

*The plan changed: exam_date, items, text.*

**Learner** (intent talk):

By the way, why do we divide by n − 1 for the sample standard deviation?

**Tutor**:

Because the deviations are measured from the sample's own mean $\bar{x}$, not from the true mean $\mu$, and that makes them systematically too small.

$\bar{x}$ is the point that sits closest to your data: it minimises the sum of squared deviations. So $\sum (x_i - \bar{x})^2$ is always at most $\sum (x_i - \mu)^2$, and dividing it by $n$ would underestimate the population variance on average. The shortfall turns out to be exactly a factor of $\frac{n-1}{n}$, so dividing by $n-1$ instead cancels it, and the sample variance $s^2$ is then right on average.

Another way to see the same thing is that the deviations from $\bar{x}$ must sum to zero, so once you know $n-1$ of them, the last is fixed. Only $n-1$ of them carry free information about spread, which is what "$n-1$ degrees of freedom" means.

With $n = 1$ the point is obvious: one observation tells you nothing about spread, and the formula gives $0/0$ rather than a misleading 0.

**Learner** (intent talk):

OK, back to the task.

**Tutor**:

Here it is again.

Waiting times at a call centre are strongly right-skewed, with mean 50 s and standard deviation 12 s. You take a random sample of 36 calls.

Roughly how likely is it that the sample's mean waiting time is above 53 s?
