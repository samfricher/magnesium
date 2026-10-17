# Synthetic course outline: recursion and application

This is a fictional, compact outline created for offline regression cases. It
does not reproduce a course, lecture, learner transcript, or private export.

## Source units

- **A — Function application:** a function consumes arguments; application
  substitutes supplied arguments for parameters before evaluating the body.
- **B — Patterns and recursion:** pattern matching chooses a branch. A
  recursive call occurs only when the selected expression calls the function
  again.
- **C — Recursive list sums:** `sumValues []` is the empty-list base case and
  returns zero; `sumValues (x : xs)` adds `x` to the recursive result for `xs`.

## Criteria used by cases

- Explain parameter, body, and application without assuming lambda notation.
- Distinguish selecting a pattern branch from recursively processing a tail.
- Trace the empty-list base case separately from the numeric result.
- Keep support for distinct criteria separate when only one criterion is
  demonstrated.

The identifiers and examples in this file are fixture-local. They must not be
compiled into runtime policy.
