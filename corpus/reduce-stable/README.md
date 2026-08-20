# reduce-stable

The control for `reduce-flip`, so the gate is tested in both directions
(corpus/README.md): the same shared-memory reduction, with every
`sync_threads()` hoisted to top level where all threads reach it. Divergent
guards contain only non-barrier work.

Expected gate behaviour (`known = "stable"`): the prune stage must
disqualify **no** configuration at any block size. A gate that rejects this
kernel is broken in the conservative direction, which is still broken.
