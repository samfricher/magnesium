# Scenario transcript

The real Claude Code, 2026-11-12T20:31:21.941Z.

## Scenarios 2 to 4

**Learner** (a new lesson):

Let's work on Sampling distributions.

**Tutor**:

Let's start with a concrete case.

A bag holds three cards numbered 2, 4 and 6. You draw one card, put it back, draw again, and take the mean of your two draws.

Which values can that mean take, and which one is most likely?

*The plan changed: goal, steps.*

**Learner** (intent answer):

I think an average varies less than a single value does: its spread is the spread of one value divided by √n.

**Tutor**:

That's right, and it's the centre of this topic: the mean of $n$ independent draws has spread $\sigma/\sqrt{n}$. That covers the first step, so I'll skip the cards and put the rule to work on something harder.

Bags of flour from a machine have weights that are roughly normal, with mean 500 g and standard deviation 100 g. You take a random sample of 25 bags.

Roughly what is the probability that the sample's mean weight is above 540 g?

*The plan changed: steps.*

**Learner** (intent talk):

I skipped three days and the midterm moved, change the plan

**Tutor**:

I can change it right away, but I need one thing: what is the midterm's new date?

Once I have it, I'll move the exam and spread the five blocks over the days left, starting from sampling distributions, where you are now. Tell me too if you'd like a different order or want to drop something.

**Learner** (intent talk):

It's on Sunday 29 November now, same time.

**Tutor**:

Done: Midterm 2 is now on Sunday 29 November, 10:00 to 11:15, and the blocks are spread over the 17 days you have.

| Block | Day |
|---|---|
| Sampling distributions | today, 12 Nov |
| Confidence intervals | 16 Nov |
| Hypothesis tests | 20 Nov |
| Tests and intervals together | 24 Nov |
| Practice midterm | 27 Nov |

The order and the 9 hours are unchanged. The days are my choice, so say if any of them doesn't suit you.

The flour-bag question is still open whenever you want to pick it up.

*The plan changed: exam_date, items, text.*

**Learner** (intent talk):

By the way, why do we divide by n − 1 for the sample standard deviation?

**Tutor**:

Because the deviations are measured from the sample's own mean $\bar{x}$, not from the true mean $\mu$, and that makes them come out a little too small.

- **Why too small:** $\bar{x}$ is computed from the same data, so it sits in the middle of your sample. The squared deviations from $\bar{x}$ are smaller than those from any other point, including $\mu$. Dividing their sum by $n$ would therefore underestimate the population variance on average.
- **Why exactly $n-1$:** the deviations from $\bar{x}$ always add up to zero, so once you know $n-1$ of them, the last one is fixed. Only $n-1$ of them carry free information about spread. Dividing by $n-1$ makes the sample variance right on average: $E[s^2] = \sigma^2$.

A quick case: with $n = 1$, your single value equals $\bar{x}$, so its deviation is 0. Dividing by $n$ would give a spread of 0, which is nonsense. Dividing by $n-1$ gives $0/0$, which correctly says that one value tells you nothing about spread.

The effect matters most for small samples. At $n = 5$ the two divisors differ by 20%, while at $n = 500$ they hardly differ at all.

**Learner** (intent talk):

OK, back to the task.

**Tutor**:

Here it is again.

Bags of flour from a machine have weights that are roughly normal, with mean 500 g and standard deviation 100 g. You take a random sample of 25 bags.

Roughly what is the probability that the sample's mean weight is above 540 g?

## Scenario 5

**Learner** (a new lesson):

I'm back. Let's continue.

**Tutor**:

Picking up where we stopped, on the spread of a sample mean.

Bags of flour from a machine have weights that are roughly normal, with mean 500 g and standard deviation 100 g. You take a random sample of 25 bags.

Roughly what is the probability that the sample's mean weight is above 540 g?
