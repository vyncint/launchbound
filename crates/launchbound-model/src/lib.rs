//! The analytical model behind `--backend model` (S6).
//!
//! It estimates *relative* cost within one kernel's space from occupancy
//! and wave count — nothing else. Its output is labelled `estimated` on
//! every surface, and it ships only with its measured Spearman rank
//! correlation against real hardware attached (docs/LIMITATIONS.md): the model
//! is gated on measured quality, not on plausibility.

use launchbound_space::{Config, KernelSpec, eval_arith_expr};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    // The known list is computed in the message, not carried in a second
    // field: adding a field to a public enum variant is a breaking change,
    // and 2.2.0 is a minor bump.
    #[error("unknown compute capability {cc:?} — the model has no device table for it; known: {known}", cc = .0, known = known_capabilities())]
    UnknownCc(String),
    #[error("kernel.toml [model]: {0}")]
    Spec(String),
    #[error(transparent)]
    Space(#[from] launchbound_space::SpaceError),
}

/// Per-SM limits by compute capability.
///
/// Every field but `sm_count` is a **compute-capability fact**, taken from
/// the CUDA C++ Programming Guide's "Technical Specifications per Compute
/// Capability" table. `sm_count` is a **product fact** — two parts at the
/// same capability differ — so each entry names the part its count came from.
///
/// An unknown cc is an error, never a guess: a fabricated capacity would
/// produce an occupancy number, and an occupancy number is exactly the sort
/// of thing a reader believes.
#[derive(Debug, Clone, Copy)]
pub struct DeviceParams {
    pub cc: &'static str,
    /// Streaming multiprocessors on the named part. Not a capability fact.
    ///
    /// Ranking within one kernel's space is barely sensitive to it: it enters
    /// only through `waves = grid / (blocks_per_sm * sm_count)`, a constant
    /// divisor that scales every candidate's cost alike, and it changes an
    /// ordering only where the `.max(1.0)` clamp on waves bites. It matters
    /// for reading `waves` as a number, not for choosing between candidates.
    pub sm_count: u32,
    pub max_threads_per_sm: u32,
    pub max_warps_per_sm: u32,
    pub max_blocks_per_sm: u32,
    /// Statically allocatable shared memory per block, without the dynamic
    /// opt-in. 48 KiB on every architecture here — deliberately flat.
    pub smem_per_block_default: u64,
    /// Shared memory per SM. Note this is the *per-SM* capacity, one KiB
    /// above the per-block opt-in maximum on Ampere and later, where the
    /// driver reserves 1 KiB.
    pub smem_per_sm: u64,
}

