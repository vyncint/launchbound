//! Compile-time tuning parameters. These are the repo defaults; the tuner
//! rewrites this file per candidate in a scratch copy of the crate, never
//! in the repository.

/// Shared-memory tile length (elements). Dimension `tile` in kernel.toml.
pub const TILE: usize = 128;

/// `#[launch_bounds]` max threads (`.maxntid`). Must cover every `block_x`
/// value in kernel.toml; launching a bigger block fails at the driver.
pub const LB_MAX: u32 = 256;
