# stencil-1d

A 1D box stencil (sum of `2*RADIUS+1` neighbours), included for **register
pressure and unroll trade-offs** (corpus/README.md). There are no barriers and
no shared memory: the gate should have nothing to say at any configuration,
and the tuning story is entirely `unroll` x `radius` x `lb_max` (the
`#[launch_bounds]` cap, which bounds register allocation) against occupancy.

Also the corpus's example of a **spec-role launch-bounds dimension**
interacting with a launch dimension through a constraint
(`block_x <= lb_max`): declaring `.maxntid` below the launch width is a
driver-level launch failure, so the space model must forbid it.
