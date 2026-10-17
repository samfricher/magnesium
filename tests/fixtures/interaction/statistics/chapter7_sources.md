# Synthetic Chapter 7 source: sample means, discrete counts, and cell boundaries

Original synthetic teaching text. It contains no private slides, transcript,
course material, or personal data. Numbers were computed independently for the
fixture; the source is not authoritative for any real course.

## Outline

1. Random samples and the sample mean.
2. The central limit theorem for sample means.
3. What the theorem does and does not claim.
4. Discrete counts: Bernoulli indicators and the Binomial family.
5. Standardization as an exact rewrite.
6. The Normal replacement as an approximation.
7. Continuity correction and cell boundaries.
8. Why align boundaries halfway between attainable values.
9. Worked examples.
10. Error sources in the cell argument.
11. Scope limits and common mistakes.
12. Exercises.

Standing notation: a random variable is written with a capital letter, a
realized value with the matching lower-case letter. `E` is expectation, `Var`
is variance, `SD` is standard deviation, and `Phi` is the standard Normal CDF.
`Phi(z) = P(Z <= z)` for a standard Normal `Z`. The symbol `~` means
"distributed as". `Bin(n, p)` is the Binomial family for `n` independent
trials with success probability `p`. All logarithms in the imported lemma are
natural logarithms.

## 1. Random samples and the sample mean

**Definition 1.1 (random sample).** `X_1, ..., X_n` is a random sample if the
`n` variables are independent and each has the same distribution. The shared
distribution is the population or parent distribution; `n` is the sample size.

**Definition 1.2 (sample mean).** For a random sample,
`X_bar = (X_1 + ... + X_n) / n`.

**Theorem 1.3 (mean and variance of the sample mean).** If the parent
distribution has mean `mu` and finite variance `sigma^2`, then
`E[X_bar] = mu` and `Var(X_bar) = sigma^2 / n`.

*Why.* Expectation is linear, so the mean of the sum is `n mu`; dividing by
the constant `n` gives `E[X_bar] = mu`. Independence makes the variance of the
sum the sum of the variances, `n sigma^2`; dividing by `n^2` gives
`Var(X_bar) = sigma^2 / n`. Only finite variance and independence are used.

Three distinct quantities must not be conflated:

- `X_i`, one observation;
- `X_bar`, the average of the `n` observations in one sample;
- the sampling distribution of `X_bar`, the distribution over repeated
  independent samples. `n` counts observations per sample. The number of
  repeated samples, written `R` in simulation, counts how many averages are
  computed; increasing `R` sharpens a simulated description of the fixed
  sampling distribution and does not change it.

## 2. The central limit theorem for sample means

**Theorem 2.1 (central limit theorem).** Let `X_1, ..., X_n` be a random
sample with mean `mu` and finite positive variance `sigma^2`. Define the
standardized sample mean

    Z_n = (X_bar - mu) / (sigma / sqrt(n)).

As `n` grows, the CDF of `Z_n` at every real `z` approaches `Phi(z)`. In
distribution, `Z_n -> Z` with `Z` standard Normal. Equivalently, for large
`n` the distribution of `X_bar` is close to `Normal(mu, sigma/sqrt(n))`, where
the second argument is the standard deviation.

**Conditions.** Independence, identical distribution, and finite positive
variance. This route does not require an MGF or any other transform to be
finite near zero. A proof through moment generating functions needs its own
existence and interchange conditions; those conditions are not implied by
finite variance alone. Presenting an MGF proof is one route, not a statement
that finite variance is insufficient. Characteristic functions are an
alternative route when a scope genuinely needs them; they are not a mandatory
detour for the sample-mean theorem.

## 3. What the theorem does and does not claim

The theorem is a limit statement. It does not supply a universal finite-`n`
error bound, and it does not promise that the Normal approximation improves
monotonically in `n` for every event and every parent distribution. Skew and
heavy tails degrade the approximation, especially in the tails. Any finite-`n`
error statement needs additional assumptions and an explicit error control.

