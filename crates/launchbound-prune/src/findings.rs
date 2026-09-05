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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FindingsDoc {
    /// Schema tag; expected `findings.v1`.
    #[serde(default)]
    pub schema: String,
    /// The compiled target's crate types (`lib`, `bin`, …). reconverge
    /// 0.5.0 and later; absent from an older analyzer's output.
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub findings: Vec<Finding>,
}

/// Why a stream of findings documents could not be read.
#[derive(Debug)]
pub enum ReadError {
    /// A line was not a findings document at all.
    Parse {
        line: usize,
        error: serde_json::Error,
    },
    /// A line parsed but declared a schema this build does not implement.
    Schema { line: usize, declared: String },
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Finding {
    /// Rule ID, e.g. `RC001`.
    pub code: String,
    /// `warning`, `deny`, or `confirmed`.
    pub confidence: String,
    #[serde(default)]
    pub kernel: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub span: Option<Span>,
    #[serde(default)]
    pub provenance: Vec<ProvenanceEntry>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub help: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Span {
    pub file: String,
    pub line_start: u32,
    pub column_start: u32,
    pub line_end: u32,
    pub column_end: u32,
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line_start, self.column_start)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProvenanceEntry {
    #[serde(default)]
    pub what: String,
    #[serde(default)]
    pub span: Option<Span>,
}

impl FindingsDoc {
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
