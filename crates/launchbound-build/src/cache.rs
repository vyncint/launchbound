//! The artifact cache, keyed by specialization source hash. Two candidates
//! differing only in launch shape share source, and therefore an artifact.

use crate::BuildError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum CacheOutcome {
    Hit,
    Miss { compile_seconds: f64 },
}

#[derive(Debug, Serialize, Deserialize)]
struct Meta {
    kernel: String,
    source_hash: String,
    created_utc_epoch_secs: u64,
}

pub struct ArtifactCache {
    root: PathBuf,
}

impl ArtifactCache {
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

    pub fn lookup(&self, kernel: &str, hash: &str) -> Option<PathBuf> {
        let path = self.ptx_path(kernel, hash);
        path.is_file().then_some(path)
    }

    pub fn store(&self, kernel: &str, hash: &str, ptx: &str) -> Result<PathBuf, BuildError> {
        let path = self.ptx_path(kernel, hash);
        let dir = path.parent().expect("cache path has a parent");
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
        std::fs::write(
            meta_path,
            serde_json::to_string_pretty(&meta).expect("meta serializes"),
        )
        .map_err(|e| BuildError::Cache(e.to_string()))?;
        Ok(path)
    }
}
