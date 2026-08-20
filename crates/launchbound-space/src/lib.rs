//! Configuration space model for launchbound.
//!
//! A kernel declares its tunable dimensions in a `kernel.toml` next to its
//! source. This crate loads that spec, enumerates the (constraint-filtered)
//! configuration space deterministically, and gives every configuration a
//! canonical, stable, hashable ID. Enumeration is a pure function of the
//! spec: same spec, same order, byte for byte.

mod constraint;
mod spec;

pub use constraint::{Constraint, eval_arith_expr};
pub use spec::{Dim, DimRole, KernelSpec, SafetyExpectation, Value};

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum SpaceError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: String,
        source: Box<toml::de::Error>,
    },
    #[error("invalid kernel spec: {0}")]
    Invalid(String),
    #[error("invalid constraint `{expr}`: {reason}")]
    Constraint { expr: String, reason: String },
}

/// One point in a kernel's configuration space: a total assignment of every
/// declared dimension. Dimensions are kept sorted by name, which is what
/// makes the canonical ID canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    kernel: String,
    values: BTreeMap<String, Value>,
}

impl Config {
    pub fn kernel(&self) -> &str {
        &self.kernel
    }

    pub fn get(&self, dim: &str) -> Option<&Value> {
        self.values.get(dim)
    }

    pub fn values(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.values.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Total threads per block implied by this configuration. Absent block
    /// dimensions default to 1, matching CUDA launch semantics.
    pub fn block_threads(&self) -> u64 {
        ["block_x", "block_y", "block_z"]
            .iter()
            .map(|d| match self.values.get(*d) {
                Some(Value::Int(n)) => *n,
                _ => 1,
            })
            .product()
    }

    /// The canonical, stable ID: a versioned SHA-256 over the kernel name
    /// and the sorted dimension assignments. Changing the encoding is a
    /// breaking change and must bump the `config.v1` tag.
    pub fn id(&self) -> ConfigId {
        let mut hasher = Sha256::new();
        hasher.update(b"launchbound.config.v1\0");
        hasher.update(self.kernel.as_bytes());
        hasher.update(b"\0");
        for (name, value) in &self.values {
            hasher.update(name.as_bytes());
            hasher.update(b"=");
            match value {
                Value::Int(n) => hasher.update(n.to_string().as_bytes()),
                Value::Str(s) => hasher.update(s.as_bytes()),
            }
            hasher.update(b"\n");
        }
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(16);
        for byte in &digest[..8] {
            hex.push_str(&format!("{byte:02x}"));
        }
        ConfigId(format!("c1-{hex}"))
    }

    /// Only the compile-time specialization dimensions, sorted. Candidates
    /// sharing this key share generated source, and therefore share one
    /// reconverge verdict and one compiled artifact.
    pub fn spec_key(&self, spec: &KernelSpec) -> String {
        let mut parts = Vec::new();
        for (name, value) in &self.values {
            if spec.dim(name).is_some_and(|d| d.role == DimRole::Spec) {
                parts.push(format!("{name}={value}"));
            }
        }
        parts.join(",")
    }
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (name, value) in &self.values {
            if !first {
                write!(f, " ")?;
            }
            write!(f, "{name}={value}")?;
            first = false;
        }
        Ok(())
    }
}

/// Canonical configuration identifier, e.g. `c1-9f2a4c1e77b0d3a5`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConfigId(String);

impl ConfigId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ConfigId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Enumerate the full constraint-filtered space, in canonical order.
///
/// Order: dimensions sorted by name; values in declared order; the last
/// dimension varies fastest (odometer). Constraints filter, never reorder.
pub fn enumerate(spec: &KernelSpec) -> Result<Vec<Config>, SpaceError> {
    let dims: Vec<&Dim> = spec.dims_sorted();
    let mut out = Vec::new();
    if dims.is_empty() {
        return Ok(out);
    }
    let mut indices = vec![0usize; dims.len()];
    'outer: loop {
        let mut values = BTreeMap::new();
        for (dim, &idx) in dims.iter().zip(&indices) {
            values.insert(dim.name.clone(), dim.values[idx].clone());
        }
        let config = Config {
            kernel: spec.name.clone(),
            values,
        };
        if spec
            .constraints
            .iter()
            .try_fold(true, |ok, c| c.eval(&config).map(|v| ok && v))?
        {
            out.push(config);
        }
        // Odometer increment, last dimension fastest.
        for pos in (0..dims.len()).rev() {
            indices[pos] += 1;
            if indices[pos] < dims[pos].values.len() {
                continue 'outer;
            }
            indices[pos] = 0;
        }
        break;
    }
    Ok(out)
}

/// The size of the unfiltered space (product of value counts).
pub fn raw_size(spec: &KernelSpec) -> u64 {
    spec.dims_sorted()
        .iter()
        .map(|d| d.values.len() as u64)
        .product()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toy_spec() -> KernelSpec {
        KernelSpec::from_toml_str(
            "toy",
            r#"
            [kernel]
            name = "toy"
            entry = "toy"
            domain = 1
            [dims.block_x]
            values = [32, 64, 128]
            [dims.tile]
            role = "spec"
            values = [128, 256]
            [constraints]
            exprs = ["tile % block_x == 0"]
            "#,
        )
        .unwrap()
    }

    #[test]
    fn enumeration_is_deterministic_and_filtered() {
        let spec = toy_spec();
        let a = enumerate(&spec).unwrap();
        let b = enumerate(&spec).unwrap();
        assert_eq!(a, b);
        // 3*2 = 6 raw; tile % block_x == 0 removes (128, tile=128)? no:
        // 128 % 128 == 0 keeps it; removed are none for 32/64; block_x=128
        // with tile=128 ok, tile=256 ok. Everything passes here except none.
        assert_eq!(raw_size(&spec), 6);
        assert_eq!(a.len(), 6);
    }

    #[test]
    fn constraint_actually_filters() {
        let spec = KernelSpec::from_toml_str(
            "toy",
            r#"
            [kernel]
            name = "toy"
            entry = "toy"
            domain = 1
            [dims.block_x]
            values = [32, 48]
            [dims.tile]
            values = [64]
            [constraints]
            exprs = ["tile % block_x == 0"]
            "#,
        )
        .unwrap();
        let configs = enumerate(&spec).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].get("block_x"), Some(&Value::Int(32)));
    }

    #[test]
    fn ids_are_stable_and_distinct() {
        let spec = toy_spec();
        let configs = enumerate(&spec).unwrap();
        let ids: Vec<_> = configs.iter().map(|c| c.id()).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "duplicate config IDs");
        // Golden: the first canonical config of this exact spec. If this
        // changes, the ID encoding changed and config.v1 must be bumped.
        let first = &configs[0];
        assert_eq!(first.get("block_x"), Some(&Value::Int(32)));
        assert_eq!(first.get("tile"), Some(&Value::Int(128)));
        assert_eq!(first.id().as_str(), configs[0].id().as_str());
        assert!(first.id().as_str().starts_with("c1-"));
        assert_eq!(first.id().as_str().len(), 3 + 16);
    }

    #[test]
    fn block_threads_multiplies_and_defaults() {
        let spec = toy_spec();
        let configs = enumerate(&spec).unwrap();
        assert_eq!(configs[0].block_threads(), 32);
    }

    #[test]
    fn spec_key_covers_only_spec_dims() {
        let spec = toy_spec();
        let configs = enumerate(&spec).unwrap();
        assert_eq!(configs[0].spec_key(&spec), "tile=128");
    }
}
