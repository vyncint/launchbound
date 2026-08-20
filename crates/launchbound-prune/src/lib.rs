//! The safety gate: invoke reconverge, parse `findings.v1`, decide.
//!
//! The decision rule lives in [`decide`] and is specified in
//! `docs/SAFETY.md`. It is a pure function; the impure parts (scratch
//! copies, process invocation) live in the runner module.

mod decide;
mod findings;
mod runner;

pub use decide::{AnalyzerOutcome, CaveatRecord, RejectionRecord, Verdict, WARP_SIZE, decide};
pub use findings::{Finding, FindingsDoc, ProvenanceEntry, Span};
pub use runner::{CandidateVerdict, PruneOptions, prune_kernel};

#[derive(Debug, thiserror::Error)]
pub enum PruneError {
    #[error(transparent)]
    Space(#[from] launchbound_space::SpaceError),
    #[error(transparent)]
    Build(#[from] launchbound_build::BuildError),
}