## 4. Discrete counts: Bernoulli indicators and the Binomial family

**Definition 4.1 (Bernoulli indicator).** A Bernoulli(`p`) variable takes the
value `1` with probability `p` and `0` with probability `1 - p`. Its mean is
`p` and its variance is `p(1 - p)`.

**Definition 4.2 (Binomial count).** If `Y` is the number of successes in `n`
independent Bernoulli(`p`) trials, then `Y ~ Bin(n, p)` and

    P(Y = j) = C(n, j) p^j (1 - p)^(n - j),  j = 0, 1, ..., n,

where `C(n, j)` is the binomial coefficient. `E[Y] = n p` and
`Var(Y) = n p (1 - p)`.

Two structural facts matter for approximation:

- The attainable values of `Y` are the integers `0, 1, ..., n`; they are
  **discrete**, and the spacing between adjacent attainable values is `d = 1`.
  Standardization rescales the axis; it does not make `Y` continuous and does
  not change its support.
- The CDF event `{Y <= k}` includes the attainable value `k` and excludes
  `k + 1`. The largest included point on the real line is exactly `k`, but the
  event region as a probability mass ends where the last included mass sits.

## 5. Standardization as an exact rewrite

Let `mu = E[Y]`, `sigma = SD(Y) > 0`, and define the standardized coordinate

    Z = (Y - mu) / sigma.

For any real `k`, the event `{Y <= k}` is exactly the event
`{Z <= (k - mu)/sigma}`. This is a rewrite of the same event under an
invertible affine change of coordinate. When `Z` is defined from `Y`, writing
`P(Y <= k) = P(Z <= z_k)` with `z_k = (k - mu)/sigma` is exact. Nothing
probabilistic has been approximated at this step.

The approximation enters only when the exact distribution of `Z` is replaced
by the standard Normal distribution. That replacement is legitimate as an
approximation under the conditions of Theorem 2.1; it is not an identity, and
it must not be moved silently across an equals sign.

## 6. The Normal replacement as an approximation

For `Y ~ Bin(n, p)` with `sigma^2 = n p (1 - p)` and `z_k = (k - mu)/sigma`,
the plain Normal approximation is `P(Y <= k) approx Phi(z_k)`. The error of
this replacement is not controlled by the standardization step; it is the
separate question of how close the standardized Binomial CDF is to `Phi` at
the requested point, including the accumulated behavior over the index set
`{0, ..., k}` and the lower tail.

## 7. Continuity correction and cell boundaries

Each attainable value `j` receives a **cell** of width `d` centered on it:

    cell(j) = [j - d/2, j + d/2].

Every real point belongs to exactly one cell for spacing `d`, and the cells
tile the line. For the Binomial lattice, `d = 1`, so `cell(j) = [j - 1/2,
j + 1/2]`.

The event `{Y <= k}` is a union of cells `cell(0), ..., cell(k)`. Its outer
boundary is the right endpoint `k + d/2`; the next attainable value `k + 1`
is excluded. The continuity-corrected approximation evaluates the smooth
distribution at that outer boundary:

    P(Y <= k) approx Phi((k + d/2 - mu) / sigma) = Phi(z_k + h/2),

with `h = d / sigma`. For the Binomial case `d = 1` and `h = 1 / sigma`, so
the corrected argument is `(k + 0.5 - mu) / sigma`.

**Units and support.** In the original coordinate the half-cell is `d/2`. A
standardization by a positive SD `sigma` divides lengths by `sigma`: the
standardized grid spacing is `h = d / sigma` and the standardized half-cell
is `h / 2 = d / (2 sigma)`. Standardizing does not change the attainable
values; `Y` still takes only `0, ..., n`.

## 8. Why align boundaries halfway between attainable values

Two questions must stay separate.

1. Why may the standardized Binomial shape be approximated by a Gaussian
   curve at all?
2. Given a smooth approximation, why should event boundaries sit halfway
   between adjacent attainable values?

