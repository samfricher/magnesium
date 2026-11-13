# Scenario transcript

The real Claude Code, 2026-11-12T23:17:06.918Z.

## Scenarios 2 to 4

**Learner** (a new lesson):

Sampling distributions

**Tutor**:

Let's start with a small case.

A population has just three values: 2, 4 and 6. You draw two values at random, **with replacement**, and take the mean of the two.

What is the probability that the sample mean is exactly 4?

*The plan changed: goal, steps.*

**Learner** (intent answer):

I think an average varies less than a single value does: its spread is the spread of one value divided by √n.

**Tutor**:

That is right, and it is the central fact here: the mean of $n$ independent values has spread $\sigma/\sqrt{n}$. Since you already have that, let's use it on something harder.

Delivery times at a restaurant have mean 50 minutes and standard deviation 12 minutes, and the distribution is strongly right-skewed. You take a random sample of 36 deliveries.

About what is the probability that the sample mean is above 54 minutes? And what allows you to use a normal curve here, when the times themselves are skewed?

**Learner** (intent talk):

I skipped three days and the midterm moved, change the plan

**Tutor**:

I can move it as soon as I know the new date: the plan still has Midterm 2 on Sunday 22 November, 10:00. What date is it on now?

The days you mention need no change of their own, because the route starts today with sampling distributions, which is where you are. Once I have the date, I'll re-space the five blocks to fit before it.

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

Sampling distributions is today's block, and the delivery-times question is still open whenever you want to go on with it.

*The plan changed: exam_date, items, text.*

**Learner** (intent talk):

By the way, why do we divide by n − 1 for the sample standard deviation?

**Tutor**:

Because the deviations are measured from the sample's own mean $\bar{x}$, not from the true mean $\mu$, and that makes them come out a little too small.

$\bar{x}$ is the point that sits closest to your particular sample: the sum of squared deviations from $\bar{x}$ is smaller than from any other point, $\mu$ included. So dividing that sum by $n$ underestimates the population variance on average. The shortfall is exactly a factor of $(n-1)/n$, and dividing by $n-1$ instead cancels it.

Another way to see the same thing is that the $n$ deviations from $\bar{x}$ always add to zero, so once you know $n-1$ of them, the last is fixed. Only $n-1$ of them carry free information about spread, which is what "degrees of freedom" means.

With $n = 1$ this is easy to see: a single value has deviation 0 from its own mean, yet it tells you nothing about spread, and $0/0$ says exactly that.

**Learner** (intent talk):

OK, back to the task.

**Tutor**:

Here it is again.

Delivery times at a restaurant have mean 50 minutes and standard deviation 12 minutes, and the distribution is strongly right-skewed. You take a random sample of 36 deliveries.

About what is the probability that the sample mean is above 54 minutes? And what allows you to use a normal curve here, when the times themselves are skewed?

## Scenario 5

**Learner** (a new lesson):

I'm back. Let's continue.

**Tutor**:

Here is the question we were on.

Delivery times at a restaurant have mean 50 minutes and standard deviation 12 minutes, and the distribution is strongly right-skewed. You take a random sample of 36 deliveries.

About what is the probability that the sample mean is above 54 minutes? And what allows you to use a normal curve here, when the times themselves are skewed?
