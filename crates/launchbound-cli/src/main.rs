//! The launchbound binary. Exit codes (the README): 0 success, 1 the
//! notable-but-not-error outcome (tune only), 2 tool error.

use anyhow::Context;
use clap::{Parser, Subcommand};
use launchbound_space::{KernelSpec, SafetyExpectation, enumerate, raw_size};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "launchbound", version, about = "A convergence-safe autotuner for Rust GPU kernels", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Enumerate a kernel's configuration space and print its size.
    Space {
        /// Path to a kernel directory (containing kernel.toml), or a corpus
        /// kernel name resolved under --corpus. Omit to list every corpus
        /// kernel.
        kernel: Option<String>,
        /// Corpus root used to resolve kernel names.
        #[arg(long, default_value = "corpus")]
        corpus: PathBuf,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Print every configuration, not just the size.
        #[arg(long)]
        list: bool,
    },
    /// Run the safety gate over a kernel's space — reconverge only, NO GPU.
    Prune {
        /// Path to a kernel directory (containing kernel.toml), or a corpus
        /// kernel name resolved under --corpus. Omit to prune the whole
        /// corpus.
        kernel: Option<String>,
        /// Corpus root used to resolve kernel names.
        #[arg(long, default_value = "corpus")]
        corpus: PathBuf,
        /// Target compute capability for RC004 shared-memory context
        /// (docs/SAFETY.md): 8.6 for A10G, 7.5 for T4. A verdict at one
        /// --cc does not transfer to another.
        #[arg(long)]
        cc: String,
        /// Directory containing the cargo-reconverge binary (else
        /// LAUNCHBOUND_RECONVERGE or PATH).
        #[arg(long)]
        reconverge_dir: Option<PathBuf>,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Plumbing for `tune` (S5): prune, compile every admitted
    /// specialization (tier 1, no GPU), and emit a bench plan directory to
    /// ship to the measurement box.
    Stage {
        /// Path to a kernel directory, or a corpus kernel name.
        kernel: String,
        /// Corpus root used to resolve kernel names.
        #[arg(long, default_value = "corpus")]
        corpus: PathBuf,
        /// Target compute capability (gate context and provenance).
        #[arg(long)]
        cc: String,
        /// Output directory for plan.json + PTX artifacts.
        #[arg(long)]
        out: PathBuf,
        /// Directory containing the cargo-reconverge binary.
        #[arg(long)]
        reconverge_dir: Option<PathBuf>,
        /// Also stage gate-REFUSED candidates for measurement. Requires
        /// --reason; the reason is recorded in the plan and every report.
        /// Never the default (docs/SAFETY.md).
        #[arg(long)]
        allow_unsafe: bool,
        /// The explicit reason for --allow-unsafe, recorded verbatim.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Render the report for a staged (and possibly measured) run
    /// directory. Exit 0: a safe configuration was found. Exit 1: the
    /// fastest candidates were refused and the chosen one is slower than a
    /// rejected candidate — notable, not an error. Exit 2: tool error.
    Report {
        /// The run directory (from `stage`, plus the runner's results.json).
        run: PathBuf,
        /// Emit the report.v1 JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Print only the rejection section.
        #[arg(long)]
        rejected: bool,
    },
    /// Rank a kernel's space with the analytical model — NO GPU, output
    /// labelled `estimated` everywhere. With --results, print the model's
    /// Spearman rank correlation against those measurements instead.
    Model {
        /// Path to a kernel directory, or a corpus kernel name.
        kernel: String,
        /// Corpus root used to resolve kernel names.
        #[arg(long, default_value = "corpus")]
        corpus: PathBuf,
        /// Target compute capability (device table lookup).
        #[arg(long)]
        cc: String,
        /// A results.v1 file to correlate the model's ranking against.
        #[arg(long)]
        results: Option<PathBuf>,
    },
    /// Tune a kernel: search its space for the fastest convergence-safe
    /// configuration. Backends: cuda (gate + real silicon), metal (real
    /// silicon, NO GATE — stated on every surface), model (gate + estimate,
    /// no GPU anywhere).
    Tune {
        /// Path to a kernel directory, or a corpus kernel name.
        kernel: String,
        /// Corpus root used to resolve kernel names.
        #[arg(long, default_value = "corpus")]
        corpus: PathBuf,
        #[arg(long, value_parser = ["cuda", "metal", "model"])]
        backend: String,
        /// Target compute capability (cuda/model backends).
        #[arg(long, default_value = "8.6")]
        cc: String,
        /// Wall-clock budget, e.g. 30m, 90s, 1h. Honoured, resumably.
        #[arg(long)]
        budget: Option<String>,
        /// Run directory (default `runs/<kernel>-<backend>`).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Search order for measured backends.
        #[arg(long, default_value = "exhaustive", value_parser = ["exhaustive", "random"])]
        order: String,
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// Directory containing the cargo-reconverge binary.
        #[arg(long)]
        reconverge_dir: Option<PathBuf>,
    },
    /// Emit the chosen configuration as a cuda-oxide compile-time
    /// specialization: the params.rs to paste, the launch geometry, and
    /// provenance. Verifies the emitted source through the safety gate.
    Apply {
        /// The run directory (stage + runner outputs).
        run: PathBuf,
        /// The kernel directory the run tuned (for the params.rs template).
        #[arg(long)]
        kernel: String,
        /// Corpus root used to resolve kernel names.
        #[arg(long, default_value = "corpus")]
        corpus: PathBuf,
        /// Re-verify the emitted specialization with cargo reconverge.
        #[arg(long, default_value_t = true)]
        verify: bool,
        /// Directory containing the cargo-reconverge binary.
        #[arg(long)]
        reconverge_dir: Option<PathBuf>,
    },
}

fn parse_budget(text: &str) -> anyhow::Result<f64> {
    let text = text.trim();
    let (digits, unit) = text.split_at(text.len() - 1);
    match unit {
        "s" => Ok(digits.parse::<f64>()?),
        "m" => Ok(digits.parse::<f64>()? * 60.0),
        "h" => Ok(digits.parse::<f64>()? * 3600.0),
        _ => Ok(text.parse::<f64>()?),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Space {
            kernel,
            corpus,
            json,
            list,
        } => cmd_space(kernel.as_deref(), &corpus, json, list),
        Command::Prune {
            kernel,
            corpus,
            cc,
            reconverge_dir,
            json,
        } => cmd_prune(kernel.as_deref(), &corpus, &cc, reconverge_dir, json),
        Command::Stage {
            kernel,
            corpus,
            cc,
            out,
            reconverge_dir,
            allow_unsafe,
            reason,
        } => cmd_stage(
            &kernel,
            &corpus,
            &cc,
            &out,
            reconverge_dir,
            allow_unsafe,
            reason,
        ),
        Command::Report {
            run,
            json,
            rejected,
        } => cmd_report(&run, json, rejected),
        Command::Model {
            kernel,
            corpus,
            cc,
            results,
        } => cmd_model(&kernel, &corpus, &cc, results),
        Command::Apply {
            run,
            kernel,
            corpus,
            verify,
            reconverge_dir,
        } => cmd_apply(&run, &kernel, &corpus, verify, reconverge_dir),
        Command::Tune {
            kernel,
            corpus,
            backend,
            cc,
            budget,
            out,
            order,
            seed,
            reconverge_dir,
        } => cmd_tune(
            &kernel,
            &corpus,
            &backend,
            &cc,
            budget.as_deref(),
            out,
            &order,
            seed,
            reconverge_dir,
        ),
    }
}

fn cmd_apply(
    run_dir: &Path,
    kernel: &str,
    corpus: &Path,
    verify: bool,
    reconverge_dir: Option<PathBuf>,
) -> anyhow::Result<ExitCode> {
    use launchbound_report::{RunDir, build_report};

    let run = RunDir::load(run_dir)?;
    let report = build_report(&run)?;
    let chosen = report
        .chosen
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no chosen configuration: nothing measured yet"))?;

    let dir = resolve_kernel_dirs(Some(kernel), corpus)?.remove(0);
    let spec = KernelSpec::load(&dir)?;
    let config = enumerate(&spec)?
        .into_iter()
        .find(|c| c.id().as_str() == chosen.id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "chosen id {} is not in {}'s current space — the spec changed since the run",
                chosen.id,
                spec.name
            )
        })?;

    // Render the winning params.rs through the same specializer that built
    // the measured artifact: what you paste is what was measured.
    let scratch_root = launchbound_build::scratch::default_scratch_root(&spec);
    let scratch = launchbound_build::scratch::prepare_scratch(&spec, &scratch_root)?;
    launchbound_build::scratch::write_params(&spec, &config, &scratch)?;
    let params = std::fs::read_to_string(scratch.join("src/params.rs"))?;

    let block: Vec<String> = ["block_x", "block_y", "block_z"]
        .iter()
        .filter_map(|d| config.get(d).map(|v| format!("{d} = {v}")))
        .collect();
    let s = &chosen.summary;
    println!(
        "// ==== launchbound apply: {} / {} ====",
        spec.name, chosen.id
    );
    println!(
        "// measured: {:.4} ms [{:.4}, {:.4}] on {} (gate cc {}, {})",
        s.median_ms,
        s.ci95_lo_ms,
        s.ci95_hi_ms,
        report
            .device
            .as_ref()
            .map(|d| d.name.as_str())
            .unwrap_or("?"),
        report.gate_cc,
        report.measurement_kind
    );
    println!(
        "// launch: {}  (grid per your workload; the contract declares domain {})",
        block.join(", "),
        spec.domain
    );
    println!("// This result is valid only for the part above; it does not port across parts.");
    println!("// ---- src/params.rs ----");
    print!("{params}");

    if verify {
        eprintln!("verifying the emitted specialization with cargo reconverge --strict ...");
        let verdicts = launchbound_prune::prune_kernel(
            &spec,
            &launchbound_prune::PruneOptions {
                cc: report.gate_cc.clone(),
                reconverge_dir,
                scratch_root: None,
            },
        )?;
        let cv = verdicts
            .iter()
            .find(|cv| cv.config.id().as_str() == chosen.id)
            .ok_or_else(|| anyhow::anyhow!("chosen config missing from prune output"))?;
        match &cv.verdict {
            launchbound_prune::Verdict::Clean => {
                eprintln!("verified: clean under the gate at cc {}", report.gate_cc)
            }
            launchbound_prune::Verdict::AdmittedWithCaveats { .. } => {
                eprintln!("verified: admitted with caveats (see the report)")
            }
            other => anyhow::bail!(
                "the chosen configuration no longer passes the gate: {other:?} — refusing to emit"
            ),
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[allow(clippy::too_many_arguments)]
fn cmd_tune(
    kernel: &str,
    corpus: &Path,
    backend: &str,
    cc: &str,
    budget: Option<&str>,
    out: Option<PathBuf>,
    order: &str,
    seed: u64,
    reconverge_dir: Option<PathBuf>,
) -> anyhow::Result<ExitCode> {
    let budget_secs = budget.map(parse_budget).transpose()?;
    let dir = resolve_kernel_dirs(Some(kernel), corpus)?.remove(0);
    let spec = KernelSpec::load(&dir)?;
    let out = out.unwrap_or_else(|| PathBuf::from("runs").join(format!("{}-{backend}", spec.name)));
    std::fs::create_dir_all(&out)?;

    match backend {
        "metal" => {
            // NO GATE on this path (docs/SAFETY.md §3.4): candidates are
            // `ungated`, and the report renderer prints the notice
            // unconditionally.
            use launchbound_bench::BenchSpec;
            let bench = BenchSpec::load(&spec)?;
            let configs = enumerate(&spec)?;
            let pairs: Vec<_> = configs
                .iter()
                .map(|c| bench.candidate(&spec, c, "").map(|cand| (c.clone(), cand)))
                .collect::<Result<_, _>>()?;
            let verdicts: Vec<serde_json::Value> = configs
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "id": c.id().as_str(), "config": c.to_string(),
                        "verdict": "ungated", "block_threads": c.block_threads(),
                    })
                })
                .collect();
            std::fs::write(
                out.join("verdicts.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "schema": "verdicts.v1", "kernel": spec.name, "cc": "metal",
                    "gate": "none", "candidates": verdicts,
                }))?,
            )?;
            let mut progress = |line: &str| println!("{line}");
            let results = launchbound_metal::run_metal(&spec, &pairs, budget_secs, &mut progress)?;
            results
                .checkpoint(&out.join("results.json"))
                .map_err(anyhow::Error::msg)?;
            cmd_report(&out, false, false)
        }
        "model" => {
            use launchbound_model::{device, estimate};
            use launchbound_prune::{PruneOptions, Verdict, prune_kernel};
            let verdicts = prune_kernel(
                &spec,
                &PruneOptions {
                    cc: cc.into(),
                    reconverge_dir,
                    scratch_root: None,
                },
            )?;
            let dev = device(cc)?;
            println!(
                "{} — ESTIMATED tuning (analytical model, cc {cc}); the gate is full,                  the timings are NOT measurements:",
                spec.name
            );
            let mut admitted: Vec<_> = verdicts
                .iter()
                .filter(|cv| {
                    matches!(
                        cv.verdict,
                        Verdict::Clean | Verdict::AdmittedWithCaveats { .. }
                    )
                })
                .map(|cv| estimate(&spec, &cv.config, &dev).map(|e| (cv, e)))
                .collect::<Result<_, _>>()?;
            admitted.sort_by(|a, b| a.1.cost.partial_cmp(&b.1.cost).expect("no NaN"));
            for (cv, est) in admitted.iter().take(10) {
                println!(
                    "  estimated {}  {}  cost {:.3}",
                    cv.config.id(),
                    cv.config,
                    est.cost
                );
            }
            let refused = verdicts.len() - admitted.len();
            println!(
                "  ({} candidates refused by the gate; run `launchbound prune` for details)",
                refused
            );
            Ok(ExitCode::SUCCESS)
        }
        "cuda" => {
            let code = cmd_stage(kernel, corpus, cc, &out, reconverge_dir, false, None)?;
            if code != ExitCode::SUCCESS {
                return Ok(code);
            }
            let plan = launchbound_bench::BenchPlan::load(&out.join("plan.json"))?;
            let strategy = launchbound_search::Strategy::parse(order, seed)
                .ok_or_else(|| anyhow::anyhow!("unknown order {order}"))?;
            let options = launchbound_bench::run::RunOptions {
                order: strategy.order(&plan),
                budget_secs,
                strategy: Some(order.to_string()),
            };
            let mut progress = |line: &str| println!("{line}");
            match launchbound_bench::run_plan(
                &plan,
                &out,
                &out.join("results.json"),
                &options,
                &mut progress,
            ) {
                Ok(_) => cmd_report(&out, false, false),
                Err(e) if e.contains("libcuda not found") => {
                    println!(
                        "staged {} candidates at {} — no CUDA driver here; on the box:\n                           launchbound-runner {}/plan.json",
                        plan.candidates.len(),
                        out.display(),
                        out.display()
                    );
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => Err(anyhow::anyhow!(e)),
            }
        }
        other => anyhow::bail!("unknown backend {other}"),
    }
}

