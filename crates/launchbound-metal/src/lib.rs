//! Metal measurement on Apple Silicon (S7).
//!
//! **This path has no convergence gate.** reconverge analyzes cuda-oxide
//! kernels; there is no equivalent for MSL and this project does not build
//! one (docs/LIMITATIONS.md). Apple GPUs have 32-wide SIMD-groups and
//! simd-scoped collectives, so the same bug class exists and is simply not
//! checked. Every result this module produces carries `backend = "metal"`,
//! and the report renderer refuses to omit the notice.

use launchbound_bench::Results;
#[cfg(target_os = "macos")]
use launchbound_bench::{CandidateResult, summarize};
use launchbound_space::{Config, DimRole, KernelSpec, Value};

/// The notice. Tested verbatim; changing it is fine, omitting it is not.
pub const METAL_NO_GATE_NOTICE: &str =
    "NO convergence gate exists on the Metal path: the same bug class is NOT checked";

#[derive(Debug, thiserror::Error)]
pub enum MetalError {
    #[error("metal backend: {0}")]
    Backend(String),
    #[error("kernel.metal: {0}")]
    Source(String),
}

/// Rewrite `constant constexpr uint NAME = ...;` parameters in MSL source
/// for a candidate's spec dimensions — the params.rs pattern, for MSL.
pub fn specialize_msl(
    source: &str,
    spec: &KernelSpec,
    config: &Config,
) -> Result<String, MetalError> {
    let mut text = source.to_string();
    for (name, value) in config.values() {
        if spec.dim(name).map(|d| d.role) != Some(DimRole::Spec) {
            continue;
        }
        let Value::Int(n) = value else {
            return Err(MetalError::Source(format!(
                "dimension `{name}` is a string; unsupported on the Metal path"
            )));
        };
        let konst = name.to_uppercase();
        let needle = format!("constant constexpr uint {konst} = ");
        let Some(start) = text.find(&needle) else {
            // Not every CUDA-side dimension exists in the MSL twin
            // (lb_max, for example, has no Metal meaning); skip those.
            continue;
        };
        let value_start = start + needle.len();
        let end = text[value_start..]
            .find(';')
            .map(|i| value_start + i)
            .ok_or_else(|| MetalError::Source(format!("unterminated constant {konst}")))?;
        text.replace_range(value_start..end, &n.to_string());
    }
    Ok(text)
}

/// Measure every candidate of a kernel on the system-default Metal device.
/// Buffer/argument layout mirrors the [bench] args: buffers bind in arg
/// order; `len_of`/scalars pass as small constant buffers.
#[cfg(target_os = "macos")]
pub fn run_metal(
    spec: &KernelSpec,
    configs: &[(Config, launchbound_bench::Candidate)],
    budget_secs: Option<f64>,
    progress: &mut dyn FnMut(&str),
) -> Result<Results, MetalError> {
    imp::run_metal(spec, configs, budget_secs, progress)
}

