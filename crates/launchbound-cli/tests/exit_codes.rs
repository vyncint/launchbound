//! The exit-code contract, which the README states and nothing checked.
//!
//! > Exit codes: `0` a safe configuration was found; `1` the fastest
//! > candidates were refused and the chosen one is slower than a rejected
//! > candidate — notable, not an error; `2` tool error.
//!
//! That `1` is the product's whole argument in one integer: a caller can
//! tell "tuned, and the honest answer cost you something" from "tuned, and
//! nothing was refused" without parsing a report. It is also the one a
//! refactor can quietly turn into `0`, because every output-only test
//! passes either way — the text is identical, only the status differs.
//!
//! None of this needs a GPU. `report` reads a recorded run, and the usage
//! errors are decided before any device is opened.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/launchbound-cli.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the repository root is two levels above this crate")
        .to_path_buf()
}

fn launchbound(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_launchbound"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("the binary under test runs")
}

fn code(out: &Output) -> i32 {
    out.status
        .code()
        .expect("exited rather than being signalled")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The recorded Metal sweep committed under `runs/`, which is the only run in
/// the repository and the reason these tests need no hardware.
const RECORDED_RUN: &str = "runs/reduce-stable-metal";

#[test]
fn reporting_a_recorded_run_agrees_with_its_own_json() {
    // The status is not asserted against a constant: whether this run has a
    // refused-but-faster candidate is a property of the recording, and
    // pinning the integer would make the test a copy of the fixture rather
    // than a check of the rule. So the rule is checked directly -- the exit
    // code must be 1 exactly when `rejected_faster` is non-empty.
    let json = launchbound(&["report", RECORDED_RUN, "--json"]);
    let report: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("--json prints a JSON document");
    let refused_faster = report
        .get("rejected_faster")
        .and_then(|v| v.as_array())
        .expect("the report names the refused-but-faster set")
        .len();

    let text = launchbound(&["report", RECORDED_RUN]);
    let expected = i32::from(refused_faster > 0);
    assert_eq!(
        code(&text),
        expected,
        "{refused_faster} refused-but-faster candidate(s) must mean exit {expected}"
    );
    // The format a caller asked for must not change the verdict it is told.
    assert_eq!(
        code(&json),
        code(&text),
        "--json and the text report disagreed about the exit code"
    );
}

/// Build a run directory from the recorded one with `disqualified` marked
/// refused, and return its path. Everything else is copied verbatim, so the
/// timings are real measurements from the Apple M4 Pro sweep.
fn run_with_a_refusal(name: &str, disqualified: &[String]) -> PathBuf {
    let root = repo_root();
    let dst = root.join("target/tmp").join(name);
    std::fs::create_dir_all(&dst).expect("a scratch run directory");
    std::fs::copy(
        root.join(RECORDED_RUN).join("results.json"),
        dst.join("results.json"),
    )
    .expect("the recorded results");

    let text = std::fs::read_to_string(root.join(RECORDED_RUN).join("verdicts.json"))
        .expect("the recorded verdicts");
    let mut verdicts: serde_json::Value = serde_json::from_str(&text).expect("verdicts.v1 JSON");
    let mut marked = 0;
    for c in verdicts["candidates"]
        .as_array_mut()
        .expect("verdicts name their candidates")
    {
        let config = c["config"].as_str().unwrap_or_default().to_string();
        if disqualified.contains(&config) {
            c["verdict"] = serde_json::json!("disqualified");
            c["rules"] = serde_json::json!(["RC001"]);
            marked += 1;
        }
    }
    assert_eq!(
        marked,
        disqualified.len(),
        "every named config must match exactly one candidate"
    );
    std::fs::write(
        dst.join("verdicts.json"),
        serde_json::to_string_pretty(&verdicts).expect("serializable"),
    )
    .expect("writing the doctored verdicts");
    dst
}

/// The `n` fastest configurations in the recorded sweep, by median, read from
/// it rather than hard-coded so this cannot drift if the recording is remade.
fn fastest_configs(n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join(RECORDED_RUN).join("results.json"))
        .expect("the recorded results");
    let results: serde_json::Value = serde_json::from_str(&text).expect("results.v1 JSON");
    let mut measured: Vec<(f64, String)> = results["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .filter_map(|c| {
            Some((
                c["summary"]["median_ms"].as_f64()?,
                c["config"].as_str()?.to_string(),
            ))
        })
        .collect();
    measured.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(
        measured.len() > n,
        "the recording must have more than {n} measured candidates"
    );
    measured.into_iter().take(n).map(|(_, c)| c).collect()
}