The second question can be taught conditionally on the first, without
pretending that it proves the first. The half-cell alignment is a statement
about which area under the smooth curve represents a lattice probability
mass; it does not establish that the smooth curve is the right one.

A local reading: the probability of one attainable value `j`,
`P(Y = j)`, is a point mass on the lattice, while the smooth curve assigns
area `integral over cell(j) of phi` to the same cell. Matching the mass to the
area of its centered cell is a cell-area approximation. For a density `g`
that is twice differentiable on one cell with `|g''| <= M`, the midpoint rule
on the centered cell satisfies

    | integral over cell of g  -  h g(midpoint) | <= M h^3 / 24.

The first-order (linear) term cancels on a symmetric cell, which is why the
centered choice is the natural one; the leading remaining error is quadratic
in the cell width. State the differentiability and boundedness assumption: it
is not a universal finite-`n` Binomial error guarantee, and it does not by
itself bound the total CDF error.

## 9. Worked examples

### Example 9.1 (central limit theorem for a sample mean)

Let `X_1, ..., X_36` be Uniform(0, 1), so `mu = 1/2` and `sigma^2 = 1/12`.
Then `Var(X_bar) = (1/12)/36 = 1/432`, so `SD(X_bar) = 1/sqrt(432) = 0.0481125...`.
The standardized value of `0.55` is

    z = (0.55 - 0.5) / 0.0481125... = 1.039230...,

and `Phi(1.039230...) = 0.850651...`. The theorem supports the approximate
statement `P(X_bar <= 0.55) approx 0.8507` for this `n`. The bound is not
exact, and no finite-`n` error bound follows from the limit theorem alone.

### Example 9.2 (normal approximation to a discrete count)

Let `Y ~ Bin(20, 0.4)`, so `mu = 8` and `sigma^2 = 20(0.4)(0.6) = 4.8`,
`sigma = 2.190890...`. Take `k = 8`. The exact CDF is

    P(Y <= 8) = sum over j = 0..8 of C(20, j) 0.4^j 0.6^(20 - j)
              = 0.5955987253122249 (to displayed precision).

The standardization rewrite gives `z_8 = (8 - 8)/2.190890... = 0`. The
uncorrected Normal value is `Phi(0) = 0.5`, which is visibly low because it
cuts the mass of `Y = 8` in half. The corrected value aligns the event at the
outer cell boundary `k + 0.5 = 8.5`:

    Phi((8.5 - 8)/2.190890...) = Phi(0.228217...) = 0.5902615116112394.

The corrected value is closer to the exact `0.5956`; the remaining gap is not
removed by the correction, and that gap has several separate sources
(Section 10). A cell table makes the boundary concrete:

| attainable value j | cell(j)      | standardized cell (z_j +/- h/2) |
| ------------------ | ------------ | ------------------------------- |
| 7                  | [6.5, 7.5]   | [-0.685, -0.228]                |
| 8                  | [7.5, 8.5]   | [-0.228,  0.228]                |
| 9 (excluded)       | [8.5, 9.5]   | [ 0.228,  0.685]                |

The event ends at `8.5`, which is the right edge of the cell for `j = 8`.

### Example 9.3 (midpoint cancellation with explicit numbers)

Take `g(x) = 4 x^2` on the centered cell `[-0.25, 0.25]`, so `h = 0.5` and
`g''(x) = 8 = M`. The exact integral is `a h^3 / 12 = 4(0.125)/12 =
0.041666...`; the midpoint value is `h g(0) = 0`. The error is therefore
`0.041666...`, which equals `M h^3 / 24 = 8(0.125)/24`. The bound is attained
for a pure quadratic; for a general smooth `g` it is an upper bound, not an
estimate of the actual error.

A linear term does not contribute to the midpoint error on a symmetric cell:
`integral over [-h/2, h/2] of (a x + b) dx = b h` and the midpoint value is
also `b h`, so the linear coefficient cancels exactly.

## 10. Error sources in the cell argument

