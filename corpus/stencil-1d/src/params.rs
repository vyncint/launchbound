//! Compile-time tuning parameters. Repo defaults; the tuner rewrites this
//! file per candidate in a scratch copy of the crate, never in the repo.

/// Stencil radius: the kernel sums 2*RADIUS+1 neighbours. Dimension `radius`.
pub const RADIUS: usize = 1;

/// Unroll factor for the neighbour loop (0 = full unroll; 1 is invalid to
/// cuda-oxide). Dimension `unroll`.
pub const UNROLL: u32 = 2;

/// `#[launch_bounds]` max threads (`.maxntid`) — itself a tuning dimension
/// here (`lb_max`): it bounds register allocation.
pub const LB_MAX: u32 = 256;