/// Exit 1 is the contract that a refactor can quietly turn into 0: the text
/// on stdout is identical either way, so every output-only test passes.
///
/// The recorded sweep has no refused candidate at all -- `gate: "none"`, the
/// Metal path has no convergence gate -- so asserting against it as-is would
/// have been exactly that kind of test. This marks the *fastest* candidate
/// refused, which is the shape the exit code exists for: the honest answer
/// is slower than something the tool would not hand you.
#[test]
fn a_refused_candidate_that_measured_faster_is_exit_one() {
    // The two fastest, not one. Their 95% intervals overlap each other
    // ([0.09679, 0.09688] both), and `rejected_faster` requires the refused
    // candidate's whole interval to sit *below* the chosen one's -- the same
    // "overlapping intervals are indistinguishable, never ranked" rule the
    // ranking uses. Refusing only the first would leave the second as the
    // chosen one and produce no gap, which is correct behaviour and would
    // have made this test pass for the wrong reason.
    let dir = run_with_a_refusal("report-refusal", &fastest_configs(2));
    let dir = dir.to_string_lossy().into_owned();

    let json = launchbound(&["report", &dir, "--json"]);
    let report: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("--json prints a JSON document");
    assert!(
        !report["rejected_faster"]
            .as_array()
            .expect("the refused-but-faster set")
            .is_empty(),
        "marking the fastest candidate refused must produce one"
    );

    let text = launchbound(&["report", &dir]);
    assert_eq!(
        code(&text),
        1,
        "a refused candidate that measured faster is exit 1, not 0:\n{}",
        String::from_utf8_lossy(&text.stdout)
    );
    assert!(
        String::from_utf8_lossy(&text.stdout).contains("REFUSED BUT FASTER"),
        "and the reader is told which one"
    );

    // `--rejected` prints the same section, and `--json` the same document;
    // both keep the status, because the format is not the verdict.
    let only = launchbound(&["report", &dir, "--rejected"]);
    assert_eq!(code(&only), 1);
    assert_eq!(code(&json), 1, "--json must carry the same exit code");
}

#[test]
fn json_and_text_report_the_same_run() {
    // A caller may read either; they must not disagree about the verdict.
    let json = launchbound(&["report", RECORDED_RUN, "--json"]);
    let text = launchbound(&["report", RECORDED_RUN]);
    assert_eq!(code(&json), code(&text), "the two formats must agree");
    assert!(
        !text.stdout.is_empty(),
        "the text report is what a human reads; it must not be empty"
    );
}

#[test]
fn a_missing_run_directory_is_a_tool_error_not_a_verdict() {
    // Exit 2 rather than 1: nothing was measured, so there is no verdict to
    // report, and a caller that treats 1 as "notable" must not see one here.
    let out = launchbound(&["report", "runs/there-is-no-such-run"]);
    assert_eq!(code(&out), 2, "stderr was:\n{}", stderr(&out));
    assert!(
        stderr(&out).starts_with("error:"),
        "the diagnosis must be the first thing on stderr:\n{}",
        stderr(&out)
    );
}

