//! Serde model of reconverge's `findings.v1` document. Tolerant of unknown
//! fields: reconverge may grow the schema, and the gate must not silently
//! pass on a parse failure (the caller treats one as a tool error).
//!
//! **The contract is JSONL, one document per analyzed *target*.** A package
//! with a lib and a bin compiles twice and prints two lines; before 2.1.0
//! this reader handed the whole of stdout to one `from_str`, so a second
//! line was `trailing characters at line 2 column 1` — a tool error, a hard
//! stop, for every candidate of a crate that has nothing wrong with it. A
//! `src/main.rs` beside a kernel library is the ordinary shape of a GPU
//! crate: the host launcher lives there.
//!
//! reconverge 0.5.0 added `target` to distinguish the documents; it is
//! optional here because an older analyzer on someone's PATH does not write
//! it, and the union below does not depend on it.

use serde::{Deserialize, Serialize};

/// One `findings.v1` document, as reconverge prints it.
///
/// Every field is `#[serde(default)]` on purpose: this is another tool's
/// wire format, pinned but alpha, and a document that grows a field must
/// not stop parsing here. What matters is that a *missing* field never
/// reads as a clean verdict.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FindingsDoc {
    /// Schema tag; expected `findings.v1`.
    #[serde(default)]
    pub schema: String,
    /// The compiled target's crate types (`lib`, `bin`, …). reconverge
    /// 0.5.0 and later; absent from an older analyzer's output.
    #[serde(default)]
    pub target: Option<String>,
    /// Every finding in the document. Empty means the analyzer ran and
    /// found nothing — which is a clean result only if it also *ran*. An
    /// analyzer that printed no document at all is a read error, and
    /// [`decide`](crate::decide) turns that into
    /// [`Verdict::ToolError`](crate::Verdict::ToolError) rather than a
    /// pass.
    #[serde(default)]
    pub findings: Vec<Finding>,
}

/// Why a stream of findings documents could not be read.
#[derive(Debug)]
pub enum ReadError {
    /// A line was not a findings document at all.
    Parse {
        /// 1-based line number in the analyzer's output.
        line: usize,
        /// The deserialization failure.
        error: serde_json::Error,
    },
    /// A line parsed but declared a schema this build does not implement.
    Schema {
        /// 1-based line number in the analyzer's output.
        line: usize,
        /// The schema tag that was found instead of `findings.v1`.
        declared: String,
    },
    /// The analyzer printed nothing where a document was expected.
    Empty,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Parse { line, error } => {
                write!(f, "findings.v1 parse failed on line {line}: {error}")
            }
            ReadError::Schema { line, declared } => {
                write!(f, "unexpected findings schema `{declared}` on line {line}")
            }
            ReadError::Empty => write!(f, "the analyzer printed no findings document"),
        }
    }
}

/// One rule firing at one place in the kernel.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Finding {
    /// Rule ID, e.g. `RC001`.
    pub code: String,
    /// `warning`, `deny`, or `confirmed`.
    pub confidence: String,
    /// The `#[kernel]` entry this finding is about.
    #[serde(default)]
    pub kernel: String,
    /// One-line summary, e.g. `barrier under divergence`.
    #[serde(default)]
    pub message: String,
    /// Where in the source, when the analyzer could attribute it.
    #[serde(default)]
    pub span: Option<Span>,
    /// The chain of reasoning that reached this finding — the divergence
    /// source, the call path, the barrier. This is what the rejection view
    /// shows a reader who wants to know *why*.
    #[serde(default)]
    pub provenance: Vec<ProvenanceEntry>,
    /// Additional context the analyzer attached.
    #[serde(default)]
    pub notes: Vec<String>,
    /// A suggested fix, when the rule has one.
    #[serde(default)]
    pub help: Option<String>,
}

/// A source range, rendered as `file:line:column`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Span {
    /// Path as the analyzer reported it, relative to the kernel crate.
    pub file: String,
    /// 1-based start line.
    pub line_start: u32,
    /// 1-based start column.
    pub column_start: u32,
    /// 1-based end line.
    pub line_end: u32,
    /// 1-based end column.
    pub column_end: u32,
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line_start, self.column_start)
    }
}

/// One step in a finding's chain of reasoning.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProvenanceEntry {
    /// What this step is, e.g. `divergence source` or `barrier`.
    #[serde(default)]
    pub what: String,
    /// Where it is, when the analyzer could say.
    #[serde(default)]
    pub span: Option<Span>,
}

impl FindingsDoc {
    /// Parse a single document.
    ///
    /// reconverge prints JSONL — one document per compiled target — so
    /// this is the per-line step, not the way to read a whole run. The
    /// crate reads a run with its own `read_stream`, which takes the union
    /// across documents; that function and its `ReadError` are internal
    /// today, so a consumer parsing analyzer output directly gets this and
    /// splits the lines itself.
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Read reconverge's stdout as JSONL and take the union of the findings.
///
/// The union is the decision rule, not a convenience: a deny finding in
/// *any* target of the crate is a reason to refuse, and the bin target's
/// document — usually empty, since the kernels live in the lib — is
/// harmless to merge. It is also what makes a multi-crate kernel workspace
/// possible later without touching `decide`.
///
/// A line that is not a findings document is still an error, and still a
/// hard stop: `docs/SAFETY.md` §2 is explicit that unreadable analyzer
/// output is never a pass.
///
/// # Errors
///
/// The first line that does not parse, or that declares another schema,
/// naming which line it was.
pub fn read_stream(stdout: &str) -> Result<Vec<Finding>, ReadError> {
    let mut findings = Vec::new();
    let mut documents = 0;
    for (index, line) in stdout.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let number = index + 1;
        let doc = FindingsDoc::parse(line).map_err(|error| ReadError::Parse {
            line: number,
            error,
        })?;
        if doc.schema != "findings.v1" {
            return Err(ReadError::Schema {
                line: number,
                declared: doc.schema,
            });
        }
        documents += 1;
        findings.extend(doc.findings);
    }
    if documents == 0 {
        return Err(ReadError::Empty);
    }
    Ok(findings)
}

