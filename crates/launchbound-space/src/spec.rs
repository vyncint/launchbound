//! Kernel spec: the `kernel.toml` sitting next to a corpus kernel's source.

use crate::SpaceError;
use crate::constraint::Constraint;
use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};

/// One value a dimension can take.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    Int(u64),
    Str(String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Str(s) => f.write_str(s),
        }
    }
}

/// What a dimension controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimRole {
    /// Launch geometry only: no source change, no recompile. `block_x`,
    /// `block_y`, `block_z` default to this.
    Launch,
    /// Compile-time specialization: a distinct generated source, therefore a
    /// distinct reconverge verdict and a distinct compiled artifact.
    Spec,
}

/// One tunable dimension.
#[derive(Debug, Clone)]
pub struct Dim {
    pub name: String,
    pub role: DimRole,
    pub values: Vec<Value>,
}

/// What the corpus documents about a kernel's expected gate behaviour, so
/// the gate is tested in both directions (corpus/README.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyExpectation {
    /// Known to flip safety with block size (the §2.2 family).
    Flip,
    /// Known not to flip at any block size in its space.
    Stable,
    /// No documented expectation.
    None,
}

/// A kernel's declared tuning space, loaded from `kernel.toml`.
#[derive(Debug, Clone)]
pub struct KernelSpec {
    pub name: String,
    /// The `#[kernel]` entry function name.
    pub entry: String,
    /// Launch-contract domain (1, 2 or 3).
    pub domain: u8,
    /// Minimum compute capability the kernel needs, e.g. "7.0".
    pub needs_cc: Option<String>,
    pub known: SafetyExpectation,
    /// Directory holding the kernel crate (where kernel.toml lives).
    pub dir: PathBuf,
    pub dims: Vec<Dim>,
    pub constraints: Vec<Constraint>,
}

#[derive(Deserialize)]
struct RawSpec {
    kernel: RawKernel,
    #[serde(default)]
    dims: toml::value::Table,
    #[serde(default)]
    constraints: RawConstraints,
}

#[derive(Deserialize)]
struct RawKernel {
    name: String,
    entry: String,
    domain: u8,
    #[serde(default)]
    needs_cc: Option<String>,
    #[serde(default)]
    known: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawConstraints {
    #[serde(default)]
    exprs: Vec<String>,
}

#[derive(Deserialize)]
struct RawDim {
    #[serde(default)]
    role: Option<String>,
    values: Vec<toml::Value>,
}

impl KernelSpec {
    /// Load `<dir>/kernel.toml`.
    pub fn load(dir: &Path) -> Result<Self, SpaceError> {
        let path = dir.join("kernel.toml");
        let text = std::fs::read_to_string(&path).map_err(|source| SpaceError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&path.display().to_string(), &text, dir.to_path_buf())
    }

    /// Parse from a string (tests).
    pub fn from_toml_str(origin: &str, text: &str) -> Result<Self, SpaceError> {
        Self::parse(origin, text, PathBuf::from("."))
    }

    fn parse(origin: &str, text: &str, dir: PathBuf) -> Result<Self, SpaceError> {
        let raw: RawSpec = toml::from_str(text).map_err(|source| SpaceError::Parse {
            path: origin.to_string(),
            source: Box::new(source),
        })?;

        if !(1..=3).contains(&raw.kernel.domain) {
            return Err(SpaceError::Invalid(format!(
                "domain must be 1..=3, got {}",
                raw.kernel.domain
            )));
        }
        let known = match raw.kernel.known.as_deref() {
            Some("flip") => SafetyExpectation::Flip,
            Some("stable") => SafetyExpectation::Stable,
            None => SafetyExpectation::None,
            Some(other) => {
                return Err(SpaceError::Invalid(format!(
                    "known must be \"flip\" or \"stable\", got {other:?}"
                )));
            }
        };

        let mut dims = Vec::new();
        for (name, value) in raw.dims {
            if !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                return Err(SpaceError::Invalid(format!(
                    "dimension name {name:?} must be [a-z0-9_]"
                )));
            }
            let raw_dim: RawDim = value
                .try_into()
                .map_err(|e| SpaceError::Invalid(format!("dimension {name}: {e}")))?;
            let role = match raw_dim.role.as_deref() {
                Some("launch") => DimRole::Launch,
                Some("spec") => DimRole::Spec,
                None if name.starts_with("block_") => DimRole::Launch,
                None => DimRole::Spec,
                Some(other) => {
                    return Err(SpaceError::Invalid(format!(
                        "dimension {name}: role must be \"launch\" or \"spec\", got {other:?}"
                    )));
                }
            };
            let mut values = Vec::new();
            for v in raw_dim.values {
                match v {
                    toml::Value::Integer(n) if n >= 0 => values.push(Value::Int(n as u64)),
                    toml::Value::String(s) => values.push(Value::Str(s)),
                    other => {
                        return Err(SpaceError::Invalid(format!(
                            "dimension {name}: values must be non-negative integers or strings, got {other}"
                        )));
                    }
                }
            }
            if values.is_empty() {
                return Err(SpaceError::Invalid(format!(
                    "dimension {name} has no values"
                )));
            }
            let mut seen = values.clone();
            seen.sort();
            seen.dedup();
            if seen.len() != values.len() {
                return Err(SpaceError::Invalid(format!(
                    "dimension {name} has duplicate values"
                )));
            }
            dims.push(Dim { name, role, values });
        }
        if dims.is_empty() {
            return Err(SpaceError::Invalid("spec declares no dimensions".into()));
        }
        dims.sort_by(|a, b| a.name.cmp(&b.name));

        let dim_names: Vec<&str> = dims.iter().map(|d| d.name.as_str()).collect();
        let constraints = raw
            .constraints
            .exprs
            .iter()
            .map(|e| Constraint::parse(e, &dim_names))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(KernelSpec {
            name: raw.kernel.name,
            entry: raw.kernel.entry,
            domain: raw.kernel.domain,
            needs_cc: raw.kernel.needs_cc,
            known,
            dir,
            dims,
            constraints,
        })
    }

    pub fn dim(&self, name: &str) -> Option<&Dim> {
        self.dims.iter().find(|d| d.name == name)
    }

    /// Dimensions in canonical (name-sorted) order.
    pub fn dims_sorted(&self) -> Vec<&Dim> {
        self.dims.iter().collect()
    }
}
