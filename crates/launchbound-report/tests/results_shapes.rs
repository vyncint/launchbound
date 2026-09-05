//! A `results.json` the report cannot read is an error, not "unmeasured".
//!
//! The run directory is the hand-off between two machines: `stage` writes
//! the plan, the measurement box writes `results.json`, and the file comes
//! back by whatever copies it. A partial copy, the wrong file, or a
//! `results.v2` from a newer runner all used to read as "the box has not run
//! yet" — exit 0, nothing on stderr, and a JSON report that validated and
//! said `unmeasured`. Those two conditions call for opposite actions: wait,
//! or go and look.

use launchbound_report::RunDir;
use std::fs;
use std::path::PathBuf;

/// A run directory with a valid `verdicts.json` and whatever `results.json`
/// the caller wants.
fn run_dir(tag: &str, results: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lb-results-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("verdicts.json"),
        serde_json::json!({
            "schema": "verdicts.v1",
            "kernel": "probe",
            "cc": "8.6",
            "candidates": [],
        })
        .to_string(),
    )
    .unwrap();
    if let Some(body) = results {
        fs::write(dir.join("results.json"), body).unwrap();
    }
    dir
}

#[test]
fn a_missing_results_file_is_still_simply_unmeasured() {
    let dir = run_dir("missing", None);
    let run = RunDir::load(&dir).expect("a run that has not been measured still loads");
    assert!(run.results.is_none());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn every_unreadable_shape_is_an_error_that_names_the_cause() {
    // The six shapes from the report, each of which rendered identically.
    for (tag, body, expect) in [
        ("truncated", "{", "not JSON"),
        ("empty", "", "not JSON"),
        ("null", "null", "no `schema` field"),
        ("array", "[]", "no `schema` field"),
        (
            "v2",
            r#"{"schema":"results.v2"}"#,
            "unsupported results schema",
        ),
        (
            "shape",
            r#"{"schema":"results.v1"}"#,
            "not a results.v1 document",
        ),
    ] {
        let dir = run_dir(tag, Some(body));
        let err = RunDir::load(&dir)
            .err()
            .unwrap_or_else(|| panic!("{tag}: an unreadable results.json must be an error"))
            .to_string();
        assert!(
            err.contains(expect),
            "{tag}: the message must name the cause, got: {err}"
        );
        assert!(err.contains("results.json"), "{tag}: and the path: {err}");
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_directory_named_results_json_is_an_error_too() {
    let dir = run_dir("isdir", None);
    fs::create_dir(dir.join("results.json")).unwrap();
    let err = RunDir::load(&dir)
        .err()
        .expect("a directory is not a document");
    assert!(err.to_string().contains("results.json"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}
