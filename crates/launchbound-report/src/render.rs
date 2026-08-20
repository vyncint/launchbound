//! Text rendering. Legible to someone who has never used reconverge: rule
//! IDs come with the source span and a plain-language reason.

use crate::Report;
use std::fmt::Write;

/// Kept in the report crate so rendering cannot compile without it.
pub fn launchbound_metal_notice() -> &'static str {
    "NO convergence gate exists on the Metal path: the same bug class is NOT checked"
}

pub fn render_text(report: &Report) -> String {
    let mut out = String::new();
    let device = report
        .device
        .as_ref()
        .map(|d| format!("{} (cc {}, driver {})", d.name, d.cc, d.driver_version))
        .unwrap_or_else(|| "no device — nothing measured yet".into());
    let _ = writeln!(
        out,
        "launchbound report — {} · gate cc {} · {} · {}",
        report.kernel, report.gate_cc, report.measurement_kind, device
    );
    // Unconditional whenever the gate did not run: the Metal asymmetry is
    // published, never buried (docs/LIMITATIONS.md). Do not add a way to skip this.
    if report.convergence_gate == "none" {
        let _ = writeln!(out, "\n*** {} ***", launchbound_metal_notice());
    }

    match &report.chosen {
        Some(chosen) => {
            let s = &chosen.summary;
            let _ = writeln!(
                out,
                "\nCHOSEN: {}  {}\n  {:.4} ms  [{:.4}, {:.4}]  (n={}, {} outliers rejected)",
                chosen.id,
                chosen.config,
                s.median_ms,
                s.ci95_lo_ms,
                s.ci95_hi_ms,
                s.n,
                s.outliers_rejected
            );
            if !report.indistinguishable_from_chosen.is_empty() {
                let _ = writeln!(
                    out,
                    "  statistically indistinguishable from: {}",
                    report.indistinguishable_from_chosen.join(", ")
                );
            }
        }
        None => {
            let _ = writeln!(
                out,
                "\nCHOSEN: none — no admitted candidate has a measurement"
            );
        }
    }

    if !report.rejected_faster.is_empty() {
        let _ = writeln!(
            out,
            "\nREFUSED BUT FASTER — an autotuner without a convergence gate would have\nhanded you one of these:"
        );
        for r in &report.rejected_faster {
            let s = &r.summary;
            let _ = writeln!(
                out,
                "  {}  {}\n    {:.4} ms  [{:.4}, {:.4}]  — {:.2}x faster than the chosen config",
                r.id, r.config, s.median_ms, s.ci95_lo_ms, s.ci95_hi_ms, r.speedup_vs_chosen
            );
            for rule in &r.rules {
                let _ = writeln!(
                    out,
                    "    REFUSED {} at {}: {}",
                    rule.rule,
                    rule.span.as_deref().unwrap_or("<no span>"),
                    rule.reason
                );
            }
        }
        if let Some(reason) = &report.allow_unsafe_reason {
            let _ = writeln!(
                out,
                "    (measured under --allow-unsafe; recorded reason: {reason:?})"
            );
        }
    }

    let _ = writeln!(out, "\nALL CANDIDATES:");
    for c in &report.candidates {
        let timing = match (&c.summary, c.measurement_status.as_str()) {
            (Some(s), "ok") => format!(
                "{:.4} ms [{:.4}, {:.4}]",
                s.median_ms, s.ci95_lo_ms, s.ci95_hi_ms
            ),
            (_, "timeout") => "TIMED OUT (presumed hung — the failure the gate predicts)".into(),
            (_, "error") => format!("error: {}", c.measurement_error.as_deref().unwrap_or("?")),
            _ => "unmeasured".into(),
        };
        let mark = match c.verdict.as_str() {
            "clean" => " ",
            "admitted_with_caveats" => "~",
            "disqualified" => "x",
            "ungated" => "u",
            _ => "!",
        };
        let _ = writeln!(out, "  {mark} {}  {}  {timing}", c.id, c.config);
        for rule in &c.rules {
            let _ = writeln!(
                out,
                "      {} at {}: {}",
                rule.rule,
                rule.span.as_deref().unwrap_or("<no span>"),
                rule.reason
            );
        }
    }

    let t = &report.totals;
    let _ = writeln!(
        out,
        "\n{} candidates: {} admitted, {} refused; {} measured ok; {:.1} GPU-seconds consumed",
        t.candidates, t.admitted, t.refused, t.measured_ok, t.gpu_seconds
    );
    out
}
