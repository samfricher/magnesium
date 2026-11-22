# Chapter 4: Logarithms

This chapter is invented for Magnesium's yardstick. It stands in for one chapter of a textbook read alone, with no course, no instructor and no test.

## 4.1 Exponents and their inverse

An exponential function multiplies by the same factor b for each step of x: f(x) = b^x with b > 0 and b ≠ 1. Because it is one to one, it has an inverse.

The logarithm base b is that inverse: log_b(y) = x means b^x = y. So log_2(8) = 3 because 2^3 = 8.

Example: log_10(1000) = 3, and log_5(1) = 0 because 5^0 = 1.

## 4.2 The three laws

For positive m and n:

- Product: log_b(mn) = log_b(m) + log_b(n).
- Quotient: log_b(m/n) = log_b(m) − log_b(n).
- Power: log_b(m^k) = k log_b(m).

Each law is an exponent law read backwards. The product law comes from b^p b^q = b^(p+q).

A common slip: log_b(m + n) has no law. It is not log_b(m) + log_b(n).

## 4.3 Change of base

A calculator has log base 10 and ln, base e. For any other base, log_b(y) = ln(y) / ln(b).

Example: log_3(20) = ln 20 / ln 3 ≈ 2.727.

## 4.4 Solving exponential equations

To solve b^x = c, take a logarithm of both sides and use the power law: x = ln(c) / ln(b).

Example: 3^x = 20 gives x = ln 20 / ln 3 ≈ 2.727.

When the unknown appears in two exponents, as in 2^(x+1) = 5^x, take logs, collect the x terms on one side and factor x out.

## 4.5 Growth and halving

A quantity that grows by a fixed percentage each period follows A(t) = A_0 (1 + r)^t. The time to double solves (1 + r)^t = 2, so t = ln 2 / ln(1 + r).

Halving works the same way with a factor below 1. A drug with a half-life of 6 hours leaves A_0 (1/2)^(t/6).

## Exercises

The exercises are numbered 4.21 to 4.32.

4.21 Evaluate log_2(32), log_10(0.01) and log_7(7).

4.22 Write log_3(81) = 4 as an exponential equation.

4.23 Expand log(x^3 y / z) with the three laws.

4.24 Combine 2 log(x) − log(y) into one logarithm.

4.25 Show that log_b(m + n) is not log_b(m) + log_b(n) with a numeric example.

4.26 Find log_6(50) to three decimal places.

4.27 Solve 5^x = 40.

4.28 Solve 2^(x+1) = 5^x.

4.29 Solve e^(2x) = 7.

4.30 An account grows by 4% a year. How long until it doubles?

4.31 A sample has a half-life of 6 hours. How long until 10% of it is left?

4.32 Prove the quotient law from the exponent law b^p / b^q = b^(p−q).
