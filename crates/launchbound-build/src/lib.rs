//! cuda-oxide compile driver: specialization scratch copies, the compile
//! executor (direct or Apple-container), and the artifact cache keyed by
//! specialization source hash.

#![warn(missing_docs)]

pub mod cache;
pub mod compile;
pub mod scratch;

pub use cache::{ArtifactCache, CacheOutcome};
pub use compile::{Artifact, Compiler, Executor, entry_param_count, extract_ptx};

/// What can go wrong turning a configuration into PTX.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// The spec or one of its constraints did not load.
    #[error(transparent)]
    Space(#[from] launchbound_space::SpaceError),
    /// The per-candidate scratch copy of the kernel crate could not be
    /// made — a permissions or disk problem, not a kernel problem.
    #[error("scratch setup failed: {0}")]
    Scratch(String),
    /// A `params.rs` constant named by the spec was not found in the
    /// source, or could not be rewritten to the candidate's value.
    #[error("params rewrite failed: {0}")]
    Params(String),
    /// `cargo oxide` failed. The message carries its output, which is
    /// where a genuine kernel error appears — including the codegen-time
    /// consts `cargo check` under the reconverge driver does not evaluate.
    #[error("compile failed: {0}")]
    Compile(String),
    /// The artifact cache could not be read or written.
    #[error("artifact cache: {0}")]
    Cache(String),
}
