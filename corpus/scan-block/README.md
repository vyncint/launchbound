# scan-block

A block-level inclusive prefix scan (Hillis-Steele), included because it is
**barrier-dense** (corpus/README.md): two `sync_threads()` per doubling round,
log2(block) rounds, all at top level inside a loop whose bound is the
compile-time block size. Divergent guards (`tid >= stride`) contain only
reads, never barriers.

Exercises: many barriers in a uniform loop must produce zero findings, at
every block size — a false positive here would poison every scan-shaped
kernel. The `block_max` spec dimension pins the shared buffer and loop
bound to the launch width via a constraint.
