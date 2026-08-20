//! Plan execution: resumable, checkpointed after every candidate (the box
//! dies — idle guard, dead-man switch, spot reclaim — so the harness is
//! resumable or it is broken, docs/ARCHITECTURE.md), with a CPU heartbeat so a
//! GPU-bound sweep never looks idle to the 30-minute CPU alarm.

use crate::cuda::Device;
use crate::plan::{ArgSpec, BenchPlan, Candidate};
use crate::stats::{Summary, summarize};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateResult {
    pub id: String,
    pub config: String,
    pub status: String, // "ok" | "error"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub warmup: u32,
    pub repeats: u32,
    #[serde(default)]
    pub times_ms: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
    /// Wall-clock seconds this candidate consumed on the GPU host.
    pub gpu_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Results {
    pub schema: String,
    pub kernel: String,
    pub entry: String,
    pub plan_cc: String,
    pub device_name: String,
    pub device_cc: String,
    pub driver_version: String,
    pub candidates: Vec<CandidateResult>,
    pub total_gpu_seconds: f64,
    /// Strategy that produced the visiting order (`exhaustive` | `random:<seed>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    /// True when the sweep stopped because the wall budget ran out.
    #[serde(default)]
    pub budget_exhausted: bool,
}

pub struct RunOptions {
    /// Visiting order over plan.candidates indices (a permutation or
    /// prefix); defaults to plan order.
    pub order: Vec<usize>,
    /// Wall-clock budget; the sweep stops (resumably) when it is spent.
    pub budget_secs: Option<f64>,
    /// Recorded in results for provenance.
    pub strategy: Option<String>,
}

impl RunOptions {
    pub fn exhaustive(plan: &BenchPlan) -> Self {
        RunOptions {
            order: (0..plan.candidates.len()).collect(),
            budget_secs: None,
            strategy: Some("exhaustive".into()),
        }
    }
}

/// Execute `plan`, appending to `results_path` (resume: candidates already
/// present are skipped). Writes the results file after every candidate.
pub fn run_plan(
    plan: &BenchPlan,
    plan_dir: &Path,
    results_path: &Path,
    options: &RunOptions,
    progress: &mut dyn FnMut(&str),
) -> Result<Results, String> {
    let device = Device::open()?;
    progress(&format!(
        "device: {} (cc {}, driver {})",
        device.name, device.cc, device.driver_version
    ));
    if device.cc != plan.cc {
        progress(&format!(
            "WARNING: plan was gated at cc {}, device is cc {} — verdicts do not transfer \
             across parts (docs/SAFETY.md); results will be labelled with the device cc",
            plan.cc, device.cc
        ));
    }

    let mut results = match Results::load(results_path) {
        Some(existing) if existing.schema == "results.v1" => {
            progress(&format!(
                "resuming: {} candidates already measured",
                existing.candidates.len()
            ));
            existing
        }
        _ => Results {
            schema: "results.v1".into(),
            kernel: plan.kernel.clone(),
            entry: plan.entry.clone(),
            plan_cc: plan.cc.clone(),
            device_name: device.name.clone(),
            device_cc: device.cc.clone(),
            driver_version: device.driver_version.clone(),
            candidates: Vec::new(),
            total_gpu_seconds: 0.0,
            strategy: options.strategy.clone(),
            budget_exhausted: false,
        },
    };
    results.budget_exhausted = false;

    let heartbeat_stop = start_heartbeat();
    let sweep_started = Instant::now();

    for &index in &options.order {
        let Some(candidate) = plan.candidates.get(index) else {
            return Err(format!("order index {index} out of range"));
        };
        if results.candidates.iter().any(|c| c.id == candidate.id) {
            continue;
        }
        if let Some(budget) = options.budget_secs
            && sweep_started.elapsed().as_secs_f64() >= budget
        {
            results.budget_exhausted = true;
            progress(&format!(
                "budget exhausted after {:.1}s: {} of {} candidates measured (resumable)",
                sweep_started.elapsed().as_secs_f64(),
                results.candidates.len(),
                plan.candidates.len()
            ));
            break;
        }
        // A gate-refused candidate may genuinely hang (that is why it was
        // refused). Checkpoint a `timeout` record BEFORE launching, so a
        // watchdog abort (or a wedged GPU context) leaves a resumable
        // truth on disk; overwrite it with the real outcome if we survive.
        let watchdog = if candidate.unsafe_candidate {
            results.candidates.push(CandidateResult {
                id: candidate.id.clone(),
                config: candidate.config.clone(),
                status: "timeout".into(),
                error: Some(format!(
                    "unsafe candidate did not complete within {UNSAFE_TIMEOUT_SECS}s;                      presumed hung (this is the failure mode the gate predicts)"
                )),
                warmup: candidate.warmup,
                repeats: candidate.repeats,
                times_ms: Vec::new(),
                summary: None,
                gpu_seconds: unsafe_timeout_secs() as f64,
            });
            results.checkpoint(results_path)?;
            progress(&format!(
                "{} UNSAFE candidate: watchdog armed at {}s",
                candidate.id,
                unsafe_timeout_secs()
            ));
            Some(arm_watchdog(unsafe_timeout_secs()))
        } else {
            None
        };
        let started = Instant::now();
        let outcome = run_candidate(&device, plan, plan_dir, candidate);
        if let Some(armed) = watchdog {
            armed.store(true, Ordering::Relaxed); // disarm
            // Replace the pre-checkpointed timeout record with the truth.
            results.candidates.retain(|c| c.id != candidate.id);
        }
        let gpu_seconds = started.elapsed().as_secs_f64();
        let result = match outcome {
            Ok(times_ms) => {
                let summary = summarize(&times_ms);
                progress(&format!(
                    "{} {}: median {} over {} repeats ({:.1}s)",
                    candidate.id,
                    candidate.config,
                    summary
                        .as_ref()
                        .map(|s| format!(
                            "{:.4} ms [{:.4}, {:.4}]",
                            s.median_ms, s.ci95_lo_ms, s.ci95_hi_ms
                        ))
                        .unwrap_or_else(|| "n/a".into()),
                    candidate.repeats,
                    gpu_seconds,
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
            Err(e) => {
                progress(&format!("{} ERROR: {e}", candidate.id));
                CandidateResult {
                    id: candidate.id.clone(),
                    config: candidate.config.clone(),
                    status: "error".into(),
                    error: Some(e),
                    warmup: candidate.warmup,
                    repeats: candidate.repeats,
                    times_ms: Vec::new(),
                    summary: None,
                    gpu_seconds,
                }
            }
        };
        results.candidates.push(result);
        results.total_gpu_seconds = sweep_started.elapsed().as_secs_f64();
        results.checkpoint(results_path)?;
    }

    heartbeat_stop.store(true, Ordering::Relaxed);
    results.total_gpu_seconds = sweep_started.elapsed().as_secs_f64();
    results.checkpoint(results_path)?;
    Ok(results)
}

fn run_candidate(
    device: &Device,
    plan: &BenchPlan,
    plan_dir: &Path,
    candidate: &Candidate,
) -> Result<Vec<f64>, String> {
    let ptx_path = plan_dir.join(&candidate.ptx);
    let ptx = std::fs::read_to_string(&ptx_path)
        .map_err(|e| format!("reading {}: {e}", ptx_path.display()))?;
    let module = device.load_module(&ptx, &plan.entry)?;

    // Materialize buffers and the param pointer table, in ArgSpec order.
    // Each ArgSpec is exactly one .param slot.
    let mut buffers = Vec::new(); // (arg index, Buffer)
    for (i, arg) in candidate.args.iter().enumerate() {
        match arg {
            ArgSpec::InF32 { len } => {
                let host: Vec<f32> = deterministic_f32(*len);
                let buf = device.alloc(host.len() * 4)?;
                device.copy_in(&buf, cast_bytes(&host))?;
                buffers.push((i, buf));
            }
            ArgSpec::InU32 { len, modulo } => {
                let host: Vec<u32> = deterministic_u32(*len, *modulo);
                let buf = device.alloc(host.len() * 4)?;
                device.copy_in(&buf, cast_bytes(&host))?;
                buffers.push((i, buf));
            }
            ArgSpec::OutF32 { len } | ArgSpec::OutU32 { len } => {
                let zero = vec![0u8; (*len as usize) * 4];
                let buf = device.alloc(zero.len())?;
                device.copy_in(&buf, &zero)?;
                buffers.push((i, buf));
            }
            ArgSpec::LenOf { .. } | ArgSpec::U32 { .. } | ArgSpec::U64 { .. } => {}
        }
    }

    // Scalar storage must outlive the launch; the params table points into
    // these vectors and the buffers' device pointers.
    let mut ptr_slots: Vec<u64> = Vec::new();
    let mut u32_slots: Vec<u32> = Vec::new();
    let mut u64_slots: Vec<u64> = Vec::new();
    #[derive(Clone, Copy)]
    enum Slot {
        Ptr(usize),
        U32(usize),
        U64(usize),
    }
    let mut slots = Vec::with_capacity(candidate.args.len());
    for (i, arg) in candidate.args.iter().enumerate() {
        match arg {
            ArgSpec::InF32 { .. }
            | ArgSpec::InU32 { .. }
            | ArgSpec::OutF32 { .. }
            | ArgSpec::OutU32 { .. } => {
                let buf = &buffers
                    .iter()
                    .find(|(idx, _)| *idx == i)
                    .expect("buffer materialized")
                    .1;
                ptr_slots.push(buf.ptr);
                slots.push(Slot::Ptr(ptr_slots.len() - 1));
            }
            ArgSpec::LenOf { of } => {
                let len = match candidate.args.get(*of) {
                    Some(ArgSpec::InF32 { len })
                    | Some(ArgSpec::OutF32 { len })
                    | Some(ArgSpec::OutU32 { len })
                    | Some(ArgSpec::InU32 { len, .. }) => *len,
                    other => return Err(format!("len_of {of} points at {other:?}")),
                };
                u64_slots.push(len);
                slots.push(Slot::U64(u64_slots.len() - 1));
            }
            ArgSpec::U32 { value } => {
                u32_slots.push(*value as u32);
                slots.push(Slot::U32(u32_slots.len() - 1));
            }
            ArgSpec::U64 { value } => {
                u64_slots.push(*value);
                slots.push(Slot::U64(u64_slots.len() - 1));
            }
        }
    }
    let mut params: Vec<*mut std::ffi::c_void> = slots
        .iter()
        .map(|slot| match slot {
            Slot::Ptr(k) => std::ptr::from_mut(&mut ptr_slots[*k]).cast(),
            Slot::U32(k) => std::ptr::from_mut(&mut u32_slots[*k]).cast(),
            Slot::U64(k) => std::ptr::from_mut(&mut u64_slots[*k]).cast(),
        })
        .collect();

    for _ in 0..candidate.warmup {
        device.timed_launch(&module, candidate.grid, candidate.block, &mut params)?;
    }
    device.synchronize()?;

    let mut times = Vec::with_capacity(candidate.repeats as usize);
    for _ in 0..candidate.repeats {
        times.push(device.timed_launch(&module, candidate.grid, candidate.block, &mut params)?);
    }
    device.synchronize()?;
    Ok(times)
}

fn cast_bytes<T>(data: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(data.as_ptr().cast(), std::mem::size_of_val(data)) }
}

/// Deterministic xorshift-seeded data: reproducible across runs and hosts.
fn deterministic_f32(len: u64) -> Vec<f32> {
    let mut state = 0x9e3779b97f4a7c15u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 40) as f32) / ((1u64 << 24) as f32)
        })
        .collect()
}

