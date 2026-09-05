//! The compile driver: turn a specialized scratch crate into PTX through
//! `cargo oxide`, behind an executor so the same code runs on a Linux box
//! (direct) and on macOS (inside the Apple `container` guest, where the
//! host's ~/Projects is mounted at the identical absolute path).

use crate::BuildError;
use crate::cache::{ArtifactCache, CacheOutcome};
use crate::scratch::{prepare_scratch, source_hash, write_params};
use launchbound_space::{Config, KernelSpec};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// How to reach a working `cargo oxide`.
#[derive(Debug, Clone)]
pub enum Executor {
    /// Run `bash -lc <script>` directly (a Linux host with the toolchain).
    Direct,
    /// Run inside an Apple `container` guest: `container exec <name> bash -lc`.
    Container { name: String },
}

impl Executor {
    /// Default for this host: the container on macOS, direct elsewhere.
    /// LAUNCHBOUND_OXIDE_CONTAINER overrides the container name;
    /// LAUNCHBOUND_OXIDE_DIRECT=1 forces direct.
    pub fn detect() -> Self {
        if std::env::var_os("LAUNCHBOUND_OXIDE_DIRECT").is_some() {
            return Executor::Direct;
        }
        if let Ok(name) = std::env::var("LAUNCHBOUND_OXIDE_CONTAINER") {
            return Executor::Container { name };
        }
        if cfg!(target_os = "macos") {
            Executor::Container {
                name: "cuda-oxide-dev".into(),
            }
        } else {
            Executor::Direct
        }
    }

    fn run_script(&self, script: &str) -> Result<std::process::Output, BuildError> {
        let mut cmd = match self {
            Executor::Direct => {
                let mut c = Command::new("bash");
                c.args(["-lc", script]);
                c
            }
            Executor::Container { name } => {
                let mut c = Command::new("container");
                c.args(["exec", name, "bash", "-lc", script]);
                c
            }
        };
        cmd.output()
            .map_err(|e| BuildError::Compile(format!("failed to spawn executor: {e}")))
    }
}

pub struct Compiler {
    pub executor: Executor,
    pub cache: ArtifactCache,
    /// Compiles performed (cache misses); lets tests prove a hit did no work.
    pub compiles: u32,
}

/// One compiled artifact.
#[derive(Debug, Clone)]
pub struct Artifact {
    pub ptx_path: PathBuf,
    pub source_hash: String,
    pub outcome: CacheOutcome,
}

impl Compiler {
    pub fn new(executor: Executor, cache: ArtifactCache) -> Self {
        Compiler {
            executor,
            cache,
            compiles: 0,
        }
    }

    /// Compile (or fetch from cache) the artifact for `config`'s
    /// specialization. Candidates sharing a spec key share the artifact.
    pub fn compile(
        &mut self,
        spec: &KernelSpec,
        config: &Config,
        scratch_root: &Path,
    ) -> Result<Artifact, BuildError> {
        let scratch = prepare_scratch(spec, scratch_root)?;
        write_params(spec, config, &scratch)?;
        let hash = source_hash(&scratch)?;

        if let Some(ptx_path) = self.cache.lookup(&spec.name, &hash) {
            return Ok(Artifact {
                ptx_path,
                source_hash: hash,
                outcome: CacheOutcome::Hit,
            });
        }

        let started = Instant::now();
        let ptx = self.invoke_oxide(&scratch, &spec.name)?;
        self.compiles += 1;
        let ptx_path = self.cache.store(&spec.name, &hash, &ptx)?;
        Ok(Artifact {
            ptx_path,
            source_hash: hash,
            outcome: CacheOutcome::Miss {
                compile_seconds: started.elapsed().as_secs_f64(),
            },
        })
    }

