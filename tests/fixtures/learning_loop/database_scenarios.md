# Synthetic database application scenario

This original fictional scenario exists only for offline transfer-case
contracts. It is not quoted from DDIA or another publication.

A client writes a new value to a primary. The primary acknowledges the write.
The client then reads through a lagging replica and receives the older value.
The learner must identify the violated read guarantee and propose one justified
system change while keeping added read latency at or below 20 ms. The case does
not prescribe a single implementation: a proposal must name its guarantee and
explain its latency consequence.
