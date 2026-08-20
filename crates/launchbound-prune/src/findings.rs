//! Serde model of reconverge's `findings.v1` document. Tolerant of unknown
//! fields: reconverge may grow the schema, and the gate must not silently
//! pass on a parse failure (the caller treats one as a tool error).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FindingsDoc {
    /// Schema tag; expected `findings.v1`.
    #[serde(default)]
    pub schema: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
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