#[cfg(not(target_os = "macos"))]
pub fn run_metal(
    _spec: &KernelSpec,
    _configs: &[(Config, launchbound_bench::Candidate)],
    _budget_secs: Option<f64>,
    _progress: &mut dyn FnMut(&str),
) -> Result<Results, MetalError> {
    Err(MetalError::Backend(
        "the Metal backend only exists on macOS".into(),
    ))
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use launchbound_bench::ArgSpec;
    use metal::{ComputePipelineDescriptor, Device, MTLResourceOptions, MTLSize};
    use std::time::Instant;

    pub fn run_metal(
        spec: &KernelSpec,
        configs: &[(Config, launchbound_bench::Candidate)],
        budget_secs: Option<f64>,
        progress: &mut dyn FnMut(&str),
    ) -> Result<Results, MetalError> {
        let source_path = spec.dir.join("kernel.metal");
        let source = std::fs::read_to_string(&source_path).map_err(|e| {
            MetalError::Source(format!(
                "{}: {e} — this kernel has no MSL twin, so it cannot tune on Metal",
                source_path.display()
            ))
        })?;
        let device = Device::system_default()
            .ok_or_else(|| MetalError::Backend("no Metal device".into()))?;
        let queue = device.new_command_queue();
        progress(&format!("metal device: {}", device.name()));
        progress(METAL_NO_GATE_NOTICE);

        let mut results = Results {
            schema: "results.v1".into(),
            kernel: spec.name.clone(),
            entry: spec.entry.clone(),
            plan_cc: "metal".into(),
            device_name: device.name().to_string(),
            device_cc: "metal".into(),
            driver_version: format!("macOS {}", macos_version()),
            candidates: Vec::new(),
            total_gpu_seconds: 0.0,
            strategy: Some("exhaustive".into()),
            budget_exhausted: false,
        };
        let sweep_started = Instant::now();

        for (config, candidate) in configs {
            if let Some(budget) = budget_secs
                && sweep_started.elapsed().as_secs_f64() >= budget
            {
                results.budget_exhausted = true;
                progress("budget exhausted (resumable ordering; rerun to continue)");
                break;
            }
            let started = Instant::now();
            let outcome = run_candidate(&device, &queue, &source, spec, config, candidate);
            let gpu_seconds = started.elapsed().as_secs_f64();
            let result = match outcome {
                Ok(times_ms) => {
                    let summary = summarize(&times_ms);
                    progress(&format!(
                        "{} {}: median {} (metal, {:.1}s)",
                        candidate.id,
                        candidate.config,
                        summary
                            .as_ref()
                            .map(|s| format!("{:.4} ms", s.median_ms))
                            .unwrap_or_else(|| "n/a".into()),
                        gpu_seconds
                    ));
                    CandidateResult {
                        id: candidate.id.clone(),
                        config: candidate.config.clone(),
                        status: "ok".into(),
                        error: None,
                        warmup: candidate.warmup,
                        repeats: candidate.repeats,
                        times_ms,
                        summary,
                        gpu_seconds,
                    }
                }
                Err(e) => CandidateResult {
                    id: candidate.id.clone(),
                    config: candidate.config.clone(),
                    status: "error".into(),
                    error: Some(e.to_string()),
                    warmup: candidate.warmup,
                    repeats: candidate.repeats,
                    times_ms: Vec::new(),
                    summary: None,
                    gpu_seconds,
                },
            };
            results.candidates.push(result);
        }
        results.total_gpu_seconds = sweep_started.elapsed().as_secs_f64();
        Ok(results)
    }

    fn run_candidate(
        device: &Device,
        queue: &metal::CommandQueue,
        source: &str,
        spec: &KernelSpec,
        config: &Config,
        candidate: &launchbound_bench::Candidate,
    ) -> Result<Vec<f64>, MetalError> {
        let specialized = specialize_msl(source, spec, config)?;
        let library = device
            .new_library_with_source(&specialized, &metal::CompileOptions::new())
            .map_err(|e| MetalError::Backend(format!("MSL compile: {e}")))?;
        let function = library
            .get_function(&spec.entry, None)
            .map_err(|e| MetalError::Backend(format!("entry {}: {e}", spec.entry)))?;
        let descriptor = ComputePipelineDescriptor::new();
        descriptor.set_compute_function(Some(&function));
        let pipeline = device
            .new_compute_pipeline_state(&descriptor)
            .map_err(|e| MetalError::Backend(format!("pipeline: {e}")))?;

        // Buffers in arg order; scalars recorded for set_bytes.
        enum Bind {
            Buffer(metal::Buffer),
            Scalar32(u32),
            Scalar64(u64),
        }
        let mut binds = Vec::new();
        for arg in &candidate.args {
            match arg {
                ArgSpec::InF32 { len } => {
                    let host: Vec<f32> = (0..*len).map(|i| (i % 977) as f32 * 0.001).collect();
                    binds.push(Bind::Buffer(device.new_buffer_with_data(
                        host.as_ptr().cast(),
                        (host.len() * 4) as u64,
                        MTLResourceOptions::StorageModeShared,
                    )));
                }
                ArgSpec::InU32 { len, modulo } => {
                    let host: Vec<u32> = (0..*len).map(|i| (i % modulo.max(&1)) as u32).collect();
                    binds.push(Bind::Buffer(device.new_buffer_with_data(
                        host.as_ptr().cast(),
                        (host.len() * 4) as u64,
                        MTLResourceOptions::StorageModeShared,
                    )));
                }
                ArgSpec::OutF32 { len } | ArgSpec::OutU32 { len } => {
                    binds.push(Bind::Buffer(device.new_buffer(
                        (*len * 4).max(4),
                        MTLResourceOptions::StorageModeShared,
                    )));
                }
                ArgSpec::LenOf { of } => {
                    let len = match candidate.args.get(*of) {
                        Some(ArgSpec::InF32 { len })
                        | Some(ArgSpec::OutF32 { len })
                        | Some(ArgSpec::OutU32 { len })
                        | Some(ArgSpec::InU32 { len, .. }) => *len,
                        other => {
                            return Err(MetalError::Backend(format!(
                                "len_of {of} points at {other:?}"
                            )));
                        }
                    };
                    binds.push(Bind::Scalar32(len as u32));
                }
                ArgSpec::U32 { value } => binds.push(Bind::Scalar32(*value as u32)),
                ArgSpec::U64 { value } => binds.push(Bind::Scalar64(*value)),
            }
        }

        let grid = MTLSize {
            width: candidate.grid[0] as u64,
            height: candidate.grid[1] as u64,
            depth: candidate.grid[2] as u64,
        };
        let block = MTLSize {
            width: candidate.block[0] as u64,
            height: candidate.block[1] as u64,
            depth: candidate.block[2] as u64,
        };

        let dispatch = || -> Result<f64, MetalError> {
            let cmd = queue.new_command_buffer();
            let encoder = cmd.new_compute_command_encoder();
            encoder.set_compute_pipeline_state(&pipeline);
            for (i, bind) in binds.iter().enumerate() {
                match bind {
                    Bind::Buffer(buf) => encoder.set_buffer(i as u64, Some(buf), 0),
                    Bind::Scalar32(v) => {
                        encoder.set_bytes(i as u64, 4, std::ptr::from_ref(v).cast())
                    }
                    Bind::Scalar64(v) => {
                        encoder.set_bytes(i as u64, 8, std::ptr::from_ref(v).cast())
                    }
                }
            }
            encoder.dispatch_thread_groups(grid, block);
            encoder.end_encoding();
            cmd.commit();
            cmd.wait_until_completed();
            let (start, end) = gpu_timestamps(cmd);
            Ok((end - start) * 1000.0)
        };

        for _ in 0..candidate.warmup {
            dispatch()?;
        }
        let mut times = Vec::with_capacity(candidate.repeats as usize);
        for _ in 0..candidate.repeats {
            times.push(dispatch()?);
        }
        Ok(times)
    }

    /// metal-rs 0.29 has no GPUStartTime/GPUEndTime bindings; go through
    /// objc directly. GPU-side timestamps in seconds — kernel-only time.
    /// objc 0.2's sel_impl! probes a `cargo-clippy` cfg that modern rustc
    /// flags as unexpected; that lint is the macro's, not ours.
    #[allow(unexpected_cfgs)]
    fn gpu_timestamps(cmd: &metal::CommandBufferRef) -> (f64, f64) {
        use objc::{msg_send, sel, sel_impl};
        let obj = cmd as *const metal::CommandBufferRef as *mut objc::runtime::Object;
        unsafe { (msg_send![obj, GPUStartTime], msg_send![obj, GPUEndTime]) }
    }

    fn macos_version() -> String {
        std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| "unknown".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launchbound_space::{KernelSpec, enumerate};

    #[test]
    fn msl_specialization_rewrites_spec_dims_only() {
        let spec = KernelSpec::from_toml_str(
            "t",
            r#"
            [kernel]
            name = "t"
            entry = "t"
            domain = 1
            [dims.block_x]
            values = [64]
            [dims.tile]
            role = "spec"
            values = [512]
            "#,
        )
        .unwrap();
        let config = enumerate(&spec).unwrap().remove(0);
        let src = "constant constexpr uint TILE = 128;\nkernel void t() {}\n";
        let out = specialize_msl(src, &spec, &config).unwrap();
        assert!(out.contains("constant constexpr uint TILE = 512;"));
    }
}