/// Ascending by compute capability. A test enforces both the order and the
/// internal consistency of every row.
///
/// # Why this is not shared with reconverge
///
/// reconverge's `cc.rs` carries a capability table too, and #52 asked whether
/// a shared `simt-device-table` crate should own both. The answer for 2.2.0
/// is no, for three reasons:
///
/// 1. **They answer different questions.** reconverge needs
///    `max_per_block` — the dynamic opt-in ceiling — because RC004 asks
///    "could this allocation ever load". This needs per-SM occupancy
///    capacity: threads, warps, blocks and shared memory *per SM*. Only
///    shared memory overlaps at all, and even there the numbers differ by
///    the 1 KiB the driver reserves on Ampere and later.
/// 2. **It would be a third pin.** A shared crate joins the lockstep set,
///    in a project whose headline 2.2.0 issue was that the pin set went 133
///    commits stale. Adding a pin to reduce duplication of eleven numbers is
///    a poor trade.
/// 3. **The drift is checkable without it.** The overlapping figures were
///    cross-checked by hand against reconverge 0.6.0's table when these rows
///    were written, and they agree exactly, per-SM minus the reserved KiB:
///
///    | cc | reconverge `max_per_block` | here `smem_per_sm` |
///    |---|---|---|
///    | 7.5 | 64 KiB | 64 KiB (no reservation pre-Ampere) |
///    | 8.0 | 163 KiB | 164 KiB |
///    | 8.6 | 99 KiB | 100 KiB |
///    | 8.9 | 99 KiB | 100 KiB |
///    | 9.0 | 227 KiB | 228 KiB |
///    | 10.0 | 227 KiB | 228 KiB |
///
/// Revisit if a third consumer appears, or if the two tables are ever found
/// to disagree — that would be the evidence this reasoning is wrong.
pub const DEVICES: &[DeviceParams] = &[
    // NVIDIA T4 (TU104, cc 7.5) — 40 SMs.
    DeviceParams {
        cc: "7.5",
        sm_count: 40,
        max_threads_per_sm: 1024,
        max_warps_per_sm: 32,
        max_blocks_per_sm: 16,
        smem_per_block_default: 49_152,
        smem_per_sm: 65_536, // 64 KiB
    },
    // NVIDIA A100 (GA100, cc 8.0) — 108 SMs on the 40 GB and 80 GB parts.
    DeviceParams {
        cc: "8.0",
        sm_count: 108,
        max_threads_per_sm: 2048,
        max_warps_per_sm: 64,
        max_blocks_per_sm: 32,
        smem_per_block_default: 49_152,
        smem_per_sm: 167_936, // 164 KiB
    },
    // NVIDIA A10G (GA102, cc 8.6) — 80 SMs.
    DeviceParams {
        cc: "8.6",
        sm_count: 80,
        max_threads_per_sm: 1536,
        max_warps_per_sm: 48,
        max_blocks_per_sm: 16,
        smem_per_block_default: 49_152,
        smem_per_sm: 102_400, // 100 KiB
    },
    // NVIDIA L4 (AD104, cc 8.9) — 58 SMs. The L40 is the same capability
    // with 142; pass the one you are running on.
    DeviceParams {
        cc: "8.9",
        sm_count: 58,
        max_threads_per_sm: 1536,
        max_warps_per_sm: 48,
        max_blocks_per_sm: 24,
        smem_per_block_default: 49_152,
        smem_per_sm: 102_400, // 100 KiB
    },
    // NVIDIA H100 SXM5 (GH100, cc 9.0) — 132 SMs. The PCIe part has 114.
    DeviceParams {
        cc: "9.0",
        sm_count: 132,
        max_threads_per_sm: 2048,
        max_warps_per_sm: 64,
        max_blocks_per_sm: 32,
        smem_per_block_default: 49_152,
        smem_per_sm: 233_472, // 228 KiB
    },
    // NVIDIA B200 (GB100, cc 10.0) — 148 SMs.
    DeviceParams {
        cc: "10.0",
        sm_count: 148,
        max_threads_per_sm: 2048,
        max_warps_per_sm: 64,
        max_blocks_per_sm: 32,
        smem_per_block_default: 49_152,
        smem_per_sm: 233_472, // 228 KiB
    },
];

