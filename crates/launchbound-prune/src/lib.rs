//! The safety gate: invoke reconverge, parse `findings.v1`, decide.
//!
//! The decision rule lives in [`decide`] and is specified in
//! `docs/SAFETY.md`. It is a pure function; the impure parts (scratch
//! copies, process invocation) live in the runner module.

#![warn(missing_docs)]

mod decide;
mod findings;
mod runner;

pub use decide::{AnalyzerOutcome, CaveatRecord, RejectionRecord, Verdict, WARP_SIZE, decide};
pub use findings::{Finding, FindingsDoc, ProvenanceEntry, Span};
pub use runner::{CandidateVerdict, PruneOptions, prune_kernel};

/// What can go wrong running the gate over a space.
///
/// Note what is *not* here: an analyzer that could not answer is a
/// [`Verdict::ToolError`] on that candidate, not an error on the sweep. The
/// distinction is deliberate — one unanalyzable configuration must not
/// discard the verdicts of the others, and it must not be reported as
/// clean either.
#[derive(Debug, thiserror::Error)]
pub enum PruneError {
    /// The spec would not load, or a constraint would not evaluate.
    #[error(transparent)]
    Space(#[from] launchbound_space::SpaceError),
    /// The per-candidate source could not be specialized.
    #[error(transparent)]
    Build(#[from] launchbound_build::BuildError),
}