/// The first `limit` bytes of what was received, for a tool-error detail.
///
/// "trailing characters at line 2 column 1" told the person who reported
/// this everything and would tell a user nothing. Truncated on a character
/// boundary and with control bytes escaped, because this is foreign output
/// on its way to a terminal.
pub fn received_excerpt(stdout: &str, limit: usize) -> String {
    let mut out = String::new();
    for ch in stdout.chars() {
        if out.len() >= limit {
            out.push('…');
            break;
        }
        match ch {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    if out.is_empty() {
        "(nothing)".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    fn doc(krate: &str, target: &str, codes: &[&str]) -> String {
        let findings: Vec<serde_json::Value> = codes
            .iter()
            .map(|code| {
                serde_json::json!({
                    "code": code,
                    "confidence": "deny",
                    "kernel": "k",
                    "message": "m",
                    "explain": code,
                })
            })
            .collect();
        serde_json::json!({
            "schema": "findings.v1",
            "tool": { "name": "reconverge", "version": "0.5.0" },
            "crate": krate,
            "target": target,
            "findings": findings,
        })
        .to_string()
    }

    /// The shape that hard-stopped every candidate of a crate with a
    /// `src/main.rs` beside its library: two documents, one per target.
    #[test]
    fn a_lib_and_a_bin_are_two_documents_and_their_findings_union() {
        let stdout = format!(
            "{}\n{}\n",
            doc("k", "bin", &[]),
            doc("k", "lib", &["RC001"])
        );
        let findings = read_stream(&stdout).expect("two documents are the contract, not an error");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].code, "RC001");
    }

    /// A deny finding in *any* target is a reason to refuse.
    #[test]
    fn a_finding_in_either_target_reaches_the_decision() {
        let stdout = format!(
            "{}\n{}\n",
            doc("k", "lib", &["RC001"]),
            doc("k", "bin", &["RC003"])
        );
        let mut codes: Vec<String> = read_stream(&stdout)
            .unwrap()
            .into_iter()
            .map(|f| f.code)
            .collect();
        codes.sort();
        assert_eq!(codes, ["RC001", "RC003"]);
    }

    #[test]
    fn one_document_still_works_and_blank_lines_are_skipped() {
        let stdout = format!("\n{}\n\n", doc("k", "lib", &["RC002"]));
        assert_eq!(read_stream(&stdout).unwrap().len(), 1);
    }

    /// An older analyzer writes no `target`; the union does not need one.
    #[test]
    fn a_document_without_a_target_field_still_parses() {
        let stdout = r#"{"schema":"findings.v1","crate":"k","findings":[]}"#;
        assert!(read_stream(stdout).unwrap().is_empty());
        let doc = FindingsDoc::parse(stdout).unwrap();
        assert_eq!(doc.target, None);
    }

    /// Unreadable output is still a hard stop — `docs/SAFETY.md` §2 — and
    /// now says which line, so a two-document stream with one bad line is
    /// diagnosable.
    #[test]
    fn a_line_that_does_not_parse_is_an_error_naming_the_line() {
        let stdout = format!("{}\nnot json\n", doc("k", "lib", &[]));
        let err = read_stream(&stdout).unwrap_err().to_string();
        assert!(err.contains("line 2"), "{err}");
        assert!(err.contains("parse failed"), "{err}");
    }

    #[test]
    fn another_schema_is_refused_by_name() {
        let stdout = r#"{"schema":"findings.v99","crate":"k","findings":[]}"#;
        let err = read_stream(stdout).unwrap_err().to_string();
        assert!(err.contains("findings.v99"), "{err}");
    }

    #[test]
    fn no_output_at_all_is_an_error_rather_than_a_clean_pass() {
        // The direction that matters: nothing must ever read as "no
        // findings", which is a pass.
        for stdout in ["", "   ", "\n\n"] {
            assert!(read_stream(stdout).is_err(), "{stdout:?}");
        }
    }

    /// "trailing characters at line 2 column 1" told the reporter
    /// everything and would tell a user nothing.
    #[test]
    fn a_failure_shows_what_was_received() {
        let excerpt = received_excerpt("{\"schema\":\"findings.v1\"}\nsecond line\n", 200);
        assert!(excerpt.contains("findings.v1"), "{excerpt}");
        assert!(
            excerpt.contains("\\n"),
            "newlines are escaped, not printed: {excerpt}"
        );
        assert_eq!(received_excerpt("", 200), "(nothing)");
        // Bounded, and marked when it is cut.
        let long = received_excerpt(&"x".repeat(500), 200);
        assert!(
            long.ends_with('…') && long.chars().count() <= 201,
            "{}",
            long.len()
        );
        // Control bytes never reach a terminal from foreign output.
        assert!(!received_excerpt("a\u{1b}[2Jb", 200).contains('\u{1b}'));
    }
}