/// `("8.6")` -> `(8, 6)`, for ordering and range checks. Returns `None` for
/// anything that is not `<int>.<int>`.
fn cc_parts(cc: &str) -> Option<(u32, u32)> {
    let (major, minor) = cc.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

pub fn device(cc: &str) -> Result<DeviceParams, ModelError> {
    DEVICES
        .iter()
        .find(|d| d.cc == cc)
        .copied()
        .ok_or_else(|| ModelError::UnknownCc(cc.to_string()))
}

/// The capabilities `device` will accept, ascending, for error messages.
///
/// Worth saying out loud rather than leaving the reader to guess: the gate
/// (reconverge) knows more capabilities than the model does, so `prune --cc`
/// can succeed at a value `tune --backend model --cc` refuses. That gap is
/// real and narrower than it was, and the message is where a reader meets it.
#[must_use]
pub fn known_capabilities() -> String {
    let mut ccs: Vec<&str> = DEVICES.iter().map(|d| d.cc).collect();
    ccs.sort_by_key(|cc| cc_parts(cc));
    ccs.join(", ")
}

/// One candidate's estimate. `cost` is a unitless relative score within a
/// kernel's space — smaller is predicted faster. It is NOT a time.
#[derive(Debug, Clone, Serialize)]
pub struct Estimate {
    pub id: String,
    pub config: String,
    pub cost: f64,
    pub occupancy: f64,
    pub waves: f64,
    pub smem_bytes: u64,
    /// Always "estimated" (docs/LIMITATIONS.md); serialized so every surface carries it.
    pub kind: &'static str,
}

/// Shared-memory bytes per block for a candidate: the `[model]`
/// `smem_bytes` expression in kernel.toml, over the kernel's dimensions.
pub fn smem_bytes(spec: &KernelSpec, config: &Config) -> Result<u64, ModelError> {
    let path = spec.dir.join("kernel.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| ModelError::Spec(format!("{}: {e}", path.display())))?;
    let table: toml::Value = toml::from_str(&text).map_err(|e| ModelError::Spec(e.to_string()))?;
    let Some(expr) = table
        .get("model")
        .and_then(|m| m.get("smem_bytes"))
        .and_then(|v| v.as_str())
    else {
        return Ok(0);
    };
    Ok(eval_arith_expr(expr, config, &BTreeMap::new())?)
}

/// Grid blocks for a candidate, from the [bench] grid expressions.
fn grid_blocks(spec: &KernelSpec, config: &Config) -> Result<u64, ModelError> {
    let path = spec.dir.join("kernel.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| ModelError::Spec(format!("{}: {e}", path.display())))?;
    let table: toml::Value = toml::from_str(&text).map_err(|e| ModelError::Spec(e.to_string()))?;
    let bench = table
        .get("bench")
        .ok_or_else(|| ModelError::Spec("no [bench] section".into()))?;
    let elements = bench
        .get("elements")
        .and_then(|v| v.as_integer())
        .unwrap_or(1) as u64;
    let mut extra = BTreeMap::new();
    extra.insert("elements".to_string(), elements);
    let mut blocks = 1u64;
    for axis in ["grid_x", "grid_y", "grid_z"] {
        let value = match bench.get(axis) {
            Some(toml::Value::Integer(n)) => *n as u64,
            Some(toml::Value::String(expr)) => eval_arith_expr(expr, config, &extra)?,
            None => 1,
            Some(other) => return Err(ModelError::Spec(format!("{axis}: bad value {other}"))),
        };
        blocks = blocks.saturating_mul(value.max(1));
    }
    Ok(blocks)
}

/// Estimate one candidate. Model: blocks-per-SM limited by threads, smem
/// and the block cap; cost = waves / occupancy — a candidate that needs
/// more waves of less-occupied SMs is predicted slower.
pub fn estimate(
    spec: &KernelSpec,
    config: &Config,
    dev: &DeviceParams,
) -> Result<Estimate, ModelError> {
    let threads = config.block_threads().max(1);
    let warps_per_block = threads.div_ceil(32);
    let smem = smem_bytes(spec, config)?;

    let by_threads = (dev.max_threads_per_sm as u64) / threads;
    let by_smem = dev.smem_per_sm.checked_div(smem).unwrap_or(u64::MAX);
    let blocks_per_sm = by_threads.min(by_smem).min(dev.max_blocks_per_sm as u64);

    if blocks_per_sm == 0 || smem > dev.smem_per_block_default {
        // Unlaunchable at this device's limits: infinite cost, not an error
        // — the ranking must place it last, the gate refuses it elsewhere.
        return Ok(Estimate {
            id: config.id().as_str().to_string(),
            config: config.to_string(),
            cost: f64::INFINITY,
            occupancy: 0.0,
            waves: f64::INFINITY,
            smem_bytes: smem,
            kind: "estimated",
        });
    }

    let occupancy = (blocks_per_sm * warps_per_block) as f64 / dev.max_warps_per_sm as f64;
    let occupancy = occupancy.min(1.0);
    let grid = grid_blocks(spec, config)? as f64;
    let waves = (grid / (blocks_per_sm * dev.sm_count as u64) as f64).max(1.0);
    // Work per block scales with the per-thread element count when a block
    // covers a fixed share of the workload; within one kernel's space that
    // is captured by waves already. Cost: waves penalized by low occupancy.
    let cost = waves / occupancy.max(1e-6);

    Ok(Estimate {
        id: config.id().as_str().to_string(),
        config: config.to_string(),
        cost,
        occupancy,
        waves,
        smem_bytes: smem,
        kind: "estimated",
    })
}

/// Spearman rank correlation between two paired samples (average ranks for
/// ties). Returns None below 3 pairs — a correlation of two points is
/// noise dressed up as a number.
pub fn spearman(xs: &[f64], ys: &[f64]) -> Option<f64> {
    if xs.len() != ys.len() || xs.len() < 3 {
        return None;
    }
    let rx = ranks(xs);
    let ry = ranks(ys);
    let n = rx.len() as f64;
    let mean = (n + 1.0) / 2.0;
    let (mut num, mut dx, mut dy) = (0.0, 0.0, 0.0);
    for (a, b) in rx.iter().zip(&ry) {
        num += (a - mean) * (b - mean);
        dx += (a - mean).powi(2);
        dy += (b - mean).powi(2);
    }
    if dx == 0.0 || dy == 0.0 {
        return None;
    }
    Some(num / (dx * dy).sqrt())
}

fn ranks(values: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let mut out = vec![0.0; values.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && values[order[j + 1]] == values[order[i]] {
            j += 1;
        }
        let avg_rank = (i + j) as f64 / 2.0 + 1.0;
        for &k in &order[i..=j] {
            out[k] = avg_rank;
        }
        i = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spearman_perfect_and_inverse_and_ties() {
        assert_eq!(
            spearman(&[1.0, 2.0, 3.0, 4.0], &[10.0, 20.0, 30.0, 40.0]),
            Some(1.0)
        );
        assert_eq!(
            spearman(&[1.0, 2.0, 3.0, 4.0], &[40.0, 30.0, 20.0, 10.0]),
            Some(-1.0)
        );
        assert!(spearman(&[1.0, 2.0], &[1.0, 2.0]).is_none());
        let r = spearman(&[1.0, 1.0, 2.0, 3.0], &[5.0, 5.0, 7.0, 9.0]).unwrap();
        assert!(r > 0.99);
    }

    // `spearman` is public and takes any `&[f64]` a caller has. Its `ranks`
    // helper sorted with `partial_cmp(..).expect("no NaN")`, so a NaN
    // argument -- a correlation against a column with one missing
    // measurement, say -- took the process down from safe code. `total_cmp`
    // orders it instead; the correlation that comes back is meaningless, but
    // it is a value, and the caller is still running to notice.
    #[test]
    fn a_nan_in_either_sample_does_not_panic() {
        let xs = [1.0, 2.0, f64::NAN, 4.0, 5.0];
        let ys = [10.0, 20.0, 30.0, 40.0, 50.0];
        let _ = spearman(&xs, &ys);
        let _ = spearman(&ys, &xs);
        let _ = spearman(&xs, &xs);
        let both_nan = [f64::NAN; 5];
        let _ = spearman(&both_nan, &ys);
        // Infinities were always orderable, but they share the code path.
        let inf = [1.0, f64::INFINITY, 3.0, f64::NEG_INFINITY, 5.0];
        let _ = spearman(&inf, &ys);
    }

    // Ranking is still correct for ordinary input -- `total_cmp` and
    // `partial_cmp` agree on every pair of non-NaN floats.
    #[test]
    fn total_cmp_did_not_change_the_ranking_of_ordinary_samples() {
        assert_eq!(
            spearman(&[1.0, 2.0, 3.0, 4.0], &[10.0, 20.0, 30.0, 40.0]),
            Some(1.0)
        );
        assert_eq!(
            spearman(&[3.0, 1.0, 4.0, 1.5], &[3.0, 1.0, 4.0, 1.5]),
            Some(1.0)
        );
    }

    /// The two capabilities the issue names, which used to be model errors.
    #[test]
    fn hopper_and_blackwell_are_in_the_table() {
        let h = device("9.0").expect("cc 9.0 (Hopper) must be known");
        assert_eq!(h.max_threads_per_sm, 2048);
        assert_eq!(h.max_warps_per_sm, 64);
        assert_eq!(h.max_blocks_per_sm, 32);
        assert_eq!(h.smem_per_sm, 228 * 1024);

        let b = device("10.0").expect("cc 10.0 (Blackwell) must be known");
        assert_eq!(b.max_threads_per_sm, 2048);
        assert_eq!(b.smem_per_sm, 228 * 1024);
    }

    /// Ordering and internal consistency of every row, so a future entry
    /// cannot be pasted in with a transposed digit and go unnoticed. These
    /// are the CUDA Programming Guide's documented ranges, not opinions.
    #[test]
    fn every_device_row_is_ordered_and_within_the_documented_ranges() {
        let mut previous: Option<(u32, u32)> = None;
        for d in DEVICES {
            let parts = cc_parts(d.cc).unwrap_or_else(|| panic!("cc {:?} does not parse", d.cc));

            // Ascending, and numerically: "10.0" sorts before "8.6" as a
            // string, which is exactly the trap a naive check falls into.
            if let Some(prev) = previous {
                assert!(
                    parts > prev,
                    "DEVICES must ascend by capability: {parts:?} follows {prev:?}"
                );
            }
            previous = Some(parts);

            // A warp is 32 threads on every NVIDIA part that has ever
            // shipped; the two limits are the same fact twice.
            assert_eq!(
                d.max_warps_per_sm * 32,
                d.max_threads_per_sm,
                "cc {}: {} warps x 32 != {} threads",
                d.cc,
                d.max_warps_per_sm,
                d.max_threads_per_sm
            );

            assert!(
                (1024..=2048).contains(&d.max_threads_per_sm),
                "cc {}: threads/SM {} outside the documented 1024..=2048",
                d.cc,
                d.max_threads_per_sm
            );
            assert!(
                (8..=32).contains(&d.max_blocks_per_sm),
                "cc {}: blocks/SM {} outside the documented 8..=32",
                d.cc,
                d.max_blocks_per_sm
            );

            // Static shared memory is capped at 48 KiB per block on every
            // architecture listed; anything above it needs the dynamic
            // opt-in, which is a launch-time decision this model does not
            // make. Flat, deliberately.
            assert_eq!(
                d.smem_per_block_default,
                48 * 1024,
                "cc {}: the static per-block cap is 48 KiB everywhere",
                d.cc
            );
            assert!(
                d.smem_per_sm >= d.smem_per_block_default,
                "cc {}: an SM cannot hold less than one block's worth",
                d.cc
            );
            assert!(
                d.smem_per_sm <= 228 * 1024,
                "cc {}: smem/SM {} above the largest documented capacity",
                d.cc,
                d.smem_per_sm
            );

            assert!(d.sm_count > 0, "cc {}: sm_count is a real part", d.cc);
        }
    }

    /// Every row is reachable by the name it carries, and no capability is
    /// listed twice — a duplicate would shadow silently, since `device`
    /// takes the first match.
    #[test]
    fn every_row_is_reachable_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for d in DEVICES {
            assert!(seen.insert(d.cc), "cc {} appears twice", d.cc);
            let found = device(d.cc).expect("a listed cc resolves");
            assert_eq!(found.cc, d.cc);
            assert_eq!(found.sm_count, d.sm_count);
        }
        assert_eq!(seen.len(), DEVICES.len());
    }

    /// The unknown-cc error names what would have worked. A reader who
    /// mistypes `8.60` should not have to read the source to find `8.6`.
    #[test]
    fn an_unknown_capability_lists_the_known_ones() {
        let err = device("11.5").expect_err("11.5 is not in the table");
        let msg = err.to_string();
        for cc in ["7.5", "8.0", "8.6", "8.9", "9.0", "10.0"] {
            assert!(msg.contains(cc), "message must name {cc}: {msg}");
        }
        // Ascending numerically, so 10.0 comes last rather than after 8.6.
        assert!(
            msg.find("9.0").unwrap() < msg.find("10.0").unwrap(),
            "known list must ascend numerically: {msg}"
        );
    }

    #[test]
    fn device_table_is_closed() {
        assert!(device("8.6").is_ok());
        assert!(device("7.5").is_ok());
        // This used to assert on 9.0, which 2.2.0 added — the example moved,
        // the rule did not. Pascal is deliberately out of scope (no corpus
        // kernel targets it and nothing here has run on one), and 99.9 is
        // not a capability at all.
        assert!(
            device("6.1").is_err(),
            "an untabulated cc is an error, never a guess"
        );
        assert!(device("99.9").is_err());
        // Nor is a well-formed prefix of a known one: "8" is not "8.0".
        assert!(device("8").is_err());
    }
}
