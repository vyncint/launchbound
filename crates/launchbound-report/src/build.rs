//! Build a report.v1 from a run directory: verdicts.json (the gate record)
//! + plan.json + results.json (measurement, if the box has run).

use crate::{
    CandidateReport, ChosenInfo, DeviceInfo, RejectedFaster, Report, ReportError, RuleRef, Totals,
};
use launchbound_bench::{BenchPlan, Results, indistinguishable};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct RunDir {
    pub verdicts: Value,
    pub plan: Option<BenchPlan>,
    pub results: Option<Results>,
}

impl RunDir {
    pub fn load(dir: &Path) -> Result<Self, ReportError> {
        let verdicts_path = dir.join("verdicts.json");
        let text = std::fs::read_to_string(&verdicts_path).map_err(|e| {
            ReportError::RunDir(format!(
                "{}: {e} (stage writes it)",
                verdicts_path.display()
            ))
        })?;
        let verdicts: Value =
            serde_json::from_str(&text).map_err(|e| ReportError::RunDir(e.to_string()))?;
        if verdicts["schema"] != "verdicts.v1" {
            return Err(ReportError::RunDir(format!(
                "unsupported verdicts schema {}",
                verdicts["schema"]
            )));
        }
        let plan = BenchPlan::load(&dir.join("plan.json")).ok();
        // A results.json that exists but cannot be read is an error, not
        // "the box has not run yet". The two call for opposite actions —
        // wait, or go and look — and rendering both as `unmeasured` at
        // exit 0 told them apart for nobody. Same treatment `verdicts.v1`
        // gets fifteen lines above.
        let results = Results::load(&dir.join("results.json")).map_err(ReportError::RunDir)?;
        Ok(RunDir {
            verdicts,
            plan,
            results,
        })
    }

    pub fn path_of(dir: &str) -> PathBuf {
        PathBuf::from(dir)
    }
}

fn rules_of(candidate: &Value) -> Vec<RuleRef> {
    let mut rules = Vec::new();
    for key in ["records", "caveats"] {
        if let Some(list) = candidate.get(key).and_then(Value::as_array) {
            for r in list {
                rules.push(RuleRef {
                    rule: r["rule"].as_str().unwrap_or("?").to_string(),
                    span: r["span"].as_str().map(String::from),
                    reason: r
                        .get("reason")
                        .or_else(|| r.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                });
            }
        }
    }
    rules
}

pub fn build_report(run: &RunDir) -> Result<Report, ReportError> {
    let kernel = run.verdicts["kernel"].as_str().unwrap_or("?").to_string();
    let gate_cc = run.verdicts["cc"].as_str().unwrap_or("?").to_string();
    let convergence_gate = run.verdicts["gate"].as_str().unwrap_or("full").to_string();
    let empty = Vec::new();
    let verdict_candidates = run.verdicts["candidates"].as_array().unwrap_or(&empty);

    let mut candidates = Vec::new();
    let mut admitted = 0usize;
    let mut refused = 0usize;
    let mut measured_ok = 0usize;
    let mut gpu_seconds = 0.0f64;

    for vc in verdict_candidates {
        let id = vc["id"].as_str().unwrap_or("?").to_string();
        let verdict = vc["verdict"].as_str().unwrap_or("?").to_string();
        match verdict.as_str() {
            "clean" | "admitted_with_caveats" | "ungated" => admitted += 1,
            "disqualified" => refused += 1,
            _ => {}
        }
        let measurement = run
            .results
            .as_ref()
            .and_then(|r| r.candidates.iter().find(|c| c.id == id));
        let (status, summary, error, secs) = match measurement {
            Some(m) => {
                if m.status == "ok" {
                    measured_ok += 1;
                }
                gpu_seconds += m.gpu_seconds;
                (
                    m.status.clone(),
                    m.summary.clone(),
                    m.error.clone(),
                    m.gpu_seconds,
                )
            }
            None => ("unmeasured".to_string(), None, None, 0.0),
        };
        candidates.push(CandidateReport {
            id,
            config: vc["config"].as_str().unwrap_or("?").to_string(),
            verdict,
            rules: rules_of(vc),
            measurement_status: status,
            summary,
            measurement_error: error,
            gpu_seconds: secs,
        });
    }

    // The chosen configuration: best median among measured, admitted, ok.
    let chosen = candidates
        .iter()
        .filter(|c| {
            matches!(
                c.verdict.as_str(),
                "clean" | "admitted_with_caveats" | "ungated"
            ) && c.measurement_status == "ok"
        })
        .filter_map(|c| c.summary.as_ref().map(|s| (c, s.median_ms)))
        .min_by(|a, b| a.1.partial_cmp(&b.1).expect("no NaN medians"))
        .map(|(c, _)| ChosenInfo {
            id: c.id.clone(),
            config: c.config.clone(),
            summary: c.summary.clone().expect("chosen is measured"),
        });

    let mut indistinguishable_from_chosen = Vec::new();
    let mut rejected_faster = Vec::new();
    if let Some(chosen) = &chosen {
        for c in &candidates {
            let Some(summary) = &c.summary else { continue };
            if c.id == chosen.id || c.measurement_status != "ok" {
                continue;
            }
            let is_admitted = matches!(
                c.verdict.as_str(),
                "clean" | "admitted_with_caveats" | "ungated"
            );
            if is_admitted && indistinguishable(summary, &chosen.summary) {
                indistinguishable_from_chosen.push(c.id.clone());
            }
            // Measurably faster: the whole interval sits below the chosen's.
            if c.verdict == "disqualified" && summary.ci95_hi_ms < chosen.summary.ci95_lo_ms {
                rejected_faster.push(RejectedFaster {
                    id: c.id.clone(),
                    config: c.config.clone(),
                    summary: summary.clone(),
                    speedup_vs_chosen: chosen.summary.median_ms / summary.median_ms,
                    rules: c.rules.clone(),
                });
            }
        }
        rejected_faster.sort_by(|a, b| {
            b.speedup_vs_chosen
                .partial_cmp(&a.speedup_vs_chosen)
                .expect("no NaN speedups")
        });
    }

    let total = candidates.len();
    Ok(Report {
        schema: "report.v1".into(),
        kernel,
        gate_cc,
        // A report with no measurements must say so — labelling it
        // `measured` would be the exact dishonesty §1.9 forbids.
        measurement_kind: if run.results.is_some() {
            "measured".into()
        } else {
            "unmeasured".into()
        },
        convergence_gate,
        device: run.results.as_ref().map(|r| DeviceInfo {
            name: r.device_name.clone(),
            cc: r.device_cc.clone(),
            driver_version: r.driver_version.clone(),
        }),
        allow_unsafe_reason: run
            .plan
            .as_ref()
            .and_then(|p| p.allow_unsafe_reason.clone()),
        chosen,
        indistinguishable_from_chosen,
        rejected_faster,
        candidates,
        totals: Totals {
            candidates: total,
            admitted,
            refused,
            measured_ok,
            gpu_seconds,
        },
    })
}
