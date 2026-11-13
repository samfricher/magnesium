# Scenario transcript

The real Claude Code, 2026-11-12T22:30:48.817Z.

## Scenarios 2 to 4

**Learner** (a new lesson):

Sampling distributions

**Tutor**:

A bag holds three cards, numbered 2, 4 and 6. You draw one card, put it back, then draw again, and write down the mean of your two draws.

Which values can that mean take, and which value is the most likely?

*The plan changed: goal, steps.*

**Learner** (intent answer):

I think an average varies less than a single value does: its spread is the spread of one value divided by √n.

**Tutor**:

That's right, and it is where the cards were heading: with two draws the mean piles up at 4, and the extremes 2 and 6 become rare. So let's put your rule to work on something harder.

Bottles from a filling machine hold a mean of 500 ml with a standard deviation of 12 ml. You take a random sample of 36 bottles.

What is the probability that the sample's mean fill is above 503 ml? Show how you get there, and say what you are assuming about the shape.

**Learner** (intent talk):

I skipped three days and the midterm moved, change the plan

**Tutor**:

I can't make the change yet, because I don't know the new date: when is Midterm 2 now, and is the time still 10:00 to 11:15?

As soon as you tell me, I'll move the exam and re-space the five blocks to fit before it. As it stands, the plan has sampling distributions today and the practice midterm on 20 Nov, with the exam on 22 Nov.

**Learner** (intent talk):

It's on Sunday 29 November now, same time.

**Tutor**:

Done: Midterm 2 is now on Sunday 29 Nov, 10:00 to 11:15, and the blocks are spread out to fit before it.

| Block | Day |
|---|---|
| Sampling distributions | Thu 12 Nov (today) |
| Confidence intervals | Mon 16 Nov |
| Hypothesis tests | Fri 20 Nov |
| Tests and intervals together | Tue 24 Nov |
| Practice midterm | Fri 27 Nov |

The order and the 9 hours are unchanged. The bottle question is still open whenever you want to pick it up.

*The plan changed: exam_date, items, text.*

**Learner** (intent talk):

By the way, why do we divide by n − 1 for the sample standard deviation?

**Tutor**:

Because you measure the deviations from the sample's own mean $\bar{x}$, not from the true mean $\mu$, and that makes them come out too small.

$\bar{x}$ is computed from the same data, so it sits in the middle of those particular points. It is closer to them, in squared distance, than any other number, $\mu$ included. So $\sum (x_i - \bar{x})^2$ is always a little smaller than $\sum (x_i - \mu)^2$ would be. Dividing by $n$ would then underestimate the population variance on average, and dividing by $n-1$ corrects exactly that shortfall.

Another way to see it: the deviations from $\bar{x}$ must add to zero, so once you know $n-1$ of them, the last one is fixed. Only $n-1$ pieces of independent information about spread are left, which is what "degrees of freedom" means.

The extreme case makes it concrete. With $n = 1$ the single point equals its own mean, so the deviation is 0, yet one value tells you nothing about spread. Dividing by $n$ would report a spread of 0, while dividing by $n-1$ gives $0/0$, which correctly says "can't tell".

**Learner** (intent talk):

OK, back to the task.

**Tutor**:

Here it is again.

Bottles from a filling machine hold a mean of 500 ml with a standard deviation of 12 ml. You take a random sample of 36 bottles.

What is the probability that the sample's mean fill is above 503 ml? Show how you get there, and say what you are assuming about the shape.

## Scenario 5

**Learner** (a new lesson):

I'm back. Let's continue.

**Tutor**:

Picking up where you stopped, with the bottle question.

Bottles from a filling machine hold a mean of 500 ml with a standard deviation of 12 ml. You take a random sample of 36 bottles.

What is the probability that the sample's mean fill is above 503 ml? Show how you get there, and say what you are assuming about the shape.
