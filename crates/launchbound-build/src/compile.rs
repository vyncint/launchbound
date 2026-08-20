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
            let tail: Vec<&str> = stderr.lines().rev().take(8).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            return Err(BuildError::Compile(format!(
                "cargo oxide inspect {stem} failed (exit {:?}):\n{}",
                output.status.code(),
                tail.join("\n")
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