    /// Run `cargo oxide inspect <crate-stem>` in the scratch crate and read
    /// the PTX artifact it writes at `<scratch>/<stem>.ptx`. cargo-oxide
    /// names external-project artifacts after the crate, not the entry.
    fn invoke_oxide(&self, scratch: &Path, crate_name: &str) -> Result<String, BuildError> {
        let stem = crate_name.replace('-', "_");
        let script = format!("cd {} && cargo oxide inspect {stem}", scratch.display());
        let output = self.executor.run_script(&script)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // A missing subcommand is not a compile failure, and cargo's own
            // help for it sends the reader to `cargo search cargo-oxide` —
            // a package that is not on crates.io. This tool knows the pin
            // and can answer properly.
            if is_missing_subcommand(&stderr) {
                return Err(BuildError::Compile(missing_oxide_message()));
            }
            return Err(BuildError::Compile(format!(
                "cargo oxide inspect {stem} failed (exit {}):\n{}",
                exit_label(output.status.code()),
                diagnosis(&stderr)
            )));
        }
        let artifact = scratch.join(format!("{stem}.ptx"));
        std::fs::read_to_string(&artifact).map_err(|e| {
            BuildError::Compile(format!(
                "cargo oxide succeeded but {} is unreadable: {e}",
                artifact.display()
            ))
        })
    }
}

/// Did cargo report that it has no `oxide` subcommand?
fn is_missing_subcommand(stderr: &str) -> bool {
    stderr.contains("no such command: `oxide`") || stderr.contains("no such subcommand: `oxide`")
}

/// The pinned cuda-oxide commit, kept beside the pin sites the policy names.
///
/// Named here so a pin bump moves the message with it — `check-pins.sh`
/// asserts this constant against `rust-toolchain.toml` and the workflows,
/// because drift between recorded pins is the failure this repository keeps
/// hitting.
pub const CUDA_OXIDE_PIN: &str = "50d07314eb8b7d5ec821ba02b0048a753c20dd4e";

/// What to say when `cargo oxide` is not installed.
fn missing_oxide_message() -> String {
    format!(
        "`cargo oxide` is not installed — the gate can compile nothing \
         without it\n\n  \
         cuda-oxide is pinned to {pin} and is NOT published to crates.io, so \
         `cargo search cargo-oxide` (which cargo suggests) finds nothing. \
         Check it out beside this repository, as CI does, and install its \
         cargo subcommand:\n\n    \
         git clone https://github.com/NVlabs/cuda-oxide ../cuda-oxide\n    \
         git -C ../cuda-oxide checkout {short}\n    \
         cargo install --path ../cuda-oxide/crates/cargo-oxide\n\n  \
         `launchbound prune` needs none of this — it is the whole pipeline a \
         laptop can run.",
        pin = CUDA_OXIDE_PIN,
        short = &CUDA_OXIDE_PIN[..8],
    )
}

/// An exit code, or what to say when there was not one.
///
/// `{:?}` on an `Option<i32>` printed `exit Some(101)` at a user.
fn exit_label(code: Option<i32>) -> String {
    match code {
        Some(code) => code.to_string(),
        // Unix only, but the string is honest anywhere.
        None => "killed by a signal".to_string(),
    }
}

/// The lines of a failing compiler's stderr that say what went wrong.
///
/// The same reasoning as `launchbound_prune::diagnosis`, and the same bug:
/// the tail of a failing compile is `error: could not compile … due to N
/// previous errors`, and the N errors are above the cut. rustc's primary
/// diagnostics carry a code — `error[E0583]:` — so both marker forms are
/// accepted, and the fallback is the head rather than the tail.
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
        .take(8)
        .collect();
    if head.is_empty() {
        "(no output on stderr)".to_string()
    } else {
        head.join("\n")
    }
}

/// Extract the PTX document (from the NVPTX header or `.version`) out of
/// mixed build output.
pub fn extract_ptx(stdout: &str) -> Option<String> {
    let start = stdout
        .find("// Generated by LLVM NVPTX Back-End")
        .or_else(|| stdout.find("\n.version").map(|i| i + 1))?;
    let doc = &stdout[start..];
    doc.contains(".entry")
        .then(|| doc.trim_end().to_string() + "\n")
}

/// Parse the `.entry` parameter list for `entry` out of a PTX document:
/// the number of `.param` slots, used to validate bench plans against the
/// real ABI.
pub fn entry_param_count(ptx: &str, entry: &str) -> Option<usize> {
    let needle = format!(".entry {entry}(");
    let start = ptx.find(&needle)?;
    let rest = &ptx[start..];
    let close = rest.find(')')?;
    Some(rest[..close].matches(".param").count())
}
