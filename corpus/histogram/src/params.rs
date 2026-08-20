//! Compile-time tuning parameters. Repo defaults; the tuner rewrites this
//! file per candidate in a scratch copy of the crate, never in the repo.

/// Number of histogram bins (shared u32s). Dimension `bins`.
pub const BINS: usize = 256;

/// `#[launch_bounds]` max threads.
pub const LB_MAX: u32 = 256;