/// The measured rank correlation attached to every estimate (docs/LIMITATIONS.md). The
/// calibration file lives next to the corpus; a kernel absent from it is
/// uncalibrated and this says so.
fn calibration_line(corpus: &Path, kernel: &str) -> String {
    let path = corpus
        .parent()
        .unwrap_or(Path::new("."))
        .join("model-calibration.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return "model quality: UNCALIBRATED — no model-calibration.toml".into();
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return "model quality: UNCALIBRATED — calibration file unreadable".into();
    };
    let device = table
        .get("meta")
        .and_then(|m| m.get("device"))
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    match table
        .get("kernels")
        .and_then(|k| k.get(kernel))
        .and_then(|v| v.as_array())
    {
        Some(pair) if pair.len() == 2 => format!(
            "model quality for this kernel: Spearman rho = {} (n = {}, measured on {})",
            pair[0], pair[1], device
        ),
        _ => format!(
            "model quality: UNCALIBRATED for this kernel (no measured rank correlation on {device})"
        ),
    }
}

fn cmd_model(
    kernel: &str,
    corpus: &Path,
    cc: &str,
    results: Option<PathBuf>,
) -> anyhow::Result<ExitCode> {
    use launchbound_model::{device, estimate, spearman};

    let dir = resolve_kernel_dirs(Some(kernel), corpus)?.remove(0);
    let spec = KernelSpec::load(&dir)?;
    let dev = device(cc)?;
    let configs = enumerate(&spec)?;
    let mut estimates = Vec::new();
    for config in &configs {
        estimates.push(estimate(&spec, config, &dev)?);
    }

    if let Some(results_path) = results {
        let measured = launchbound_bench::Results::load(&results_path)
            .ok_or_else(|| anyhow::anyhow!("cannot read results.v1 at {results_path:?}"))?;
        let mut xs = Vec::new(); // model cost
        let mut ys = Vec::new(); // measured median
        for est in &estimates {
            if let Some(m) = measured
                .candidates
                .iter()
                .find(|c| c.id == est.id && c.status == "ok")
                && let Some(summary) = &m.summary
                && est.cost.is_finite()
            {
                xs.push(est.cost);
                ys.push(summary.median_ms);
            }
        }
        match spearman(&xs, &ys) {
            Some(rho) => println!(
                "{}: Spearman rank correlation (model vs measured, n={}): {rho:.3}",
                spec.name,
                xs.len()
            ),
            None => println!(
                "{}: not enough overlapping measurements to correlate (n={})",
                spec.name,
                xs.len()
            ),
        }
        return Ok(ExitCode::SUCCESS);
    }

    let mut ranked: Vec<_> = estimates.iter().collect();
    ranked.sort_by(|a, b| a.cost.partial_cmp(&b.cost).expect("no NaN costs"));
    println!(
        "{} — ESTIMATED ranking (analytical model, cc {cc}; not a measurement):",
        spec.name
    );
    println!("  {}", calibration_line(corpus, &spec.name));
    for est in ranked {
        println!(
            "  estimated {}  {}  cost {:.3} (occupancy {:.2}, waves {:.1}, smem {} B)",
            est.id, est.config, est.cost, est.occupancy, est.waves, est.smem_bytes
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_report(run_dir: &Path, json: bool, rejected_only: bool) -> anyhow::Result<ExitCode> {
    use launchbound_report::{RunDir, build_report, render_text};

    let run = RunDir::load(run_dir)?;
    let report = build_report(&run)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if rejected_only {
        let text = render_text(&report);
        let start = text.find("REFUSED BUT FASTER").unwrap_or(0);
        let section: String = text[start..]
            .lines()
            .take_while(|l| !l.starts_with("ALL CANDIDATES"))
            .collect::<Vec<_>>()
            .join("\n");
        if report.rejected_faster.is_empty() {
            println!("no refused configuration measured faster than the chosen one");
        } else {
            println!("{section}");
        }
    } else {
        print!("{}", render_text(&report));
    }

    // Exit-code contract: exit 1 when the chosen configuration is slower than a rejected
    // candidate — notable, not an error.
    Ok(if !report.rejected_faster.is_empty() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

#[allow(clippy::too_many_arguments)]
fn cmd_stage(
    kernel: &str,
    corpus: &Path,
    cc: &str,
    out: &Path,
    reconverge_dir: Option<PathBuf>,
    allow_unsafe: bool,
    reason: Option<String>,
) -> anyhow::Result<ExitCode> {
    use launchbound_bench::{BenchPlan, BenchSpec};
    use launchbound_build::{ArtifactCache, CacheOutcome, Compiler, Executor, entry_param_count};
    use launchbound_prune::{PruneOptions, Verdict, prune_kernel};

    // --allow-unsafe without an explicit reason string is a usage error,
    // not a warning (docs/SAFETY.md).
    anyhow::ensure!(
        !allow_unsafe || reason.as_deref().is_some_and(|r| !r.trim().is_empty()),
        "--allow-unsafe requires --reason with a non-empty explanation; it is recorded in the report"
    );
    anyhow::ensure!(
        reason.is_none() || allow_unsafe,
        "--reason only makes sense with --allow-unsafe"
    );

    let dir = resolve_kernel_dirs(Some(kernel), corpus)?.remove(0);
    let spec = KernelSpec::load(&dir)?;
    let bench = BenchSpec::load(&spec)?;

    println!("prune (cc {cc}) ...");
    let verdicts = prune_kernel(
        &spec,
        &PruneOptions {
            cc: cc.into(),
            reconverge_dir,
            scratch_root: None,
        },
    )?;
    let admitted: Vec<_> = verdicts
        .iter()
        .filter(|cv| {
            matches!(
                cv.verdict,
                Verdict::Clean | Verdict::AdmittedWithCaveats { .. }
            )
        })
        .collect();
    let refused_candidates: Vec<_> = verdicts
        .iter()
        .filter(|cv| matches!(cv.verdict, Verdict::Disqualified { .. }))
        .collect();
    let refused = refused_candidates.len();
    anyhow::ensure!(
        !verdicts
            .iter()
            .any(|cv| matches!(cv.verdict, Verdict::ToolError { .. })),
        "prune hit a tool error; staging refused"
    );
    println!(
        "  {} admitted, {refused} refused; compiling admitted specializations ...",
        admitted.len()
    );

    std::fs::create_dir_all(out)?;

    // The full gate record travels with the run: the report is built from
    // it, and the rejection section is the product.
    let verdicts_json: Vec<serde_json::Value> = verdicts
        .iter()
        .map(|cv| {
            let mut v = serde_json::to_value(&cv.verdict).expect("verdict serializes");
            v["id"] = cv.config.id().as_str().into();
            v["config"] = cv.config.to_string().into();
            v["block_threads"] = cv.config.block_threads().into();
            v
        })
        .collect();
    std::fs::write(
        out.join("verdicts.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "verdicts.v1",
            "kernel": spec.name,
            "cc": cc,
            "candidates": verdicts_json,
        }))?,
    )?;

    let scratch_root = launchbound_build::scratch::default_scratch_root(&spec);
    let mut compiler = Compiler::new(
        Executor::detect(),
        ArtifactCache::under(&spec.dir.join("target")),
    );

    let mut to_stage: Vec<(&launchbound_prune::CandidateVerdict, bool)> =
        admitted.iter().map(|cv| (*cv, false)).collect();
    if allow_unsafe {
        println!(
            "  --allow-unsafe: also staging {refused} REFUSED candidates under a runner watchdog"
        );
        to_stage.extend(refused_candidates.iter().map(|cv| (*cv, true)));
    }

    let mut candidates = Vec::new();
    let mut hits = 0u32;
    for (cv, unsafe_candidate) in &to_stage {
        let artifact = compiler.compile(&spec, &cv.config, &scratch_root)?;
        if artifact.outcome == CacheOutcome::Hit {
            hits += 1;
        }
        let ptx_name = format!("{}.ptx", artifact.source_hash);
        let dest = out.join(&ptx_name);
        if !dest.exists() {
            std::fs::copy(&artifact.ptx_path, &dest)?;
        }
        // Validate the plan's argument layout against the real PTX ABI.
        let ptx = std::fs::read_to_string(&dest)?;
        let mut candidate = bench.candidate(&spec, &cv.config, &ptx_name)?;
        candidate.unsafe_candidate = *unsafe_candidate;
        if let Some(count) = entry_param_count(&ptx, &spec.entry) {
            anyhow::ensure!(
                count == candidate.args.len(),
                "PTX entry `{}` has {count} params but [bench] declares {} — the arg layout \
                 in kernel.toml has drifted from the ABI",
                spec.entry,
                candidate.args.len()
            );
        } else {
            anyhow::bail!("PTX has no entry `{}`", spec.entry);
        }
        candidates.push(candidate);
    }
    println!(
        "  compiled {} specializations ({hits} cache hits, {} compiles)",
        to_stage.len(),
        compiler.compiles
    );

    let plan = BenchPlan {
        schema: "plan.v1".into(),
        kernel: spec.name.clone(),
        entry: spec.entry.clone(),
        cc: cc.into(),
        allow_unsafe_reason: reason,
        candidates,
    };
    let plan_path = out.join("plan.json");
    plan.write(&plan_path)?;
    println!(
        "staged {} candidates -> {} (ship this directory to the box and run launchbound-runner)",
        plan.candidates.len(),
        plan_path.display()
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_prune(
    kernel: Option<&str>,
    corpus: &Path,
    cc: &str,
    reconverge_dir: Option<PathBuf>,
    json: bool,
) -> anyhow::Result<ExitCode> {
    use launchbound_prune::{PruneOptions, Verdict, prune_kernel};

    let dirs = resolve_kernel_dirs(kernel, corpus)?;
    let options = PruneOptions {
        cc: cc.to_string(),
        reconverge_dir,
        scratch_root: None,
    };

    let mut tool_error = false;
    let mut json_out = Vec::new();
    for dir in &dirs {
        let spec = KernelSpec::load(dir)?;
        let verdicts = prune_kernel(&spec, &options)?;

        if json {
            json_out.push(serde_json::json!({
                "kernel": spec.name,
                "cc": cc,
                "candidates": verdicts.iter().map(|cv| {
                    let mut v = serde_json::to_value(&cv.verdict).expect("verdict serializes");
                    v["id"] = cv.config.id().as_str().into();
                    v["config"] = cv.config.to_string().into();
                    v
                }).collect::<Vec<_>>(),
            }));
        } else {
            let (mut clean, mut caveats, mut refused, mut errors) = (0u32, 0u32, 0u32, 0u32);
            println!("{} (cc {cc}):", spec.name);
            for cv in &verdicts {
                match &cv.verdict {
                    Verdict::Clean => clean += 1,
                    Verdict::AdmittedWithCaveats { caveats: c } => {
                        caveats += 1;
                        println!("  ~ {}  {}", cv.config.id(), cv.config);
                        for record in c {
                            println!(
                                "      caveat {} ({}): {}",
                                record.rule, record.confidence, record.message
                            );
                        }
                    }
                    Verdict::Disqualified { records } => {
                        refused += 1;
                        println!("  x {}  {}", cv.config.id(), cv.config);
                        for record in records {
                            println!(
                                "      REFUSED {} at {}: {}",
                                record.rule,
                                record.span.as_deref().unwrap_or("<no span>"),
                                record.reason
                            );
                        }
                    }
                    Verdict::ToolError { detail } => {
                        errors += 1;
                        println!("  ! {}  {}", cv.config.id(), cv.config);
                        println!("      TOOL ERROR (hard stop): {detail}");
                    }
                }
            }
            println!(
                "  => {clean} clean, {caveats} with caveats, {refused} refused, {errors} tool errors"
            );
        }
        if verdicts
            .iter()
            .any(|cv| matches!(cv.verdict, Verdict::ToolError { .. }))
        {
            tool_error = true;
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&json_out)?);
    }
    Ok(if tool_error {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    })
}

fn resolve_kernel_dirs(kernel: Option<&str>, corpus: &Path) -> anyhow::Result<Vec<PathBuf>> {
    match kernel {
        Some(k) => {
            let as_path = PathBuf::from(k);
            let dir = if as_path.join("kernel.toml").is_file() {
                as_path
            } else {
                let in_corpus = corpus.join(k);
                anyhow::ensure!(
                    in_corpus.join("kernel.toml").is_file(),
                    "no kernel.toml at `{k}` or `{}`",
                    in_corpus.display()
                );
                in_corpus
            };
            Ok(vec![dir])
        }
        None => {
            let mut dirs: Vec<PathBuf> = std::fs::read_dir(corpus)
                .with_context(|| format!("reading corpus dir {}", corpus.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.join("kernel.toml").is_file())
                .collect();
            dirs.sort();
            anyhow::ensure!(
                !dirs.is_empty(),
                "no kernels (kernel.toml) under {}",
                corpus.display()
            );
            Ok(dirs)
        }
    }
}

fn cmd_space(
    kernel: Option<&str>,
    corpus: &Path,
    json: bool,
    list: bool,
) -> anyhow::Result<ExitCode> {
    let dirs = resolve_kernel_dirs(kernel, corpus)?;
    let mut entries = Vec::new();
    for dir in &dirs {
        let spec = KernelSpec::load(dir)?;
        let configs = enumerate(&spec)?;
        entries.push((spec, configs));
    }

    if json {
        let out: Vec<serde_json::Value> = entries
            .iter()
            .map(|(spec, configs)| {
                serde_json::json!({
                    "kernel": spec.name,
                    "entry": spec.entry,
                    "domain": spec.domain,
                    "needs_cc": spec.needs_cc,
                    "known": match spec.known {
                        SafetyExpectation::Flip => "flip",
                        SafetyExpectation::Stable => "stable",
                        SafetyExpectation::None => "none",
                    },
                    "raw_size": raw_size(spec),
                    "config_count": configs.len(),
                    "dims": spec.dims_sorted().iter().map(|d| {
                        serde_json::json!({
                            "name": d.name,
                            "role": match d.role {
                                launchbound_space::DimRole::Launch => "launch",
                                launchbound_space::DimRole::Spec => "spec",
                            },
                            "values": d.values.iter().map(ToString::to_string).collect::<Vec<_>>(),
                        })
                    }).collect::<Vec<_>>(),
                    "configs": if list {
                        Some(configs.iter().map(|c| {
                            serde_json::json!({"id": c.id().as_str(), "config": c.to_string()})
                        }).collect::<Vec<_>>())
                    } else { None },
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        for (spec, configs) in &entries {
            let filtered = raw_size(spec) - configs.len() as u64;
            println!(
                "{}: {} configurations ({} dims, {} filtered by constraints){}",
                spec.name,
                configs.len(),
                spec.dims_sorted().len(),
                filtered,
                match spec.known {
                    SafetyExpectation::Flip => "  [known-flip]",
                    SafetyExpectation::Stable => "  [known-stable]",
                    SafetyExpectation::None => "",
                }
            );
            if list {
                for c in configs {
                    println!("  {}  {}", c.id(), c);
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
