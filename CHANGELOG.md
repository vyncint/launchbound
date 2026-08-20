# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Entries that
change measured timings are marked `bench:`.

## [Unreleased]

## [1.0.1] - 2026-08-20

### Added

- The safety gate as a composite GitHub Action (`action/`):
  `uses: vyncint/launchbound/action@v1` prunes a kernel's space in CI with
  no GPU, with verdict counts, the full gate record, and a job-summary
  table as outputs.
- README badges (license, crates.io, pinned toolchain, GPU-not-required,
  CI).

### Changed

- Releases publish through crates.io Trusted Publishing (GitHub OIDC): no
  registry token is stored anywhere.
- termlens dev-dependency upgraded to 0.5 (fallible `send`, identical
  closed-terminal behaviour across Linux and macOS).

## [1.0.0] - 2026-08-20

### Added

- Project bootstrap: pinned workspace (`nightly-2026-04-03`, lockstep with
  `cuda-oxide` and `reconverge`), dual MIT/Apache-2.0 licensing, governance
  docs, safety-gate specification (`docs/SAFETY.md`), and CI.
- `launchbound-space`: kernel.toml tuning spaces, constraints, deterministic
  enumeration, canonical config IDs.
- Six-kernel device-only corpus with declared spaces, [bench] workloads,
  [model] shared-memory expressions, and an MSL twin for reduce-stable.
- `launchbound-prune`: the safety gate — one reconverge run per
  specialization, findings.v1 parsing, and the pure launch-shape decision
  rule; gate tests reproduce the known-flip/known-stable directions.
- `launchbound-build`: scratch specialization, an Apple-container or direct
  `cargo oxide` executor, and a source-hash artifact cache.
- `launchbound-bench` + `launchbound-runner`: plan.v1/results.v1, dlopen'd
  CUDA driver API, cuEvent timing, interval statistics, checkpoint/resume,
  CPU heartbeat, and a watchdog for --allow-unsafe candidates.
- `launchbound-search`: exhaustive and seeded-random orders, honored
  budgets, candidates-per-GPU-hour.
- `launchbound-report`: report.v1 with the REFUSED-BUT-FASTER section,
  indistinguishability, and schema + golden validation.
- `launchbound-model`: occupancy/wave estimator shipping with measured
  per-kernel Spearman correlations (model-calibration.toml).
- `launchbound-metal`: Apple-Silicon measurement with the mandatory no-gate
  notice on every surface.
- `launchbound-tui`: four deterministic views, termlens goldens, and the
  100-iteration stress gate.
- CLI verbs: space, prune, stage, tune (cuda/metal/model), report, apply,
  model.
- docs: ARCHITECTURE, BENCHMARKING, LIMITATIONS, SAFETY, research-baseline.
