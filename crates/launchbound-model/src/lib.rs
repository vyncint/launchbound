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
    #[error("unknown compute capability {0:?} — the model has no device table for it")]
    UnknownCc(String),
    #[error("kernel.toml [model]: {0}")]
    Spec(String),
    #[error(transparent)]
    Space(#[from] launchbound_space::SpaceError),
}

/// Per-SM limits by compute capability. Only the parts this project has
/// measured on are listed; an unknown cc is an error, never a guess.
#[derive(Debug, Clone, Copy)]
pub struct DeviceParams {
    pub cc: &'static str,
    pub sm_count: u32,
    pub max_threads_per_sm: u32,
    pub max_warps_per_sm: u32,
    pub max_blocks_per_sm: u32,
    pub smem_per_block_default: u64,
    pub smem_per_sm: u64,
}

pub const DEVICES: &[DeviceParams] = &[
    // NVIDIA A10G (GA102, cc 8.6)
    DeviceParams {
        cc: "8.6",
        sm_count: 80,
        max_threads_per_sm: 1536,
        max_warps_per_sm: 48,
        max_blocks_per_sm: 16,
        smem_per_block_default: 49_152,
        smem_per_sm: 102_400,
    },
    // NVIDIA T4 (TU104, cc 7.5)
    DeviceParams {
        cc: "7.5",
        sm_count: 40,
        max_threads_per_sm: 1024,
        max_warps_per_sm: 32,
        max_blocks_per_sm: 16,
        smem_per_block_default: 49_152,
        smem_per_sm: 65_536,
    },
];

pub fn device(cc: &str) -> Result<DeviceParams, ModelError> {
    DEVICES
        .iter()
        .find(|d| d.cc == cc)
        .copied()
        .ok_or_else(|| ModelError::UnknownCc(cc.to_string()))
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
    order.sort_by(|&a, &b| values[a].partial_cmp(&values[b]).expect("no NaN"));
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

    #[test]
    fn device_table_is_closed() {
        assert!(device("8.6").is_ok());
        assert!(device("7.5").is_ok());
        assert!(
            device("9.0").is_err(),
            "an unknown cc is an error, never a guess"
        );
    }
}