#[test]
fn allow_unsafe_without_a_reason_is_a_usage_error() {
    // From the README: "it requires `--reason` with a non-empty string,
    // recorded verbatim in the report. A missing reason is a usage error,
    // not a warning." Measuring a configuration the gate refused is a
    // deliberate act, and the deliberation is the string.
    // `--out` is supplied because clap requires it, and a clap error about a
    // missing `--out` would let this test pass without ever reaching the
    // check it is about. Nothing is written there: the reason is validated
    // first, which is the other half of the contract.
    let out = launchbound(&[
        "stage",
        "reduce-flip",
        "--cc",
        "8.6",
        "--out",
        "target/tmp/stage-probe",
        "--allow-unsafe",
    ]);
    assert_eq!(code(&out), 2, "stderr was:\n{}", stderr(&out));
    assert!(
        !Path::new("target/tmp/stage-probe").exists(),
        "a refused usage must not have created the output directory"
    );
    assert!(
        stderr(&out).contains("--reason"),
        "the message must name the flag that is missing:\n{}",
        stderr(&out)
    );
}

#[test]
fn a_blank_reason_is_the_same_as_no_reason() {
    // Whitespace is not an explanation, and a report carrying `reason: "  "`
    // is worse than one that was never produced: it looks deliberated.
    for blank in ["", "   "] {
        let out = launchbound(&[
            "stage",
            "reduce-flip",
            "--cc",
            "8.6",
            "--out",
            "target/tmp/stage-probe",
            "--allow-unsafe",
            "--reason",
            blank,
        ]);
        assert_eq!(code(&out), 2, "blank reason {blank:?} was accepted");
        assert!(stderr(&out).contains("--reason"), "{}", stderr(&out));
    }
}

#[test]
fn a_reason_without_allow_unsafe_is_refused_too() {
    // The other direction. A `--reason` alone means the caller believes they
    // asked for something they did not ask for.
    let out = launchbound(&[
        "stage",
        "reduce-flip",
        "--cc",
        "8.6",
        "--out",
        "target/tmp/stage-probe",
        "--reason",
        "measuring the refusal",
    ]);
    assert_eq!(code(&out), 2, "stderr was:\n{}", stderr(&out));
}

#[test]
fn tune_has_no_allow_unsafe_at_all() {
    // "`--allow-unsafe` is on `stage`, not on `tune`" -- the flag a user
    // reaches for when a refusal is inconvenient must not exist on the
    // command whose answer they act on.
    let out = launchbound(&["tune", "reduce-flip", "--cc", "8.6", "--allow-unsafe"]);
    assert_eq!(code(&out), 2, "stderr was:\n{}", stderr(&out));
    let err = stderr(&out);
    assert!(
        err.contains("unexpected argument") || err.contains("--allow-unsafe"),
        "the refusal should be about the flag itself:\n{err}"
    );
}

#[test]
fn a_malformed_cc_is_a_tool_error_before_anything_is_spawned() {
    // `prune` used to hand whatever was typed straight to `cargo reconverge`,
    // once per candidate: a mistyped `--cc 80` spawned eleven subprocesses
    // and printed ninety lines in which the actual problem appeared nowhere.
    let out = launchbound(&["prune", "corpus/reduce-flip", "--cc", "8.x"]);
    assert_eq!(code(&out), 2, "stderr was:\n{}", stderr(&out));
    let err = stderr(&out);
    assert!(
        err.contains("compute capability"),
        "the message must say what a --cc is:\n{err}"
    );
    assert!(
        err.contains("sm_86"),
        "and name the spellings that would have worked:\n{err}"
    );
}

#[test]
fn help_and_version_succeed() {
    for args in [vec!["--help"], vec!["--version"], vec!["report", "--help"]] {
        let out = launchbound(&args);
        assert_eq!(code(&out), 0, "{args:?} failed:\n{}", stderr(&out));
        assert!(!out.stdout.is_empty(), "{args:?} printed nothing");
    }
}

#[test]
fn enumerating_a_corpus_kernel_needs_no_gpu_and_no_analyzer() {
    // `space` is the one command with no gate and no device behind it, so it
    // is the cheapest end-to-end check that the corpus still parses.
    let out = launchbound(&["space", "corpus/reduce-flip"]);
    assert_eq!(code(&out), 0, "stderr was:\n{}", stderr(&out));
    assert!(
        !out.stdout.is_empty(),
        "the space is the output; it must not be empty"
    );
}
