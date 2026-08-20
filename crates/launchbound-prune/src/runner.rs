//! Invoking reconverge over a kernel crate, one run per specialization.
//!
//! Candidates sharing a specialization key share generated source, so the
//! analyzer runs once per key in a scratch copy of the crate (the repo copy
//! is never touched — see launchbound-build's scratch module) and the
//! decision rule is evaluated per candidate.

use crate::PruneError;
use crate::decide::{AnalyzerOutcome, Verdict, decide};
use crate::findings::FindingsDoc;
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
        match FindingsDoc::parse(stdout.trim()) {
            Ok(doc) if doc.schema == "findings.v1" => AnalyzerOutcome::Findings {
                exit_code,
                findings: doc.findings,
            },
            Ok(doc) => AnalyzerOutcome::ToolError {
                detail: format!("unexpected findings schema `{}`", doc.schema),
            },
            Err(e) => AnalyzerOutcome::ToolError {
                detail: format!("findings.v1 parse failed: {e}"),
            },
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr
            .lines()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        AnalyzerOutcome::ToolError {
            detail: format!("cargo reconverge exited {exit_code}:\n{tail}"),
        }
    }
}
