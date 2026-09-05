//! The decision rule (docs/SAFETY.md). Changing it requires
//! explicit human sign-off — it is the product.
//!
//! The rule is a **pure function** of (analyzer outcome, candidate config):
//! reconverge's answer does not change with the launch shape (measured in
//! docs/research-baseline.md), so one analyzer run serves every launch-shape
//! candidate of the same specialization, and this function is evaluated per
//! candidate.

use crate::findings::Finding;
use launchbound_space::Config;
use serde::Serialize;

/// Warp width on every CUDA part this project targets.
pub const WARP_SIZE: u64 = 32;

/// What the analyzer run produced for one specialization source.
#[derive(Debug, Clone)]
pub enum AnalyzerOutcome {
    /// Exit 0 or 1 with a parsed findings document.
    Findings {
        exit_code: i32,
        findings: Vec<Finding>,
    },
    /// Exit 2, a crash, or unparseable output: a hard stop, never a pass.
    ToolError { detail: String },
}

/// The gate's verdict for one candidate configuration.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    /// No findings apply at this configuration.
    Clean,
    /// Launch-shape-independent findings exist; the candidate proceeds and
    /// the caveats appear in the report.
    AdmittedWithCaveats { caveats: Vec<CaveatRecord> },
    /// Refused. The record names the rule and points at the source.
    Disqualified { records: Vec<RejectionRecord> },
    /// The analyzer could not answer: hard stop for this candidate.
    ToolError { detail: String },
}

