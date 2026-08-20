//! Compile-time tuning parameters. Repo defaults; the tuner rewrites this
//! file per candidate in a scratch copy of the crate, never in the repo.

/// Scan width: shared buffer length and loop bound. Dimension `block_max`;
/// the launch `block_x` must equal it.
pub const BLOCK_MAX: usize = 128;

/// `#[launch_bounds]` max threads.
pub const LB_MAX: u32 = 512;
