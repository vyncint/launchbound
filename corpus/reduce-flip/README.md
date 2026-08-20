# reduce-flip

The canonical case from the README, and the reason this tool exists: a shared-memory
reduction whose finalization runs under `if warp::warp_id() == 0`, with a
block-wide `sync_threads()` inside the guard.

At `block_x = 32` the block is one warp, the guard is uniform, nothing
diverges: **safe**. At `block_x = 64` and above, warps past the first skip a
block-wide barrier: **undefined** — on hardware, usually a permanent hang.
Same source, opposite truth, decided by the launch configuration.

Expected gate behaviour (`known = "flip"`): the prune stage must disqualify
every configuration with `block_x > 32` and admit `block_x = 32`.
`reconverge` reports this as RC001 (warning tier) with provenance naming
`warp_id()`; the block-size decision is launchbound's.
