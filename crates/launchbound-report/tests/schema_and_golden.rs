//! S4 tests: a synthetic run dir with a refused-but-faster candidate must
//! produce a report that (a) validates against schemas/report.v1.json,
//! (b) matches the golden snapshot, and (c) puts the refused-faster
//! configuration in the headline section with rule and span.

use launchbound_report::{RunDir, build_report, render_text};
use std::path::PathBuf;

/// A synthetic run: two admitted configs measured (one slower, one chosen),
/// one refused config measured faster under --allow-unsafe, one refused
/// config that timed out (hung, as the gate predicted).
fn synthetic_run_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lb-report-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    std::fs::write(
        dir.join("verdicts.json"),
        serde_json::json!({
            "schema": "verdicts.v1",
            "kernel": "reduce-flip",
            "cc": "8.6",
            "candidates": [
                {"id": "c1-aaaa", "config": "block_x=32 tile=128", "verdict": "clean", "block_threads": 32},
                {"id": "c1-bbbb", "config": "block_x=32 tile=256", "verdict": "clean", "block_threads": 32},
                {"id": "c1-cccc", "config": "block_x=128 tile=128", "verdict": "disqualified", "block_threads": 128,
                 "records": [{"rule": "RC001", "confidence": "warning", "message": "barrier under divergence",
                              "span": "src/lib.rs:33:13",
                              "reason": "divergence source `warp_id()` splits a 128-thread block (4 warps) at a block-wide barrier; safe only at one warp (<= 32 threads)"}]},
                {"id": "c1-dddd", "config": "block_x=256 tile=128", "verdict": "disqualified", "block_threads": 256,
                 "records": [{"rule": "RC001", "confidence": "warning", "message": "barrier under divergence",
                              "span": "src/lib.rs:33:13",
                              "reason": "divergence source `warp_id()` splits a 256-thread block (8 warps) at a block-wide barrier; safe only at one warp (<= 32 threads)"}]}
            ]
        })
        .to_string(),
    )
    .unwrap();

    std::fs::write(
        dir.join("plan.json"),
        serde_json::json!({
            "schema": "plan.v1",
            "kernel": "reduce-flip",
            "entry": "reduce",
            "cc": "8.6",
            "allow_unsafe_reason": "S4 gate test: measuring refused configs to prove the rejection report",
            "candidates": []
        })
        .to_string(),
    )
    .unwrap();

    let times_around =
        |center: f64| -> Vec<f64> { (0..50).map(|i| center + (i % 5) as f64 * 0.0001).collect() };
    std::fs::write(
        dir.join("results.json"),
        serde_json::json!({
            "schema": "results.v1",
            "kernel": "reduce-flip",
            "entry": "reduce",
            "plan_cc": "8.6",
            "device_name": "NVIDIA A10G",
            "device_cc": "8.6",
            "driver_version": "595.71",
            "total_gpu_seconds": 12.5,
            "candidates": [
                {"id": "c1-aaaa", "config": "block_x=32 tile=128", "status": "ok",
                 "warmup": 20, "repeats": 50, "times_ms": times_around(0.050),
                 "summary": null, "gpu_seconds": 3.0},
                {"id": "c1-bbbb", "config": "block_x=32 tile=256", "status": "ok",
                 "warmup": 20, "repeats": 50, "times_ms": times_around(0.040),
                 "summary": null, "gpu_seconds": 3.0},
                {"id": "c1-cccc", "config": "block_x=128 tile=128", "status": "ok",
                 "warmup": 20, "repeats": 50, "times_ms": times_around(0.020),
                 "summary": null, "gpu_seconds": 3.0},
                {"id": "c1-dddd", "config": "block_x=256 tile=128", "status": "timeout",
                 "warmup": 20, "repeats": 50, "times_ms": [],
                 "error": "unsafe candidate did not complete within 10s; presumed hung",
                 "summary": null, "gpu_seconds": 10.0}
            ]
        })
        .to_string(),
    )
    .unwrap();

    // Summaries are computed by the runner normally; recompute here from
    // times_ms so the fixture stays honest.
    let mut results: launchbound_bench::Results =
        serde_json::from_str(&std::fs::read_to_string(dir.join("results.json")).unwrap()).unwrap();
    for c in &mut results.candidates {
        if !c.times_ms.is_empty() {
            c.summary = launchbound_bench::summarize(&c.times_ms);
        }
    }
    results.checkpoint(&dir.join("results.json")).unwrap();
    dir
}

#[test]
fn report_validates_against_schema_and_matches_golden() {
    let dir = synthetic_run_dir();
    let run = RunDir::load(&dir).unwrap();
    let report = build_report(&run).unwrap();
    let value = serde_json::to_value(&report).unwrap();

    // (a) schema validation
    let schema_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/report.v1.json");
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(schema_path).unwrap()).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|e| format!("{e}"))
        .collect();
    assert!(errors.is_empty(), "schema violations: {errors:#?}");

    // (c) the headline section
    assert_eq!(report.chosen.as_ref().unwrap().id, "c1-bbbb");
    assert_eq!(
        report.rejected_faster.len(),
        1,
        "exactly the measured-faster refused one"
    );
    let rf = &report.rejected_faster[0];
    assert_eq!(rf.id, "c1-cccc");
    assert!(rf.speedup_vs_chosen > 1.5);
    assert_eq!(rf.rules[0].rule, "RC001");
    assert_eq!(rf.rules[0].span.as_deref(), Some("src/lib.rs:33:13"));

    let text = render_text(&report);
    assert!(text.contains("REFUSED BUT FASTER"));
    assert!(text.contains("src/lib.rs:33:13"));
    assert!(text.contains("TIMED OUT"));

    // (b) golden snapshot of the whole document
    insta::assert_json_snapshot!("report_v1_synthetic", value);

    std::fs::remove_dir_all(dir).ok();
}

/// S7 gate: the Metal no-gate notice cannot be omitted from a rendered
/// report whose convergence_gate is `none`.
#[test]
fn metal_reports_cannot_omit_the_no_gate_notice() {
    let dir = synthetic_run_dir_metal();
    let run = RunDir::load(&dir).unwrap();
    let report = build_report(&run).unwrap();
    assert_eq!(report.convergence_gate, "none");
    // No results.json in this run dir: the report must say `unmeasured`,
    // never `measured` (docs/LIMITATIONS.md).
    assert_eq!(report.measurement_kind, "unmeasured");
    let text = render_text(&report);
    assert!(
        text.contains("NO convergence gate exists on the Metal path"),
        "the notice is mandatory on every Metal report:\n{text}"
    );
    std::fs::remove_dir_all(dir).ok();
}

fn synthetic_run_dir_metal() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lb-report-metal-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("verdicts.json"),
        serde_json::json!({
            "schema": "verdicts.v1", "kernel": "reduce-stable", "cc": "metal",
            "gate": "none",
            "candidates": [
                {"id": "c1-m1", "config": "block_x=64 tile=128", "verdict": "ungated", "block_threads": 64},
            ]
        })
        .to_string(),
    )
    .unwrap();
    dir
}
