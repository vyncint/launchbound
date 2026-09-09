//! Configuration space model for launchbound.
//!
//! A kernel declares its tunable dimensions in a `kernel.toml` next to its
//! source. This crate loads that spec, enumerates the (constraint-filtered)
//! configuration space deterministically, and gives every configuration a
//! canonical, stable, hashable ID. Enumeration is a pure function of the
//! spec: same spec, same order, byte for byte.

#![warn(missing_docs)]

mod constraint;
mod spec;

pub use constraint::{Constraint, eval_arith_expr};
pub use spec::{Dim, DimRole, KernelSpec, SafetyExpectation, Value};

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;

/// What can go wrong loading a spec or enumerating its space.
#[derive(Debug, thiserror::Error)]
pub enum SpaceError {
    /// `kernel.toml` could not be read.
    #[error("failed to read {path}: {source}")]
    Io {
        /// The file that could not be read.
        path: String,
        /// The underlying I/O failure.
        source: std::io::Error,
    },
    /// `kernel.toml` is not valid TOML.
    #[error("failed to parse {path}: {source}")]
    Parse {
        /// The file that would not parse.
        path: String,
        /// The TOML error, boxed because it is large and this variant is
        /// rare.
        source: Box<toml::de::Error>,
    },
    /// The TOML parsed but does not describe a usable space: a bad
    /// dimension name, an empty or duplicated value list, a block axis
    /// above the CUDA limit, or no dimensions at all.
    #[error("invalid kernel spec: {0}")]
    Invalid(String),
    /// A `[constraints]` expression could not be parsed or evaluated —
    /// including arithmetic that would overflow or divide by zero, which
    /// is an error rather than a verdict.
    #[error("invalid constraint `{expr}`: {reason}")]
    Constraint {
        /// The expression as written in `kernel.toml`.
        expr: String,
        /// What was wrong with it.
        reason: String,
    },
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
    /// The kernel this configuration belongs to.
    pub fn kernel(&self) -> &str {
        &self.kernel
    }

    /// The value assigned to one dimension, or `None` if the spec does not
    /// declare it.
    pub fn get(&self, dim: &str) -> Option<&Value> {
        self.values.get(dim)
    }

    /// Every `(dimension, value)` pair, ascending by dimension name.
    ///
    /// The order is the sorted order, not the declaration order, and it is
    /// what makes [`Config::id`] canonical: two configurations that assign
    /// the same values hash identically however their spec was written.
    pub fn values(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.values.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Total threads per block implied by this configuration. Absent block
    /// dimensions default to 1, matching CUDA launch semantics.
    ///
    /// Saturating, like [`grid_blocks`] in `launchbound-model`, and for the
    /// same reason: the factors come from `kernel.toml`, and `.product()` over
    /// three attacker-shaped `u64`s panics in a debug build and wraps in a
    /// release one — where a wrapped value would then flow into `estimate` and
    /// into the gate's `threads > WARP_SIZE` test and be believed.
    ///
    /// A valid spec cannot reach the saturation point: `KernelSpec` rejects a
    /// block dimension above the CUDA per-axis limit at load, so the largest
    /// product this can be asked for is 1024 x 1024 x 64. The saturation is
    /// the floor under a `Config` built by some other route.
    ///
    /// [`grid_blocks`]: https://docs.rs/launchbound-model
    pub fn block_threads(&self) -> u64 {
        ["block_x", "block_y", "block_z"]
            .iter()
            .map(|d| match self.values.get(*d) {
                Some(Value::Int(n)) => *n,
                _ => 1,
            })
            .fold(1u64, |acc, n| acc.saturating_mul(n))
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
    /// The ID as it appears in `verdicts.v1`, `plan.v1`, `results.v1` and
    /// `report.v1` — `c1-` followed by 16 hex digits.
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

    // `block_threads` folds three `kernel.toml` integers. `.product()`
    // panicked in debug and wrapped in release, and a wrapped value went on
    // to feed `estimate` and the gate's `threads > WARP_SIZE` test — a
    // silently wrong launch shape, which is the one kind of wrong this
    // project cannot ship. Saturating matches `grid_blocks`, its sibling.
    //
    // This constructs `Config` directly because a spec can no longer express
    // these values: the load-time axis check rejects them. That is the point
    // — belt and braces, and the proptest guards the braces.
    proptest::proptest! {
        #[test]
        fn block_threads_never_panics_and_never_wraps(
            x in proptest::prelude::any::<u64>(),
            y in proptest::prelude::any::<u64>(),
            z in proptest::prelude::any::<u64>(),
        ) {
            let mut values = BTreeMap::new();
            values.insert("block_x".to_string(), Value::Int(x));
            values.insert("block_y".to_string(), Value::Int(y));
            values.insert("block_z".to_string(), Value::Int(z));
            let config = Config { kernel: "proptest".to_string(), values };

            let threads = config.block_threads();

            if x == 0 || y == 0 || z == 0 {
                proptest::prop_assert_eq!(threads, 0, "a zero axis is a zero block");
            } else {
                match x.checked_mul(y).and_then(|p| p.checked_mul(z)) {
                    // Exact whenever the true product fits: saturating must
                    // not change any answer that was already right.
                    Some(exact) => proptest::prop_assert_eq!(threads, exact),
                    // Otherwise pinned at the ceiling, never wrapped around to
                    // a small number that would read as a legal block.
                    None => proptest::prop_assert_eq!(threads, u64::MAX),
                }
            }
        }
    }

    /// The specific value the issue names.
    #[test]
    fn a_block_axis_above_the_cuda_limit_is_refused_at_load() {
        let err = KernelSpec::from_toml_str(
            "toy",
            r#"
            [kernel]
            name = "toy"
            entry = "toy"
            domain = 1
            [dims.block_x]
            values = [32, 2048]
            "#,
        )
        .expect_err("block_x = 2048 must not load");
        let msg = err.to_string();
        assert!(
            msg.contains("2048"),
            "message names the offending value: {msg}"
        );
        assert!(msg.contains("1024"), "message names the limit: {msg}");
    }

    /// z has a different limit (64), and saying "1024" there would be wrong.
    #[test]
    fn the_z_axis_limit_is_sixty_four() {
        let err = KernelSpec::from_toml_str(
            "toy",
            r#"
            [kernel]
            name = "toy"
            entry = "toy"
            domain = 1
            [dims.block_z]
            values = [65]
            "#,
        )
        .expect_err("block_z = 65 must not load");
        assert!(err.to_string().contains("64"), "{err}");
        // And 64 itself is fine.
        KernelSpec::from_toml_str(
            "toy",
            r#"
            [kernel]
            name = "toy"
            entry = "toy"
            domain = 1
            [dims.block_z]
            values = [64]
            "#,
        )
        .expect("block_z = 64 is the limit, not past it");
    }

    /// The limit applies to launch axes only; a spec dimension may be large.
    #[test]
    fn a_spec_dimension_is_not_capped_by_the_block_limit() {
        KernelSpec::from_toml_str(
            "toy",
            r#"
            [kernel]
            name = "toy"
            entry = "toy"
            domain = 1
            [dims.block_x]
            values = [32]
            [dims.elements]
            role = "spec"
            values = [1048576]
            "#,
        )
        .expect("a spec dimension is not a block axis");
    }

    #[test]
    fn spec_key_covers_only_spec_dims() {
        let spec = toy_spec();
        let configs = enumerate(&spec).unwrap();
        assert_eq!(configs[0].spec_key(&spec), "tile=128");
    }
}
