//! Invoking reconverge over a kernel crate, one run per specialization.
//!
//! Candidates sharing a specialization key share generated source, so the
//! analyzer runs once per key in a scratch copy of the crate (the repo copy
//! is never touched — see launchbound-build's scratch module) and the
//! decision rule is evaluated per candidate.

use crate::PruneError;
use crate::decide::{AnalyzerOutcome, Verdict, decide};
use crate::findings;
use launchbound_build::scratch::{default_scratch_root, prepare_scratch, write_params};
use launchbound_space::{Config, KernelSpec, enumerate};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct PruneOptions {
    /// Target compute capability, e.g. "8.6". Mandatory: RC004's
    /// shared-memory context depends on it (docs/SAFETY.md).
    pub cc: String,
    /// Directory containing the `cargo-reconverge` binary, prepended to
    /// PATH. Falls back to the LAUNCHBOUND_RECONVERGE env var, then PATH.
    pub reconverge_dir: Option<PathBuf>,
    /// Scratch root; defaults to `<kernel>/target/launchbound-scratch`.
    pub scratch_root: Option<PathBuf>,
}

/// The gate's output for one candidate.
#[derive(Debug, Clone)]
pub struct CandidateVerdict {
    pub config: Config,
    pub verdict: Verdict,
}

/// Run the gate over a kernel's whole configuration space.
pub fn prune_kernel(
    spec: &KernelSpec,
    options: &PruneOptions,
) -> Result<Vec<CandidateVerdict>, PruneError> {
    let configs = enumerate(spec)?;
    let scratch_root = options
        .scratch_root
        .clone()
        .unwrap_or_else(|| default_scratch_root(spec));
    let scratch = prepare_scratch(spec, &scratch_root)?;

    let mut outcomes: BTreeMap<String, AnalyzerOutcome> = BTreeMap::new();
    let mut results = Vec::with_capacity(configs.len());
    for config in configs {
        let key = config.spec_key(spec);
        if !outcomes.contains_key(&key) {
            write_params(spec, &config, &scratch)?;
            let outcome = run_reconverge(&scratch, options);
            outcomes.insert(key.clone(), outcome);
        }
        let verdict = decide(&outcomes[&key], &config);
        results.push(CandidateVerdict { config, verdict });
    }
    Ok(results)
}

/// Run `cargo reconverge check` in `dir`. Exit 2 or unparseable output is a
/// tool error — a hard stop, never a pass by omission (docs/SAFETY.md).
///
/// A failure names `dir`, which is the *scratch* copy rather than the
/// kernel crate. The distinction is the whole diagnosis when the two
/// differ: the gate used to report `error: could not compile` against a
/// crate whose own `cargo check` is clean, and never said that what it
/// compiled was not what the reader was looking at. `cd` there and see.
fn run_reconverge(dir: &Path, options: &PruneOptions) -> AnalyzerOutcome {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "reconverge",
        "check",
        "--strict",
        "--message-format",
        "json",
    ])
    .args(["--cc", &options.cc])
    .current_dir(dir);

    let extra_dir = options
        .reconverge_dir
        .clone()
        .or_else(|| std::env::var_os("LAUNCHBOUND_RECONVERGE").map(PathBuf::from));
    if let Some(extra) = extra_dir {
        let current = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![extra];
        paths.extend(std::env::split_paths(&current));
        if let Ok(joined) = std::env::join_paths(paths) {
            cmd.env("PATH", joined);
        }
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return AnalyzerOutcome::ToolError {
                detail: format!("failed to spawn cargo reconverge: {e}"),
            };
        }
    };
    let exit_code = output.status.code().unwrap_or(-1);
    if exit_code == 0 || exit_code == 1 {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // JSONL, one document per analyzed target — reconverge's documented
        // contract, and the shape a package with a lib and a bin produces.
        match findings::read_stream(&stdout) {
            Ok(findings) => AnalyzerOutcome::Findings {
                exit_code,
                findings,
            },
            Err(e) => AnalyzerOutcome::ToolError {
                detail: format!(
                    "{e}\n  received: {}",
                    findings::received_excerpt(&stdout, 200)
                ),
            },
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        AnalyzerOutcome::ToolError {
            detail: format!(
                "cargo reconverge exited {exit_code} in {}:\n{}",
                dir.display(),
                diagnosis(&stderr)
            ),
        }
    }
}

/// The lines of `stderr` that say what went wrong.
///
/// reconverge marks them: they begin `error:`. Taking the *last* six lines
/// instead — a reasonable-looking default, since a failing tool usually
/// fails last — reliably picked the wrong six, because reconverge prints
/// its diagnosis first and its usage reference after it. A mistyped `--cc`
/// came back as the exit-code legend, once per candidate, with the sentence
/// that would have solved it forty-odd lines out of view.
///
/// reconverge 0.4.0 stopped printing usage after a bad *value*, which fixes
/// that case at the source. This is still the right way to read it: no
/// caller can control what its analyzer prints, and an older reconverge on
/// someone's PATH is exactly when a clear message matters most.
///
/// Falls back to the *head* rather than the tail when nothing is marked —
/// a tool that prints a reference puts the reason before it.
///
/// The marker has to be **both** forms. rustc's primary diagnostics begin
/// `error[E0583]:` — a code in brackets before the colon — so a filter on
/// `error:` alone kept cargo's summary line and dropped the line that names
/// the failure. "See the errors above": the one that was above was the one
/// the filter removed. reconverge's own `error:` lines came through, so the
/// filter worked for the analyzer and failed for the compiler, which is the
/// common case of a gate tool error on a kernel that has one.
fn diagnosis(stderr: &str) -> String {
    let marked: Vec<&str> = stderr
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("error:") || line.starts_with("error[")
        })
        .collect();
    if !marked.is_empty() {
        return marked.join("\n");
    }
    let head: Vec<&str> = stderr
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(6)
        .collect();
    if head.is_empty() {
        "(no output on stderr)".to_string()
    } else {
        head.join("\n")
    }
}

#[cfg(test)]
mod diagnosis_tests {
    use super::diagnosis;

    /// The shape that caused this: the reason first, the reference after.
    #[test]
    fn the_marked_line_is_taken_however_far_from_the_end_it_is() {
        let stderr = format!(
            "error: `80` is not a compute capability; expected e.g. `8.6`\n\n{}",
            "usage line\n".repeat(44)
        );
        assert_eq!(
            diagnosis(&stderr),
            "error: `80` is not a compute capability; expected e.g. `8.6`"
        );
    }

    #[test]
    fn every_marked_line_is_kept() {
        let stderr = "error: first\nnoise\nerror: second\n";
        assert_eq!(diagnosis(stderr), "error: first\nerror: second");
    }

    #[test]
    fn unmarked_output_falls_back_to_the_head_not_the_tail() {
        let stderr = "the reason\n\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\n";
        assert!(diagnosis(stderr).starts_with("the reason"));
        assert!(!diagnosis(stderr).contains("line 8"));
    }

    #[test]
    fn empty_stderr_says_so_rather_than_nothing() {
        assert_eq!(diagnosis(""), "(no output on stderr)");
        assert_eq!(diagnosis("\n  \n"), "(no output on stderr)");
    }
}
