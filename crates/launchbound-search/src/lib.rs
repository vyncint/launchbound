//! Search strategies over a bench plan's candidates (S5).
//!
//! A strategy is an *ordering*: deterministic given (plan, seed),
//! independent of measurement outcomes, and therefore trivially resumable —
//! the runner walks the order, skips candidates already in results.json,
//! and stops when the wall budget is spent. The box will be killed sooner
//! or later (docs/ARCHITECTURE.md); an order that depended on volatile state
//! would not survive that.
//!
//! Model-guided ordering lands with S6: it is still an a-priori order, just
//! sorted by the analytical model's estimate.

use launchbound_bench::BenchPlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Plan order (the space's canonical enumeration order).
    Exhaustive,
    /// Seeded uniform shuffle, without replacement: the budget-limited
    /// baseline every cleverer strategy must beat.
    Random { seed: u64 },
}

impl Strategy {
    pub fn parse(name: &str, seed: u64) -> Option<Strategy> {
        match name {
            "exhaustive" => Some(Strategy::Exhaustive),
            "random" => Some(Strategy::Random { seed }),
            _ => None,
        }
    }

    /// The visiting order over candidate indices. Pure function of
    /// (plan, self): same plan and seed, same order, on any host.
    pub fn order(&self, plan: &BenchPlan) -> Vec<usize> {
        let n = plan.candidates.len();
        let mut indices: Vec<usize> = (0..n).collect();
        if let Strategy::Random { seed } = self {
            // Fisher-Yates with a xorshift64* stream; avoids platform RNGs
            // so the order reproduces anywhere.
            let mut state = seed.wrapping_mul(0x9e3779b97f4a7c15).max(1);
            let mut next = || {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state
            };
            for i in (1..n).rev() {
                let j = (next() % (i as u64 + 1)) as usize;
                indices.swap(i, j);
            }
        }
        indices
    }
}

/// Candidates measured per GPU-hour — the metric that matters under the
/// budget (docs/ARCHITECTURE.md).
pub fn candidates_per_gpu_hour(measured: usize, gpu_seconds: f64) -> f64 {
    if gpu_seconds <= 0.0 {
        return 0.0;
    }
    measured as f64 * 3600.0 / gpu_seconds
}

#[cfg(test)]
mod tests {
    use super::*;
    use launchbound_bench::{BenchPlan, Candidate};

    fn plan(n: usize) -> BenchPlan {
        BenchPlan {
            schema: "plan.v1".into(),
            kernel: "k".into(),
            entry: "k".into(),
            cc: "8.6".into(),
            allow_unsafe_reason: None,
            candidates: (0..n)
                .map(|i| Candidate {
                    id: format!("c1-{i:016x}"),
                    config: format!("i={i}"),
                    ptx: "x.ptx".into(),
                    unsafe_candidate: false,
                    grid: [1, 1, 1],
                    block: [32, 1, 1],
                    args: vec![],
                    warmup: 1,
                    repeats: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn exhaustive_is_plan_order() {
        let p = plan(5);
        assert_eq!(Strategy::Exhaustive.order(&p), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn random_is_deterministic_per_seed_and_complete() {
        let p = plan(20);
        let a = Strategy::Random { seed: 42 }.order(&p);
        let b = Strategy::Random { seed: 42 }.order(&p);
        assert_eq!(a, b, "same seed, same order");
        let c = Strategy::Random { seed: 43 }.order(&p);
        assert_ne!(a, c, "different seed, different order");
        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..20).collect::<Vec<_>>(), "a permutation");
    }

    #[test]
    fn throughput_metric() {
        assert_eq!(candidates_per_gpu_hour(60, 3600.0), 60.0);
        assert_eq!(candidates_per_gpu_hour(0, 0.0), 0.0);
    }
}
