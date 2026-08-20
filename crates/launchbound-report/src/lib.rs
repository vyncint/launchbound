//! The run report (`report.v1`). The section that prints every
//! configuration that was **faster and refused** is the product: never
//! soften, hide, or downrank it.

mod build;
mod render;

pub use build::{RunDir, build_report};
pub use render::render_text;

use launchbound_bench::Summary;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("run dir: {0}")]
    RunDir(String),
    #[error("report io: {0}")]
    Io(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub schema: String,
    pub kernel: String,
    pub gate_cc: String,
    /// `measured` or `estimated` — stamped on the report and every number
    /// in it. Reporting an estimate as a measurement is release-blocking
    /// (docs/LIMITATIONS.md).
    pub measurement_kind: String,
    /// `full` when every candidate passed through the reconverge gate;
    /// `none` on the Metal path (there is no MSL analyzer — §3.4). The
    /// renderer prints the no-gate notice unconditionally when this is
    /// `none`; a test asserts it cannot be omitted.
    pub convergence_gate: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_unsafe_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen: Option<ChosenInfo>,
    /// IDs whose intervals overlap the chosen one: reported as
    /// indistinguishable, never ranked (docs/BENCHMARKING.md).
    #[serde(default)]
    pub indistinguishable_from_chosen: Vec<String>,
    /// THE section: refused configurations that measurably beat the chosen
    /// one (their CI is entirely below the chosen CI).
    #[serde(default)]
    pub rejected_faster: Vec<RejectedFaster>,
    pub candidates: Vec<CandidateReport>,
    pub totals: Totals,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub cc: String,
    pub driver_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChosenInfo {
    pub id: String,
    pub config: String,
    pub summary: Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedFaster {
    pub id: String,
    pub config: String,
    pub summary: Summary,
    /// chosen_median / this_median: how much faster the refused one was.
    pub speedup_vs_chosen: f64,
    pub rules: Vec<RuleRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleRef {
    pub rule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateReport {
    pub id: String,
    pub config: String,
    /// clean | admitted_with_caveats | disqualified | tool_error
    pub verdict: String,
    #[serde(default)]
    pub rules: Vec<RuleRef>,
    /// ok | error | timeout | unmeasured
    pub measurement_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measurement_error: Option<String>,
    pub gpu_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Totals {
    pub candidates: usize,
    pub admitted: usize,
    pub refused: usize,
    pub measured_ok: usize,
    pub gpu_seconds: f64,
}
