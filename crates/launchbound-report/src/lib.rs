//! The run report (`report.v1`). The section that prints every
//! configuration that was **faster and refused** is the product: never
//! soften, hide, or downrank it.

#![warn(missing_docs)]

mod build;
mod render;

pub use build::{RunDir, build_report};
pub use render::render_text;

// Re-exported, not merely used: `Summary` appears in the public fields of
// `CandidateReport`, `ChosenInfo` and `RejectedFaster`, so a caller that reads
// a report has to be able to name it. Without this the type was reachable and
// unnameable unless you also depended on `launchbound-bench` directly.
pub use launchbound_bench::Summary;

use serde::{Deserialize, Serialize};

/// What can go wrong assembling a report from a run directory.
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    /// The run directory is missing a document, or one of them does not
    /// declare the schema this build reads.
    #[error("run dir: {0}")]
    RunDir(String),
    /// The report could not be written.
    #[error("report io: {0}")]
    Io(String),
}

/// A `report.v1` document: the whole run, as a reader should see it.
///
/// This is the surface every other consumer reads — the TUI, the text
/// renderer, and anyone parsing the JSON. Its schema is validated against
/// `schemas/report.v1.json` in CI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    /// Schema tag; always `report.v1`.
    pub schema: String,
    /// The kernel this run tuned.
    pub kernel: String,
    /// The compute capability the gate ran at. A verdict does not transfer
    /// to another one (`docs/LIMITATIONS.md`).
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
    /// The part measured on. Absent when nothing was measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceInfo>,
    /// The operator's written reason for measuring refused candidates.
    /// Present exactly when `--allow-unsafe` was used, so a report
    /// containing refused timings always says why they were taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_unsafe_reason: Option<String>,
    /// The recommended configuration. Absent when nothing was both
    /// admitted and measured — a gate-only run, or one whose whole
    /// admitted set failed to measure.
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
    /// Every candidate, admitted or not, measured or not.
    pub candidates: Vec<CandidateReport>,
    /// Run-level tallies.
    pub totals: Totals,
}

/// The part the measurements were taken on. Results are valid only for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Product name as the driver reports it, e.g. `NVIDIA A10G`.
    pub name: String,
    /// The device's actual compute capability, which need not equal
    /// [`Report::gate_cc`] — the gate can be run for a different target
    /// than the one measured on.
    pub cc: String,
    /// Driver version, part of what makes a timing reproducible.
    pub driver_version: String,
}

/// The configuration this run recommends: fastest median among the
/// candidates that were admitted by the gate *and* measured successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChosenInfo {
    /// Its canonical `config.v1` ID.
    pub id: String,
    /// Its dimension assignments, e.g. `block_x=128 tile=256`.
    pub config: String,
    /// Its timing summary. Present by construction — being measured is
    /// what made it eligible.
    pub summary: Summary,
}

/// A refused configuration that measurably beat the chosen one.
///
/// This is the section the whole tool exists to produce: it is the cost of
/// the safety decision, stated in numbers rather than asserted. Populated
/// only when refused candidates were measured under `--allow-unsafe`, and
/// only when the candidate's whole confidence interval sits below the
/// chosen one's — "faster" here means measurably, not on a point estimate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedFaster {
    /// Its canonical `config.v1` ID.
    pub id: String,
    /// Its dimension assignments.
    pub config: String,
    /// Its timing summary.
    pub summary: Summary,
    /// chosen_median / this_median: how much faster the refused one was.
    pub speedup_vs_chosen: f64,
    /// The rules that refused it — what the speed cost buys.
    pub rules: Vec<RuleRef>,
}

/// One analyzer rule as the report carries it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleRef {
    /// Rule ID, e.g. `RC001`.
    pub rule: String,
    /// `file:line:col` of the offending site, when the analyzer gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
    /// Why it applies at this configuration — the launch-shape reasoning,
    /// not just the analyzer's generic message.
    pub reason: String,
}

/// Every candidate the run considered, admitted or not, measured or not.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateReport {
    /// Its canonical `config.v1` ID.
    pub id: String,
    /// Its dimension assignments.
    pub config: String,
    /// clean | admitted_with_caveats | disqualified | tool_error
    pub verdict: String,
    /// Rules that fired: refusals when disqualified, caveats when
    /// admitted with them, empty when clean.
    #[serde(default)]
    pub rules: Vec<RuleRef>,
    /// ok | error | timeout | unmeasured
    pub measurement_status: String,
    /// Timing summary, present exactly when `measurement_status` is `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
    /// Why the measurement failed, when it did — including the timeout
    /// message for a candidate that hung, which is the gate being right.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measurement_error: Option<String>,
    /// Wall-clock GPU seconds this candidate consumed, spent or wasted.
    pub gpu_seconds: f64,
}

/// Run-level tallies, so a reader can check the parts add up.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Totals {
    /// Configurations enumerated, after constraints pruned the product.
    pub candidates: usize,
    /// Passed the gate — clean or admitted with caveats.
    pub admitted: usize,
    /// Refused by the gate.
    pub refused: usize,
    /// Measured successfully. At most `admitted`, and fewer when a budget
    /// ran out or a measurement failed.
    pub measured_ok: usize,
    /// Total GPU seconds the run consumed.
    pub gpu_seconds: f64,
}