impl std::fmt::Display for Verdict {
    /// Prose, because these reach a user.
    ///
    /// `apply` printed `ToolError { detail: "…\n…" }` — a Rust struct
    /// literal with escaped newlines — in a message somebody is meant to act
    /// on. A `Debug` dump is a fine thing to log and a poor thing to read.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Verdict::Clean => write!(f, "clean"),
            Verdict::AdmittedWithCaveats { caveats } => {
                write!(f, "admitted with {} caveat", caveats.len())?;
                if caveats.len() != 1 {
                    write!(f, "s")?;
                }
                for caveat in caveats {
                    write!(f, "\n  {} {}", caveat.rule, caveat.message)?;
                }
                Ok(())
            }
            Verdict::Disqualified { records } => {
                write!(f, "refused by the gate")?;
                for record in records {
                    write!(f, "\n  {} ", record.rule)?;
                    if let Some(span) = &record.span {
                        write!(f, "at {span}: ")?;
                    }
                    write!(f, "{}", record.reason)?;
                }
                Ok(())
            }
            Verdict::ToolError { detail } => write!(f, "{detail}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RejectionRecord {
    pub rule: String,
    pub confidence: String,
    pub message: String,
    /// `file:line:col` of the offending site, if the analyzer gave one.
    pub span: Option<String>,
    /// Why this finding disqualifies at this configuration.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CaveatRecord {
    pub rule: String,
    pub confidence: String,
    pub message: String,
    pub span: Option<String>,
}

/// Rules that flag convergence hazards (barriers, masked collectives).
fn is_convergence_rule(code: &str) -> bool {
    matches!(code, "RC001" | "RC002")
}

/// Is this finding's divergence source launch-shape-dependent?
///
/// The measured flip family (simt-diff: 11 of 147 cases, every one
/// `warp_id()`-guarded) diverges at block level exactly when the block
/// holds more than one warp. reconverge's provenance names the source read;
/// we recognize the `warp_id()` family. Other sources (threadIdx, lane_id,
/// data-dependent) diverge at essentially every launch shape, so they are
/// launch-shape-independent for this rule's purposes.
fn launch_shape_dependent(finding: &Finding) -> bool {
    finding
        .provenance
        .iter()
        .any(|p| p.what.contains("warp_id()"))
}

/// The decision rule. Pure: no I/O, no clock, no environment.
pub fn decide(outcome: &AnalyzerOutcome, config: &Config) -> Verdict {
    let (exit_code, findings) = match outcome {
        AnalyzerOutcome::ToolError { detail } => {
            return Verdict::ToolError {
                detail: detail.clone(),
            };
        }
        AnalyzerOutcome::Findings {
            exit_code,
            findings,
        } => (*exit_code, findings),
    };

    let mut rejections = Vec::new();
    let mut caveats = Vec::new();

    for finding in findings {
        let span = finding.span.as_ref().map(ToString::to_string);
        let deny_tier = matches!(finding.confidence.as_str(), "deny" | "confirmed");

        if deny_tier {
            // The analyzer itself refuses this source at any launch shape.
            rejections.push(RejectionRecord {
                rule: finding.code.clone(),
                confidence: finding.confidence.clone(),
                message: finding.message.clone(),
                span,
                reason: format!(
                    "reconverge reports this at {} confidence; it holds at every launch shape",
                    finding.confidence
                ),
            });
        } else if is_convergence_rule(&finding.code) && launch_shape_dependent(finding) {
            let threads = config.block_threads();
            if threads > WARP_SIZE {
                rejections.push(RejectionRecord {
                    rule: finding.code.clone(),
                    confidence: finding.confidence.clone(),
                    message: finding.message.clone(),
                    span,
                    reason: format!(
                        "divergence source `warp_id()` splits a {threads}-thread block \
                         ({} warps) at a block-wide barrier; safe only at one warp \
                         (<= {WARP_SIZE} threads)",
                        threads.div_ceil(WARP_SIZE)
                    ),
                });
            }
            // At <= 32 threads the block is a single warp: the guard is
            // uniform, the finding does not apply, nothing to record.
        } else {
            caveats.push(CaveatRecord {
                rule: finding.code.clone(),
                confidence: finding.confidence.clone(),
                message: finding.message.clone(),
                span,
            });
        }
    }

    // Belt and braces: exit 1 with nothing parsed as deny-tier would mean
    // our parse missed something the analyzer refused. Never pass that.
    if exit_code == 1 && rejections.is_empty() {
        rejections.push(RejectionRecord {
            rule: "EXIT1".into(),
            confidence: "deny".into(),
            message: "reconverge exited 1 (deny/confirmed findings) but none were recognized"
                .into(),
            span: None,
            reason: "unrecognized deny-tier outcome is a refusal, not a pass".into(),
        });
    }

    if !rejections.is_empty() {
        Verdict::Disqualified {
            records: rejections,
        }
    } else if !caveats.is_empty() {
        Verdict::AdmittedWithCaveats { caveats }
    } else {
        Verdict::Clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::findings::{Finding, ProvenanceEntry};
    use launchbound_space::{KernelSpec, enumerate};

    fn configs(block_values: &str) -> Vec<Config> {
        let spec = KernelSpec::from_toml_str(
            "t",
            &format!(
                "[kernel]\nname = \"t\"\nentry = \"t\"\ndomain = 1\n\
                 [dims.block_x]\nvalues = [{block_values}]\n"
            ),
        )
        .unwrap();
        enumerate(&spec).unwrap()
    }

    fn warp_id_finding() -> Finding {
        Finding {
            code: "RC001".into(),
            confidence: "warning".into(),
            kernel: "reduce".into(),
            message: "may execute sync_threads() under thread-divergent control".into(),
            span: None,
            provenance: vec![ProvenanceEntry {
                what: "_25: per-lane environment read `warp_id()`".into(),
                span: None,
            }],
            notes: vec![],
            help: None,
        }
    }

    fn tid_finding() -> Finding {
        Finding {
            provenance: vec![ProvenanceEntry {
                what: "_7: per-lane environment read `threadIdx_x()`".into(),
                span: None,
            }],
            ..warp_id_finding()
        }
    }

    #[test]
    fn warp_id_barrier_disqualifies_above_one_warp_only() {
        let outcome = AnalyzerOutcome::Findings {
            exit_code: 0,
            findings: vec![warp_id_finding()],
        };
        for config in configs("32, 64, 128, 256") {
            let verdict = decide(&outcome, &config);
            match config.block_threads() {
                32 => assert_eq!(verdict, Verdict::Clean, "block=32 must be admitted"),
                n => assert!(
                    matches!(verdict, Verdict::Disqualified { .. }),
                    "block={n} must be disqualified"
                ),
            }
        }
    }

    #[test]
    fn launch_shape_independent_findings_become_caveats() {
        let outcome = AnalyzerOutcome::Findings {
            exit_code: 0,
            findings: vec![tid_finding()],
        };
        for config in configs("32, 256") {
            assert!(
                matches!(
                    decide(&outcome, &config),
                    Verdict::AdmittedWithCaveats { .. }
                ),
                "threadIdx-sourced warning is a caveat at any shape"
            );
        }
    }

    #[test]
    fn deny_tier_disqualifies_at_every_shape() {
        let mut finding = warp_id_finding();
        finding.confidence = "deny".into();
        let outcome = AnalyzerOutcome::Findings {
            exit_code: 1,
            findings: vec![finding],
        };
        for config in configs("32, 64") {
            assert!(matches!(
                decide(&outcome, &config),
                Verdict::Disqualified { .. }
            ));
        }
    }

    #[test]
    fn tool_error_is_never_a_pass() {
        let outcome = AnalyzerOutcome::ToolError {
            detail: "exit 2".into(),
        };
        for config in configs("32") {
            assert!(matches!(
                decide(&outcome, &config),
                Verdict::ToolError { .. }
            ));
        }
    }

    #[test]
    fn exit_1_without_recognized_deny_findings_still_refuses() {
        let outcome = AnalyzerOutcome::Findings {
            exit_code: 1,
            findings: vec![],
        };
        for config in configs("32") {
            assert!(matches!(
                decide(&outcome, &config),
                Verdict::Disqualified { .. }
            ));
        }
    }

    #[test]
    fn no_findings_is_clean() {
        let outcome = AnalyzerOutcome::Findings {
            exit_code: 0,
            findings: vec![],
        };
        for config in configs("32, 256") {
            assert_eq!(decide(&outcome, &config), Verdict::Clean);
        }
    }
}
