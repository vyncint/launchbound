//! Measurement: bench plans (`plan.v1`), the PTX runner (CUDA driver API
//! via dlopen — builds anywhere, times only where a driver exists), and
//! interval statistics. Every timing carries warmup, repeats, median, a
//! 95% CI, and the outlier rule; overlapping intervals are reported as
//! indistinguishable, never ranked (docs/BENCHMARKING.md).

pub mod cuda;
pub mod plan;
pub mod run;
pub mod stats;

pub use plan::{ArgSpec, BenchPlan, BenchSpec, Candidate, PlanError};
pub use run::{CandidateResult, Results, run_plan};
pub use stats::{Summary, indistinguishable, summarize};
