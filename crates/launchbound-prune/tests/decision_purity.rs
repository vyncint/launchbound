//! Property test (CONTRIBUTING.md): the safety decision is a pure function of
//! (kernel, config, findings) — same inputs, same verdict, no environment.

use launchbound_prune::{AnalyzerOutcome, Finding, ProvenanceEntry, decide};
use launchbound_space::{KernelSpec, enumerate};
use proptest::prelude::*;

fn arb_finding() -> impl Strategy<Value = Finding> {
    (
        proptest::sample::select(vec!["RC001", "RC002", "RC003", "RC004", "RC005"]),
        proptest::sample::select(vec!["warning", "deny", "confirmed"]),
        proptest::sample::select(vec![
            "per-lane environment read `warp_id()`",
            "per-lane environment read `threadIdx_x()`",
            "per-lane environment read `lane_id()`",
            "data-dependent branch",
        ]),
    )
        .prop_map(|(code, confidence, what)| Finding {
            code: code.into(),
            confidence: confidence.into(),
            kernel: "k".into(),
            message: "m".into(),
            span: None,
            provenance: vec![ProvenanceEntry {
                what: what.into(),
                span: None,
            }],
            notes: vec![],
            help: None,
        })
}

fn arb_outcome() -> impl Strategy<Value = AnalyzerOutcome> {
    (
        proptest::collection::vec(arb_finding(), 0..4),
        proptest::sample::select(vec![0i32, 1]),
    )
        .prop_map(|(findings, exit_code)| AnalyzerOutcome::Findings {
            exit_code,
            findings,
        })
}

proptest! {
    #[test]
    fn decision_is_pure(outcome in arb_outcome(), block in proptest::sample::select(vec![32u64, 64, 128, 256, 512])) {
        let spec = KernelSpec::from_toml_str(
            "p",
            &format!("[kernel]\nname = \"p\"\nentry = \"p\"\ndomain = 1\n[dims.block_x]\nvalues = [{block}]\n"),
        ).unwrap();
        let config = enumerate(&spec).unwrap().into_iter().next().unwrap();
        let a = decide(&outcome, &config);
        let b = decide(&outcome, &config);
        prop_assert_eq!(a, b);
    }

    #[test]
    fn warp_id_convergence_findings_never_pass_above_one_warp(
        confidence in proptest::sample::select(vec!["warning", "deny", "confirmed"]),
        block in proptest::sample::select(vec![64u64, 128, 256, 512]),
    ) {
        let finding = Finding {
            code: "RC001".into(),
            confidence: confidence.into(),
            kernel: "k".into(),
            message: "m".into(),
            span: None,
            provenance: vec![ProvenanceEntry {
                what: "per-lane environment read `warp_id()`".into(),
                span: None,
            }],
            notes: vec![],
            help: None,
        };
        let exit_code = if confidence == "warning" { 0 } else { 1 };
        let outcome = AnalyzerOutcome::Findings { exit_code, findings: vec![finding] };
        let spec = KernelSpec::from_toml_str(
            "p",
            &format!("[kernel]\nname = \"p\"\nentry = \"p\"\ndomain = 1\n[dims.block_x]\nvalues = [{block}]\n"),
        ).unwrap();
        let config = enumerate(&spec).unwrap().into_iter().next().unwrap();
        let disqualified = matches!(
            decide(&outcome, &config),
            launchbound_prune::Verdict::Disqualified { .. }
        );
        prop_assert!(disqualified);
    }
}
