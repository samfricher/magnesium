# Synthetic teaching sources, version 1

## Transformations
Let X and Y be independent Uniform(0,1) variables. Their joint support is the unit square, with joint density 1. For U=X+Y, F_U(u)=P(X+Y<=u). For 0<u<1 the selected region is triangular; for 1<u<2 it is the unit square minus the upper-right triangle, a pentagon. A fixed u specifies a line of pairs, not one outcome. Density weights area; area alone gives probability only for unit density. For nonuniform density integrate the density over the event region.

A transformation of one random input, such as Y squared, still has a one-dimensional input space. Its graph is a different object. For Uniform(-1,1), the event Y squared <= u is -sqrt(u)<=Y<=sqrt(u), intersected with the support.

## Chapter 7: repeated samples
Each independent sample contains n observations. R is the number of simulated samples, hence the number of computed sample means. Increasing n changes the sampling distribution of each mean; increasing R improves the simulated description of that fixed distribution. For iid observations with finite variance sigma squared, the mean has variance sigma squared/n. Independence suffices for variance additivity; zero pairwise covariance also suffices. This source writes Normal(mu,sigma) with standard deviation as the second argument.

## Program trace
Python: a=[1]; b=a; b.append(2); b=[3]. Append mutates the shared list. The final assignment changes b's binding, leaving a bound to [1,2].

## Argument
A town reports that library visits rose after it extended opening hours. The reported sequence supports temporal association. Without accounting for other changes or a suitable comparison it does not establish that longer hours caused the increase. Whether longer hours are worth their cost additionally depends on values and evidence about costs and benefits.