Write `p_j = P(Y = j)`, `z_j = (j - mu)/sigma`, and `h = 1/sigma` for the
Binomial lattice. The corrected comparison decomposes as

    F_Y(k) - Phi(z_k + h/2)
      = sum over j = 0..k of ( p_j - h phi(z_j) )
      + sum over j = 0..k of ( h phi(z_j) - integral over cell(j) of phi )
      - Phi(z_0 - h/2).

Read the three terms separately.

1. **Local probability-mass error:** `p_j - h phi(z_j)` compares each lattice
   mass with the height of the smooth curve at the same coordinate. Near the
   center the mass sits below the curve because the lattice mass is thinner
   than the local density would suggest once neighboring cells are counted.
2. **Quadrature error:** `h phi(z_j) - integral over cell(j) of phi` is the
   midpoint-rule error on the cell. On a centered cell it is second order in
   `h` when the density is twice differentiable; the linear contribution
   cancels.
3. **Lower-tail extension:** `-Phi(z_0 - h/2)` is the mass the smooth curve
   places below the first cell boundary. It does not vanish and is not part
   of either sum.

Consequences that matter for any claim about the correction:

- A pointwise bound on the midpoint term is **not** a bound on the sum. The
  local mass errors can accumulate: even when each `|p_j - h phi(z_j)|` is
  small, the partial sum over `j = 0..k` can exceed the largest single term,
  and the partial sum grows with the number of included cells before it can
  turn around. Summing a pointwise approximation over an expanding index set
  therefore requires controlling the accumulated error and the lower tail,
  not just one cell.
- Bounding the midpoint term alone leaves the local mass term and the lower
  tail uncontrolled; a small quadrature error does not imply a small total
  CDF error.
- For a finite `n` Binomial, no universal guarantee follows from this
  argument that the correction improves every tail probability. Skew and the
  far tail can reverse the comparison. The honest statement is conditional:
  under the stated smoothness and tail control, the correction tracks the
  leading cell-area effect.

The factorial-to-Gaussian step is sometimes named rather than derived.
Stirling's approximation, `n! ~ sqrt(2 pi n) (n/e)^n`, is a useful imported
lemma for a local expansion, and it is exactly that: an imported lemma whose
error must be bounded if it is used in a proof. Expanding an identity that
already assumes a local Gaussian form does not establish that form. If a
derivation supplies a lemma without proof, say so and state precisely what
follows from it, rather than presenting the conclusion as established.

## 11. Scope limits and common mistakes

- Writing `P(Y <= k) = Phi((k + 0.5 - mu)/sigma)` as an identity. The left
  side is a lattice probability and the right side is a continuous
  approximation; the relation is approximate unless exact Normal assumptions
  hold.
- Forgetting that `sigma` in `h = d/sigma` is the standard deviation of the
  original variable, not the standard error of a mean, when the lattice is a
  count.
- Treating the continuity correction as fixing the shape approximation. It
  addresses boundary alignment; it does not prove the Gaussian shape.
- Reusing `n` for both observations per sample and repeated samples, and
  reusing `sigma` for both a parent SD and a standard error.
- Claiming monotone improvement in `n` or across all events when only a
  distributional limit has been shown.
- Quoting a formula as proof. A correct formula can rely on an unstated
  lemma; the obligation is to identify what is exact, what is a definition,
  what is an approximation, and what is imported.

## 12. Exercises

1. For `Y ~ Bin(20, 0.4)`, compute `P(Y <= 8)` exactly and compare with
   `Phi((8.5 - 8)/sigma)` and `Phi((8 - 8)/sigma)`. Which comparison shows
   the boundary alignment effect?
2. State which step in standardizing `Y` is exact and which step is an
   approximation.
3. For spacing `d = 1/5`, write the original half-cell, the standardized
   grid spacing `h`, and the standardized half-cell. Keep the support of the
   discrete variable in the answer.
4. On a symmetric cell, explain why a linear density term cancels in the
   midpoint comparison and why a quadratic term does not.
5. Give one reason a pointwise cell bound does not bound the CDF error over
   all `j <= k`.
