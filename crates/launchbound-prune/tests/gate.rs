//! The gate tests — the most important tests in the repo (CONTRIBUTING.md).
//!
//! They need `cargo-reconverge` and the sibling cuda-oxide checkout, so
//! they are `#[ignore]` for plain `cargo test` (CI stays green with no
//! toolchain) and run via `just gate`, which prune.yml provisions for.

use launchbound_prune::{PruneOptions, Verdict, prune_kernel};
use launchbound_space::KernelSpec;
use std::path::PathBuf;

fn corpus(kernel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(kernel)
}

fn options() -> PruneOptions {
    PruneOptions {
        cc: "8.6".into(),
        reconverge_dir: None, // LAUNCHBOUND_RECONVERGE or PATH
        scratch_root: None,
    }
}

/// The S2 gate, flip direction: block=64 and block=128 (and 256)
/// disqualified, block=32 admitted — reproducing simt-diff's 11-of-147
/// direction on this corpus.
#[test]
#[ignore = "needs cargo-reconverge and the sibling cuda-oxide checkout (just gate)"]
fn known_flip_kernel_disqualifies_above_one_warp() {
    let spec = KernelSpec::load(&corpus("reduce-flip")).unwrap();
    let verdicts = prune_kernel(&spec, &options()).unwrap();
    assert!(!verdicts.is_empty());
    for cv in &verdicts {
        let block = cv.config.block_threads();
        match block {
            32 => assert!(
                matches!(cv.verdict, Verdict::Clean),
                "block=32 must be admitted clean, got {:?}",
                cv.verdict
            ),
            _ => {
                let Verdict::Disqualified { records } = &cv.verdict else {
                    panic!("block={block} must be disqualified, got {:?}", cv.verdict);
                };
                assert!(records.iter().any(|r| r.rule == "RC001"));
                assert!(
                    records
                        .iter()
                        .any(|r| r.span.as_deref().is_some_and(|s| s.contains("lib.rs"))),
                    "rejection must point at the source"
                );
            }
        }
    }
}

/// The S2 gate, stable direction: nothing disqualified at any block size.
#[test]
#[ignore = "needs cargo-reconverge and the sibling cuda-oxide checkout (just gate)"]
fn known_stable_kernel_disqualifies_nothing() {
    let spec = KernelSpec::load(&corpus("reduce-stable")).unwrap();
    let verdicts = prune_kernel(&spec, &options()).unwrap();
    assert!(!verdicts.is_empty());
    for cv in &verdicts {
        assert!(
            matches!(cv.verdict, Verdict::Clean),
            "reduce-stable {} must be clean, got {:?}",
            cv.config,
            cv.verdict
        );
    }
}

/// A reconverge exit 2 is surfaced as a hard stop, proven by forcing one
/// with a fixture crate that cannot compile.
#[test]
#[ignore = "needs cargo-reconverge and the sibling cuda-oxide checkout (just gate)"]
fn tool_error_is_a_hard_stop() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/broken-kernel");
    let spec = KernelSpec::load(&fixture).unwrap();
    let verdicts = prune_kernel(&spec, &options()).unwrap();
    assert!(!verdicts.is_empty());
    for cv in &verdicts {
        assert!(
            matches!(cv.verdict, Verdict::ToolError { .. }),
            "broken kernel must be a tool error, got {:?}",
            cv.verdict
        );
    }
}
