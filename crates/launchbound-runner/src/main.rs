//! launchbound-runner: execute a bench plan on the machine with the GPU.
//! Checkpointed and resumable; see crate docs.

use launchbound_bench::run::RunOptions;
use launchbound_bench::{BenchPlan, run_plan};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut order_name = "exhaustive".to_string();
    let mut seed = 0u64;
    let mut budget_secs: Option<f64> = None;
    let mut positional = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--order" => order_name = args.next().unwrap_or_default(),
            "--seed" => seed = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--budget-secs" => budget_secs = args.next().and_then(|v| v.parse().ok()),
            _ => positional.push(arg),
        }
    }
    let usage = "usage: launchbound-runner [--order exhaustive|random --seed N --budget-secs S] <plan.json> [results.json]";
    let Some(plan_path) = positional.first().map(PathBuf::from) else {
        eprintln!("{usage}");
        return ExitCode::from(2);
    };
    let results_path = positional
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| plan_path.with_file_name("results.json"));

    let plan = match BenchPlan::load(&plan_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let plan_dir = plan_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    println!(
        "plan: {} ({} candidates, entry {})",
        plan.kernel,
        plan.candidates.len(),
        plan.entry
    );
    let Some(strategy) = launchbound_search::Strategy::parse(&order_name, seed) else {
        eprintln!("unknown --order {order_name:?} (exhaustive|random)");
        return ExitCode::from(2);
    };
    let options = RunOptions {
        order: strategy.order(&plan),
        budget_secs,
        strategy: Some(match strategy {
            launchbound_search::Strategy::Exhaustive => "exhaustive".to_string(),
            launchbound_search::Strategy::Random { seed } => format!("random:{seed}"),
        }),
    };
    let mut progress = |line: &str| println!("{line}");
    match run_plan(&plan, &plan_dir, &results_path, &options, &mut progress) {
        Ok(results) => {
            println!(
                "done: {} candidates, {:.1} GPU-seconds total, results at {}",
                results.candidates.len(),
                results.total_gpu_seconds,
                results_path.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