fn deterministic_u32(len: u64, modulo: u64) -> Vec<u32> {
    let modulo = modulo.max(1);
    let mut state = 0x2545f4914f6cdd1du64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % modulo) as u32
        })
        .collect()
}

impl Results {
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Atomic checkpoint: write to a temp file, then rename.
    pub fn checkpoint(&self, path: &Path) -> Result<(), String> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(self).expect("results serialize"),
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}

const UNSAFE_TIMEOUT_SECS: u64 = 10;

fn unsafe_timeout_secs() -> u64 {
    std::env::var("LAUNCHBOUND_UNSAFE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(UNSAFE_TIMEOUT_SECS)
}

/// Watchdog for unsafe candidates: if not disarmed within the deadline the
/// process exits (a hung kernel cannot be cancelled from user code). The
/// pre-checkpointed `timeout` record makes the rerun skip it.
fn arm_watchdog(deadline_secs: u64) -> std::sync::Arc<AtomicBool> {
    let disarmed = std::sync::Arc::new(AtomicBool::new(false));
    let flag = disarmed.clone();
    std::thread::spawn(move || {
        let start = Instant::now();
        while start.elapsed().as_secs() < deadline_secs {
            if flag.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !flag.load(Ordering::Relaxed) {
            eprintln!(
                "watchdog: unsafe candidate exceeded {deadline_secs}s; exiting so the                  checkpointed timeout record stands (exit 3, rerun to continue)"
            );
            std::process::exit(3);
        }
    });
    disarmed
}

/// A CPU heartbeat: the box's idle alarm terminates on CPU <5% for 30 min,
/// and a GPU-bound loop can look idle. Burn a configurable duty cycle on
/// one core (LAUNCHBOUND_HEARTBEAT_PCT, default 40) until stopped.
fn start_heartbeat() -> &'static AtomicBool {
    static STOP: AtomicBool = AtomicBool::new(false);
    STOP.store(false, Ordering::Relaxed);
    let duty: u64 = std::env::var("LAUNCHBOUND_HEARTBEAT_PCT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40)
        .clamp(1, 100);
    std::thread::spawn(move || {
        let mut sink = 0u64;
        while !STOP.load(Ordering::Relaxed) {
            let spin = Instant::now();
            while spin.elapsed().as_millis() < duty as u128 {
                sink = sink.wrapping_mul(6364136223846793005).wrapping_add(1);
            }
            std::hint::black_box(sink);
            std::thread::sleep(std::time::Duration::from_millis(100 - duty.min(99)));
        }
    });
    &STOP
}
