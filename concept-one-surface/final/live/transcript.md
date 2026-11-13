# Live lesson transcript

The real Claude Code, on the Magnesium engine; the learner's answers were given by a person who read each task.

**Learner** (intent a new lesson):

Let's work on Sampling distributions.

**Tutor**:

Let's start with a small case.

A bag holds three tickets numbered **1, 3, 5**. You draw one ticket, put it back, and draw again, then take the average of your two numbers.

What is the probability that the average is exactly 3? Show how you got it.

*The plan changed: goal, steps.*

**Learner** (intent answer):

1/3. With replacement there are 3 x 3 = 9 equally likely ordered draws. The average is 3 when the sum is 6: (1,5), (5,1) and (3,3), so 3 of the 9.

**Tutor**:

Right: 3 of 9 equally likely ordered draws gives 1/3. Listing every ordered sample and counting the ones that give the average is exactly how a sampling distribution is built, so that first step is done.

Next case: a machine fills bags of rice. Individual bag weights have mean 500 g and standard deviation 12 g. You weigh 36 randomly chosen bags and take their average.

If you repeated that many times, what would the standard deviation of those averages be?

*The plan changed: steps.*

*The record marked it: right, help none, cold, "building the sampling distribution of the mean from a small population".*

**Learner** (intent talk):

I skipped three days and the midterm moved, change the plan

**Tutor**:

Skipping days is fine; the plan is a route you can redraw, and we go on from where you are now.

To move the exam I need one thing: **what is Midterm 2's new date** (and time, if that changed too)? The plan currently has it on Sun 22 Nov, 10:00 to 11:15, and I haven't changed anything yet.

Once you tell me, I'll update the date and spread the five blocks (sampling distributions, confidence intervals, hypothesis tests, tests and intervals together, practice midterm) over the days you actually have.

**Learner** (intent talk):

It's on Sunday 29 November now, same time.

**Tutor**:

Done: Midterm 2 is now on Sun 29 Nov, 10:00 to 11:15, and the later blocks are spread over the extra week.

| Block | Day |
|---|---|
| Sampling distributions | today, 12 Nov |
| Confidence intervals | 16 Nov |
| Hypothesis tests | 20 Nov |
| Tests and intervals together | 24 Nov |
| Practice midterm | 27 Nov |

The order and the 9 hours are unchanged; say so if you want different days.

You're on sampling distributions now, and the rice-bag question is still open whenever you want to pick it up.

*The plan changed: exam_date, items, text.*

**Learner** (intent talk):

By the way, why do we divide by n − 1 for the sample standard deviation?

**Tutor**:

Because the deviations are measured from the sample's own mean, and that makes them a little too small.

- **The cause.** The sample mean $\bar{x}$ is the point that sits closest to your data, in the sense that it makes the sum of squared deviations as small as it can be. The true mean $\mu$ is almost never exactly at $\bar{x}$, so the squared deviations from $\bar{x}$ come out smaller than those from $\mu$ would.
- **The fix.** Dividing that too-small sum by $n$ would underestimate the population variance on average. Dividing by $n-1$ enlarges it by exactly the right amount: over many samples, $s^2 = \dfrac{\sum (x_i-\bar{x})^2}{n-1}$ averages out to the true $\sigma^2$.
- **Why exactly $n-1$.** The deviations from $\bar{x}$ always sum to zero, so once you know $n-1$ of them, the last one is fixed. Only $n-1$ of them are free to vary, which is what "degrees of freedom" means.

With $n=1$ there is one value and no information about spread at all; dividing by $n-1=0$ leaves $s$ undefined, where dividing by $n$ would claim the spread is 0.

One caveat: the $n-1$ makes $s^2$ right on average for the variance; $s$ itself is still slightly low on average, though this matters little in practice.

The rice-bag question is still open when you want to go back to it.

**Learner** (intent talk):

OK, back to the task.

**Tutor**:

Here it is again:

A machine fills bags of rice. Individual bag weights have mean 500 g and standard deviation 12 g. You weigh 36 randomly chosen bags and take their average.

If you repeated that many times, what would the standard deviation of those averages be?
