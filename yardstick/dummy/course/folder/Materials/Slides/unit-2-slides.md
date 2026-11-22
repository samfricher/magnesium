---
title: "Chance and Counting"
subtitle: "Unit 2: discrete random variables"
---

# A. Expected value

## Slide 1: A random variable and its table

A random variable X gives a number to each outcome. Its table lists each value x with its probability p(x). The probabilities are never negative and add to 1.

Example: a spinner pays 0, 2 or 10 dollars with probabilities 0.5, 0.4 and 0.1.

## Slide 2: Expected value from a table

The expected value is the long-run average: E[X] = sum of x p(x) over every value.

Spinner: E[X] = 0(0.5) + 2(0.4) + 10(0.1) = 1.8 dollars.

The expected value need not be a value X can take.

## Slide 3: Expected value of a function of X

To find E[g(X)], apply g to each value and keep the probabilities: E[g(X)] = sum of g(x) p(x).

Spinner: E[X²] = 0(0.5) + 4(0.4) + 100(0.1) = 11.6. Note that E[X²] is not (E[X])² = 3.24.

## Slide 4: Linearity of expectation

For any random variables X and Y and constants a, b: E[aX + b] = aE[X] + b and E[X + Y] = E[X] + E[Y]. Independence is not needed.

# B. Variance

## Slide 5: Variance from the definition

Variance measures spread around the mean µ = E[X]: Var(X) = E[(X − µ)²].

Spinner: µ = 1.8, so Var(X) = (0 − 1.8)²(0.5) + (2 − 1.8)²(0.4) + (10 − 1.8)²(0.1) = 1.62 + 0.016 + 6.724 = 8.36.

## Slide 6: The shortcut for variance

Var(X) = E[X²] − (E[X])². Spinner: 11.6 − 3.24 = 8.36, the same number with less work.

## Slide 7: Scaling and shifting

Var(aX + b) = a² Var(X). A shift b moves every value and the mean together, so the spread does not change. A scale a stretches every distance from the mean by a, and variance squares distances.

Spinner paid in cents with a 50 cent fee: Var(100X − 50) = 100² (8.36) = 83,600.

## Slide 8: Standard deviation and units

The standard deviation is the square root of the variance and has the units of X. SD(aX + b) = |a| SD(X).

# C. Named distributions

## Slide 9: Bernoulli trials

A Bernoulli trial has two outcomes, success with probability p and failure with probability 1 − p. For the indicator of success, the mean is p and the variance is p(1 − p).

## Slide 10: Binomial: counting successes in n trials

If X counts the successes in n independent Bernoulli trials with the same p, then X is Binomial(n, p) and P(X = k) = C(n, k) p^k (1 − p)^(n − k).

The three conditions: a fixed number of trials, independent trials, the same p on every trial.

## Slide 11: Binomial mean and variance

For X Binomial(n, p): E[X] = np and Var(X) = np(1 − p).

Ten free throws at p = 0.3: the mean is 3 and the variance is 2.1.

## Slide 12: Geometric: waiting for the first success

If N is the number of independent trials up to and including the first success, then N is Geometric(p) and P(N = k) = (1 − p)^(k − 1) p for k = 1, 2, 3 and so on.

The number of trials is not fixed, which is what separates a geometric count from a binomial one.

## Slide 13: Geometric mean and the memoryless property

For N Geometric(p): E[N] = 1/p. Waiting for a six on a fair die takes 6 rolls on average.

Memoryless: P(N > m + k given N > m) = P(N > k). After m failures, the wait starts again. The expected number of further trials is still 1/p.

# D. Indicators and sums

## Slide 14: Indicator variables

The indicator I of an event A is 1 when A happens and 0 when it does not. E[I] = P(A).

## Slide 15: Counting with indicators and linearity

Write a count as a sum of indicators, one per thing that might be counted. Then the expected count is the sum of the probabilities, even when the indicators are dependent.

Example: deal 5 cards from a 52 card deck. The expected number of aces is 5 × (4/52) = 5/13, although the cards are dependent.

## Slide 16: Variance of a sum of independent variables

If X and Y are independent, Var(X + Y) = Var(X) + Var(Y). Without independence this can fail: Var(X + X) = 4 Var(X), not 2 Var(X).
