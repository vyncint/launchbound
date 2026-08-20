//! Bench plans (`plan.v1`): everything the runner needs, self-contained —
//! candidate configs, PTX paths, launch geometry, and the argument layout
//! matching cuda-oxide's PTX parameter lowering (each slice becomes a
//! `ptr, len` pair of `.param` slots; scalars are single slots).
//!
//! A kernel's `[bench]` section in kernel.toml declares the workload;
//! sizes and grid shapes are arithmetic expressions over the kernel's
//! dimensions plus the `elements` variable.

use launchbound_space::{Config, KernelSpec, eval_arith_expr};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("kernel.toml [bench]: {0}")]
    Spec(String),
    #[error(transparent)]
    Space(#[from] launchbound_space::SpaceError),
    #[error("plan io: {0}")]
    Io(String),
}

/// One PTX kernel argument slot group, in PTX parameter order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArgSpec {
    /// Device buffer of f32, copied in (seeded deterministic init).
    /// One `.param` slot (the pointer).
    InF32 { len: u64 },
    /// Device buffer of u32, values `seed % modulo` (histogram bins etc.).
    InU32 { len: u64, modulo: u64 },
    /// Device buffer of f32, zero-filled output. One slot.
    OutF32 { len: u64 },
    /// Device buffer of u32, zero-filled output. One slot.
    OutU32 { len: u64 },
    /// The length of the buffer at `of` (0-based ArgSpec index), as u64.
    LenOf { of: usize },
    /// Scalar u32.
    U32 { value: u64 },
    /// Scalar u64.
    U64 { value: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub config: String,
    /// PTX file path, relative to the plan file.
    pub ptx: String,
    /// True for a gate-refused candidate measured under --allow-unsafe:
    /// the runner guards it with a watchdog, because the refusal means it
    /// may genuinely hang.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unsafe_candidate: bool,
    pub grid: [u32; 3],
    pub block: [u32; 3],
    pub args: Vec<ArgSpec>,
    pub warmup: u32,
    pub repeats: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchPlan {
    pub schema: String,
    pub kernel: String,
    pub entry: String,
    pub cc: String,
    /// Present iff the plan includes gate-refused candidates: the explicit,
    /// recorded reason the operator gave to --allow-unsafe (see the README). Never a
    /// default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_unsafe_reason: Option<String>,
    pub candidates: Vec<Candidate>,
}

/// The `[bench]` section of kernel.toml.
#[derive(Debug, Deserialize)]
struct RawBench {
    elements: u64,
    #[serde(default = "default_one")]
    grid_x: toml::Value,
    #[serde(default = "default_one")]
    grid_y: toml::Value,
    #[serde(default = "default_one")]
    grid_z: toml::Value,
    #[serde(default = "default_warmup")]
    warmup: u32,
    #[serde(default = "default_repeats")]
    repeats: u32,
    args: Vec<RawArg>,
}

fn default_one() -> toml::Value {
    toml::Value::Integer(1)
}
fn default_warmup() -> u32 {
    20
}
fn default_repeats() -> u32 {
    100
}

#[derive(Debug, Deserialize)]
struct RawArg {
    kind: String,
    #[serde(default)]
    len: Option<toml::Value>,
    #[serde(default)]
    of: Option<usize>,
    #[serde(default)]
    value: Option<toml::Value>,
    #[serde(default)]
    modulo: Option<toml::Value>,
}

#[derive(Debug)]
pub struct BenchSpec {
    raw: RawBench,
}

impl BenchSpec {
    /// Load the `[bench]` section from the kernel's kernel.toml.
    pub fn load(spec: &KernelSpec) -> Result<Self, PlanError> {
        let path = spec.dir.join("kernel.toml");
        let text =
            std::fs::read_to_string(&path).map_err(|e| PlanError::Io(format!("{path:?}: {e}")))?;
        let table: toml::Value =
            toml::from_str(&text).map_err(|e| PlanError::Spec(e.to_string()))?;
        let bench = table
            .get("bench")
            .ok_or_else(|| PlanError::Spec("kernel.toml has no [bench] section".into()))?;
        let raw: RawBench = bench
            .clone()
            .try_into()
            .map_err(|e| PlanError::Spec(format!("{e}")))?;
        Ok(BenchSpec { raw })
    }

    /// Render one candidate: evaluate every expression against the config.
    pub fn candidate(
        &self,
        _spec: &KernelSpec,
        config: &Config,
        ptx_relative: &str,
    ) -> Result<Candidate, PlanError> {
        let mut extra = BTreeMap::new();
        extra.insert("elements".to_string(), self.raw.elements);
        let eval = |v: &toml::Value, what: &str| -> Result<u64, PlanError> {
            match v {
                toml::Value::Integer(n) if *n >= 0 => Ok(*n as u64),
                toml::Value::String(expr) => Ok(eval_arith_expr(expr, config, &extra)?),
                other => Err(PlanError::Spec(format!(
                    "{what} must be a non-negative integer or expression string, got {other}"
                ))),
            }
        };

        let grid = [
            eval(&self.raw.grid_x, "grid_x")? as u32,
            eval(&self.raw.grid_y, "grid_y")? as u32,
            eval(&self.raw.grid_z, "grid_z")? as u32,
        ];
        let block_dim = |name: &str| -> u32 {
            match config.get(name) {
                Some(launchbound_space::Value::Int(n)) => *n as u32,
                _ => 1,
            }
        };
        let block = [
            block_dim("block_x"),
            block_dim("block_y"),
            block_dim("block_z"),
        ];

        let mut args = Vec::with_capacity(self.raw.args.len());
        for (i, raw) in self.raw.args.iter().enumerate() {
            let need = |v: &Option<toml::Value>, field: &str| -> Result<u64, PlanError> {
                let v = v.as_ref().ok_or_else(|| {
                    PlanError::Spec(format!("args[{i}] kind {} needs `{field}`", raw.kind))
                })?;
                eval(v, field)
            };
            let arg = match raw.kind.as_str() {
                "in_f32" => ArgSpec::InF32 {
                    len: need(&raw.len, "len")?,
                },
                "in_u32" => ArgSpec::InU32 {
                    len: need(&raw.len, "len")?,
                    modulo: need(&raw.modulo, "modulo")?,
                },
                "out_f32" => ArgSpec::OutF32 {
                    len: need(&raw.len, "len")?,
                },
                "out_u32" => ArgSpec::OutU32 {
                    len: need(&raw.len, "len")?,
                },
                "len_of" => ArgSpec::LenOf {
                    of: raw
                        .of
                        .ok_or_else(|| PlanError::Spec(format!("args[{i}] len_of needs `of`")))?,
                },
                "u32" => ArgSpec::U32 {
                    value: need(&raw.value, "value")?,
                },
                "u64" => ArgSpec::U64 {
                    value: need(&raw.value, "value")?,
                },
                other => {
                    return Err(PlanError::Spec(format!(
                        "args[{i}]: unknown kind {other:?}"
                    )));
                }
            };
            args.push(arg);
        }

        Ok(Candidate {
            id: config.id().as_str().to_string(),
            config: config.to_string(),
            ptx: ptx_relative.to_string(),
            unsafe_candidate: false,
            grid,
            block,
            args,
            warmup: self.raw.warmup,
            repeats: self.raw.repeats,
        })
    }
}

impl BenchPlan {
    pub fn write(&self, path: &Path) -> Result<(), PlanError> {
        let json = serde_json::to_string_pretty(self).expect("plan serializes");
        std::fs::write(path, json).map_err(|e| PlanError::Io(format!("{path:?}: {e}")))
    }

    pub fn load(path: &Path) -> Result<Self, PlanError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| PlanError::Io(format!("{path:?}: {e}")))?;
        let plan: BenchPlan =
            serde_json::from_str(&text).map_err(|e| PlanError::Io(e.to_string()))?;
        if plan.schema != "plan.v1" {
            return Err(PlanError::Spec(format!(
                "unsupported plan schema {:?}",
                plan.schema
            )));
        }
        Ok(plan)
    }

    /// Number of `.param` slots this candidate's args expand to (each
    /// ArgSpec is exactly one slot; slices appear as explicit ptr + len_of
    /// pairs). Used to validate against the PTX entry signature.
    pub fn param_slots(candidate: &Candidate) -> usize {
        candidate.args.len()
    }
}
