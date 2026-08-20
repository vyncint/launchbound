# matmul-tiled

A tiled matrix multiply with rectangular `TM x TN` output tiles and
`TK`-deep shared-memory staging — the corpus's genuinely large tile/block
space (corpus/README.md). Barriers bracket each staging round and are
unconditional; the interesting tuning dimensions are the tile shape and its
interaction with occupancy and shared-memory capacity.

The constraint set is part of the fixture: block shape equals tile shape,
thread count caps at 1024, and both staging tiles must fit the 48 KB
default shared-memory budget.

Assumes M, N, K are exact multiples of the tile sizes; the host runner
guarantees that. Fixture, not a library.
