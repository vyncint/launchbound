//! launchbound-runner: execute a bench plan on the machine with the GPU.
//! Checkpointed and resumable; see crate docs.

use launchbound_bench::run::RunOptions;
use launchbound_bench::{BenchPlan, parse_budget, run_plan};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: launchbound-runner [--order exhaustive|random] [--seed N] \
                     [--budget-secs S] <plan.json> [results.json]";

/// What the command line asked for, once every value has been understood.
#[derive(Debug)]
struct Args {
    order: String,
    seed: u64,
    budget_secs: Option<f64>,
    plan: PathBuf,
    results: PathBuf,
}

/// Parse the command line, refusing anything it cannot read.
///
/// Every value here used to be taken with `.parse().ok()`, which discards
/// the error. `--budget-secs 30m` -- the spelling `launchbound tune --budget`
/// accepts, prints in `--help`, and anyone types from memory -- silently
/// meant **no budget**, on the machine the whole stage/ship/run split exists
/// to keep cheap. `nan` meant no budget too (`elapsed >= NaN` is false for
/// every elapsed), `-5` meant a sweep that ended before it started, and
/// `--seed abc` became seed 0 and was then *recorded* as `random:0`, so the
/// provenance disagreed with what was typed.
///
/// The laptop-side binary has had all of this right for a while, in a
/// function whose doc comment is a written account of these exact failures.
/// It is the same function now; this one just calls it.
fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut order = "exhaustive".to_string();
    let mut seed = 0u64;
    let mut budget_secs = None;
    let mut positional: Vec<String> = Vec::new();

    let mut args = argv.into_iter();
    while let Some(arg) = args.next() {
        // A flag with no value is an error rather than a default: the shell
        // that dropped it is usually a variable that expanded to nothing.
        let mut value = |flag: &str| -> Result<String, String> {
            args.next()
                .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--order" => order = value("--order")?,
            "--seed" => {
                let text = value("--seed")?;
                seed = text
                    .trim()
                    .parse()
                    .map_err(|_| format!("`{text}` is not a --seed (expected a whole number)"))?;
            }
            "--budget-secs" => {
                let text = value("--budget-secs")?;
                budget_secs = Some(parse_budget("--budget-secs", &text)?);
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            // An unknown flag used to become the plan path, so `--budget 30m`
            // -- the laptop-side spelling, one word short -- was reported as
            // a missing file called `--budget`.
            other if other.starts_with('-') => {
                return Err(format!("unknown argument `{other}`\n{USAGE}"));
            }
            other => positional.push(other.to_string()),
        }
    }

    let Some(plan) = positional.first().map(PathBuf::from) else {
        return Err(USAGE.to_string());
    };
    if positional.len() > 2 {
        return Err(format!(
            "expected at most two paths, got {}: {}\n{USAGE}",
            positional.len(),
            positional.join(" ")
        ));
    }
    let results = positional
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| plan.with_file_name("results.json"));

    Ok(Args {
        order,
        seed,
        budget_secs,
        plan,
        results,
    })
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    let plan = match BenchPlan::load(&args.plan) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let plan_dir = args
        .plan
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    println!(
        "plan: {} ({} candidates, entry {})",
        plan.kernel,
        plan.candidates.len(),
        plan.entry
    );
    let Some(strategy) = launchbound_search::Strategy::parse(&args.order, args.seed) else {
        eprintln!("unknown --order {:?} (exhaustive|random)", args.order);
        return ExitCode::from(2);
    };
    let options = RunOptions {
        order: strategy.order(&plan),
        budget_secs: args.budget_secs,
        strategy: Some(match strategy {
            launchbound_search::Strategy::Exhaustive => "exhaustive".to_string(),
            launchbound_search::Strategy::Random { seed } => format!("random:{seed}"),
        }),
    };
    let mut progress = |line: &str| println!("{line}");
    match run_plan(&plan, &plan_dir, &args.results, &options, &mut progress) {
        Ok(results) => {
            println!(
                "done: {} candidates, {:.1} GPU-seconds total, results at {}",
                results.candidates.len(),
                results.total_gpu_seconds,
                args.results.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn parse(args: &[&str]) -> Result<super::Args, String> {
        parse_args(args.iter().map(|s| (*s).to_string()))
    }

    #[test]
    fn a_budget_in_the_spelling_the_cli_takes_is_understood() {
        // Not silently ignored, which is what `.parse().ok()` did with every
        // one of these: `30m` is what `launchbound tune --budget` accepts.
        for (text, secs) in [("30m", 1800.0), ("90s", 90.0), ("1h", 3600.0), ("45", 45.0)] {
            let args = parse(&["--budget-secs", text, "plan.json"]).unwrap();
            assert_eq!(args.budget_secs, Some(secs), "{text}");
        }
    }

    #[test]
    fn an_unbounded_budget_is_refused_rather_than_becoming_no_budget() {
        // Each of these was accepted, and each produced a sweep that ran to
        // the end of the plan on rented hardware. `-5` is the other
        // direction: a budget already spent before the first candidate.
        for text in ["nan", "inf", "-5", "abc", "", "   "] {
            let err = parse(&["--budget-secs", text, "plan.json"]).unwrap_err();
            assert!(
                err.contains("--budget-secs"),
                "{text}: the message must name the flag: {err}"
            );
        }
    }

    #[test]
    fn a_seed_that_is_not_a_number_is_refused_rather_than_becoming_zero() {
        // Seed 0 is a legitimate seed, and it is written into the results as
        // `random:0`, so the silent fallback did not merely lose the value --
        // it recorded a different one as if it had been asked for.
        let err = parse(&["--seed", "abc", "--order", "random", "plan.json"]).unwrap_err();
        assert!(err.contains("--seed"), "{err}");
        assert_eq!(parse(&["--seed", "7", "plan.json"]).unwrap().seed, 7);
    }

    #[test]
    fn a_flag_with_no_value_is_an_error_not_a_default() {
        for flag in ["--order", "--seed", "--budget-secs"] {
            let err = parse(&["plan.json", flag]).unwrap_err();
            assert!(err.contains("needs a value"), "{flag}: {err}");
        }
    }

    #[test]
    fn an_unknown_flag_does_not_become_the_plan_path() {
        // `--budget 30m` is the laptop-side spelling, one word short. It used
        // to be reported as a missing file named `--budget`.
        let err = parse(&["--budget", "30m", "plan.json"]).unwrap_err();
        assert!(err.contains("unknown argument `--budget`"), "{err}");
    }

    #[test]
    fn the_defaults_are_unchanged() {
        let args = parse(&["runs/x/plan.json"]).unwrap();
        assert_eq!(args.order, "exhaustive");
        assert_eq!(args.seed, 0);
        assert_eq!(args.budget_secs, None);
        assert_eq!(args.results, std::path::Path::new("runs/x/results.json"));

        let args = parse(&["plan.json", "elsewhere.json"]).unwrap();
        assert_eq!(args.results, std::path::Path::new("elsewhere.json"));
    }

    #[test]
    fn no_arguments_and_too_many_paths_both_say_what_is_expected() {
        assert!(parse(&[]).unwrap_err().contains("usage:"));
        let err = parse(&["a.json", "b.json", "c.json"]).unwrap_err();
        assert!(err.contains("at most two paths"), "{err}");
    }
}
