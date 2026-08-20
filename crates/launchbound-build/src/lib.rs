//! cuda-oxide compile driver: specialization scratch copies, the compile
//! executor (direct or Apple-container), and the artifact cache keyed by
//! specialization source hash.

pub mod cache;
pub mod compile;
pub mod scratch;

pub use cache::{ArtifactCache, CacheOutcome};
pub use compile::{Artifact, Compiler, Executor, entry_param_count, extract_ptx};

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(transparent)]
    Space(#[from] launchbound_space::SpaceError),
    #[error("scratch setup failed: {0}")]
    Scratch(String),
    #[error("params rewrite failed: {0}")]
    Params(String),
    #[error("compile failed: {0}")]
    Compile(String),
    #[error("artifact cache: {0}")]
    Cache(String),
}
