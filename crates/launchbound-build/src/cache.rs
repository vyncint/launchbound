//! The artifact cache, keyed by specialization source hash. Two candidates
//! differing only in launch shape share source, and therefore an artifact.

use crate::BuildError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Whether a candidate's PTX was already on disk.
///
/// Reported per candidate so a sweep's wall-clock can be read honestly: a
/// run that is mostly hits spent its time measuring, one that is mostly
/// misses spent it compiling.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheOutcome {
    /// The PTX was cached; nothing was compiled.
    Hit,
    /// The PTX had to be built.
    Miss {
        /// Wall-clock seconds `cargo oxide` took, for the sweep's tally.
        compile_seconds: f64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Meta {
    kernel: String,
    source_hash: String,
    created_utc_epoch_secs: u64,
}

/// Content-addressed store of compiled PTX, keyed by kernel name and a
/// hash of the source that produced it.
///
/// A specialization is expensive to compile and perfectly reproducible, so
/// the same source hash always yields the same PTX. Entries are never
/// invalidated: a changed source is a different hash and therefore a
/// different file, which means a stale entry is unreachable rather than
/// wrong.
pub struct ArtifactCache {
    root: PathBuf,
}

impl ArtifactCache {
    /// A cache rooted at exactly `root`.
    pub fn new(root: PathBuf) -> Self {
        ArtifactCache { root }
    }

    /// Default cache root under a directory (usually the kernel's target/).
    pub fn under(dir: &Path) -> Self {
        ArtifactCache {
            root: dir.join("launchbound-cache"),
        }
    }

    fn ptx_path(&self, kernel: &str, hash: &str) -> PathBuf {
        self.root.join(kernel).join(format!("{hash}.ptx"))
    }

    /// The cached PTX for this (kernel, source hash), if it is on disk.
    ///
    /// Returns `None` for a miss rather than an error: a missing entry is
    /// the normal first-run state, not a failure.
    pub fn lookup(&self, kernel: &str, hash: &str) -> Option<PathBuf> {
        let path = self.ptx_path(kernel, hash);
        path.is_file().then_some(path)
    }

    /// Write `ptx` under this (kernel, source hash) and return its path.
    ///
    /// A sibling `.meta.json` records the kernel, the hash and the time,
    /// so an operator can tell what a cache directory holds without
    /// reading PTX.
    pub fn store(&self, kernel: &str, hash: &str, ptx: &str) -> Result<PathBuf, BuildError> {
        let path = self.ptx_path(kernel, hash);
        // `ptx_path` always joins at least one component, so this holds — but
        // it holds in *another function*, and `store` already returns a
        // `Result`. Say it here rather than assert it from a distance.
        let dir = path
            .parent()
            .ok_or_else(|| BuildError::Cache(format!("{} has no parent", path.display())))?;
        std::fs::create_dir_all(dir).map_err(|e| BuildError::Cache(e.to_string()))?;
        std::fs::write(&path, ptx).map_err(|e| BuildError::Cache(e.to_string()))?;
        let meta = Meta {
            kernel: kernel.to_string(),
            source_hash: hash.to_string(),
            created_utc_epoch_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };
        let meta_path = path.with_extension("meta.json");
        let meta_json = serde_json::to_string_pretty(&meta)
            .map_err(|e| BuildError::Cache(format!("serializing cache metadata: {e}")))?;
        std::fs::write(meta_path, meta_json).map_err(|e| BuildError::Cache(e.to_string()))?;
        Ok(path)
    }
}
