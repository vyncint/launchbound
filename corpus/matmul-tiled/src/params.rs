//! Compile-time tuning parameters. Repo defaults; the tuner rewrites this
//! file per candidate in a scratch copy of the crate, never in the repo.

/// Output tile rows per block. Dimension `tm`.
pub const TM: usize = 16;
/// Output tile columns per block. Dimension `tn`.
pub const TN: usize = 16;
/// Staging depth along K. Dimension `tk`.
pub const TK: usize = 8;

/// `#[launch_bounds]` max threads; the block is TM*TN threads.
pub const LB_MAX: u32 = 1024;
