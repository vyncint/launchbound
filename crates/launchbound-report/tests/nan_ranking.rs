//! A NaN must never take the report down.
//!
//! Today's pipeline cannot deliver one: `summarize` rejects NaN samples at the
//! Tukey fences, the model clamps its divisor, and `serde_json` will not carry
//! a NaN through `results.json` at all. That is exactly why the five
//! `partial_cmp(..).expect("no NaN")` sorts survived review for four releases
//! — every one of them was true *at the time*.
//!
//! `RunDir` has public fields, so this test builds the input in memory and
//! skips the JSON that cannot express the value. It stands in for the future
//! model term, the hand-edited results file, and the third-party producer of
//! `results.v1` — none of which owe us a finite float.

use launchbound_bench::{CandidateResult, Results, Summary};
use launchbound_report::{RunDir, build_report, render_text};

fn summary(median: f64, lo: f64, hi: f64) -> Summary {
    Summary {
        n: 50,
        outliers_rejected: 0,
        median_ms: median,
        ci95_lo_ms: lo,
        ci95_hi_ms: hi,
        min_ms: lo,
        max_ms: hi,
        mean_ms: median,
    }
}

fn candidate(id: &str, config: &str, s: Summary) -> CandidateResult {
    CandidateResult {
        id: id.to_string(),
        config: config.to_string(),
        status: "ok".to_string(),
        error: None,
        warmup: 20,
        repeats: 50,
        times_ms: vec![],
        summary: Some(s),
        gpu_seconds: 1.0,
    }
}

fn run_dir_with(summaries: Vec<CandidateResult>, verdicts: serde_json::Value) -> RunDir {
    RunDir {
        verdicts,
        plan: None,
        results: Some(Results {
            schema: "results.v1".to_string(),
            kernel: "nan-probe".to_string(),
            entry: "probe".to_string(),
            plan_cc: "8.6".to_string(),
            device_name: "synthetic".to_string(),
            device_cc: "8.6".to_string(),
            driver_version: "0".to_string(),
            candidates: summaries,
            total_gpu_seconds: 4.0,
            strategy: None,
            budget_exhausted: false,
        }),
    }
}

/// Every median is NaN, so *every* comparison in the chosen-selection sort is
/// the one that used to panic. With `total_cmp` the report is produced and
/// renders; the numbers in it are meaningless, which is the honest outcome for
/// meaningless input, and the caller is still alive to see them.
#[test]
fn a_nan_median_produces_a_report_not_a_panic() {
    let verdicts = serde_json::json!({
        "schema": "verdicts.v1",
        "kernel": "nan-probe",
        "cc": "8.6",
        "candidates": [
            {"id": "c1-aaaa", "config": "block_x=32", "verdict": "clean", "block_threads": 32},
            {"id": "c1-bbbb", "config": "block_x=64", "verdict": "clean", "block_threads": 64},
            {"id": "c1-cccc", "config": "block_x=128", "verdict": "clean", "block_threads": 128}
        ]
    });
    let run = run_dir_with(
        vec![
            candidate(
                "c1-aaaa",
                "block_x=32",
                summary(f64::NAN, f64::NAN, f64::NAN),
            ),
            candidate(
                "c1-bbbb",
                "block_x=64",
                summary(f64::NAN, f64::NAN, f64::NAN),
            ),
            candidate(
                "c1-cccc",
                "block_x=128",
                summary(f64::NAN, f64::NAN, f64::NAN),
            ),
        ],
        verdicts,
    );

    let report = build_report(&run).expect("a report, not a panic");
    assert_eq!(report.candidates.len(), 3);
    // It renders too: the text path formats every one of these floats.
    let text = render_text(&report);
    assert!(
        text.contains("nan-probe"),
        "rendered report names the kernel"
    );
}

/// A NaN mixed with real measurements: the sort has to order NaN against
/// finite values, which is the comparison `partial_cmp` returned `None` for.
#[test]
fn one_nan_among_finite_medians_still_ranks() {
    let verdicts = serde_json::json!({
        "schema": "verdicts.v1",
        "kernel": "nan-probe",
        "cc": "8.6",
        "candidates": [
            {"id": "c1-aaaa", "config": "block_x=32", "verdict": "clean", "block_threads": 32},
            {"id": "c1-bbbb", "config": "block_x=64", "verdict": "clean", "block_threads": 64},
            {"id": "c1-cccc", "config": "block_x=128", "verdict": "clean", "block_threads": 128}
        ]
    });
    let run = run_dir_with(
        vec![
            candidate("c1-aaaa", "block_x=32", summary(0.050, 0.049, 0.051)),
            candidate(
                "c1-bbbb",
                "block_x=64",
                summary(f64::NAN, f64::NAN, f64::NAN),
            ),
            candidate("c1-cccc", "block_x=128", summary(0.030, 0.029, 0.031)),
        ],
        verdicts,
    );

    let report = build_report(&run).expect("a report, not a panic");
    // `total_cmp` puts +NaN above +inf, so the finite fastest still wins and
    // the NaN cannot masquerade as the best configuration.
    let chosen = report.chosen.clone().expect("a chosen configuration");
    assert_eq!(chosen.id, "c1-cccc", "the finite fastest must be chosen");
    assert!(chosen.summary.median_ms.is_finite());
    let _ = render_text(&report);
}

/// Infinities share the code path and were always orderable; they stay so.
#[test]
fn infinite_medians_are_ordered_not_rejected() {
    let verdicts = serde_json::json!({
        "schema": "verdicts.v1",
        "kernel": "nan-probe",
        "cc": "8.6",
        "candidates": [
            {"id": "c1-aaaa", "config": "block_x=32", "verdict": "clean", "block_threads": 32},
            {"id": "c1-bbbb", "config": "block_x=64", "verdict": "clean", "block_threads": 64}
        ]
    });
    let run = run_dir_with(
        vec![
            candidate(
                "c1-aaaa",
                "block_x=32",
                summary(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            ),
            candidate("c1-bbbb", "block_x=64", summary(0.030, 0.029, 0.031)),
        ],
        verdicts,
    );
    let report = build_report(&run).expect("a report, not a panic");
    assert_eq!(report.chosen.expect("chosen").id, "c1-bbbb");
}
