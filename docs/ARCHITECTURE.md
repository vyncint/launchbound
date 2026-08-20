# Architecture

```
enumerate → PRUNE (reconverge, MIR, no GPU) → compile (cuda-oxide → PTX) → benchmark → rank
```

The order is the economics (docs/research-baseline.md): pruning removes
unsafe candidates at ~0.1 s and $0 each before compilation (~1 s each, $0 in
the container) and benchmarking (the only step that costs money) ever see
them. On this corpus the refused candidates never reach a compiler at all
unless `--allow-unsafe` explicitly stages them for the rejection report.

## Crates

| crate | job |
|---|---|
| `launchbound-space` | kernel.toml → dimensions (launch/spec roles), constraint filtering, deterministic enumeration, canonical `c1-` config IDs |
| `launchbound-prune` | the gate: one reconverge run per specialization, `findings.v1` parsing, the pure decision rule (docs/SAFETY.md) |
| `launchbound-build` | scratch copies (`params.rs` rewriting), the `cargo oxide` executor (direct or Apple-container), artifact cache keyed by source hash |
| `launchbound-bench` | `plan.v1` bench plans, the CUDA driver API via dlopen, cuEvent timing, `results.v1` with checkpoint/resume, interval statistics |
| `launchbound-search` | strategies as pure (plan, seed) → order functions; budget accounting |
| `launchbound-model` | occupancy/wave estimator; closed device table; Spearman tooling; ships with model-calibration.toml numbers attached |
| `launchbound-report` | `report.v1`: chosen + intervals, indistinguishability, and the REFUSED-BUT-FASTER section |
| `launchbound-runner` | the box-side binary: plan in, results out, watchdog for unsafe candidates, CPU heartbeat |
| `launchbound-metal` | Apple-Silicon measurement; **no gate**, and every surface says so |
| `launchbound-tui` | four deterministic views over a run dir; termlens-tested |
| `launchbound-cli` | `space` / `prune` / `stage` / `tune` / `report` / `apply` / `model` |

## Design decisions that carry weight

**One analyzer run serves every launch shape.** Measured: reconverge's
answer does not change with the declared block (docs/research-baseline.md),
so the gate runs once per *specialization* and the launch-shape decision is
a pure function evaluated per candidate. Prune cost scales with distinct
sources, not with the space.

**Specialization is a file rewrite.** Every corpus kernel keeps its
compile-time parameters in `src/params.rs` (MSL twins use `constant
constexpr` lines). The tuner rewrites that file in a scratch copy — never
in the repo — so candidates are ordinary crates, reconverge sees ordinary
MIR, the artifact cache keys on the source hash, and `apply` emits exactly
what was measured.

**The runner is self-contained.** It dlopens `libcuda` and drives the
driver API directly (module load, launch, cuEvent pairs), so the box needs
PTX and a plan — not a cuda-oxide checkout, not reconverge. Everything
builds and unit-tests on machines with no CUDA. Argument ABI follows
cuda-oxide's PTX lowering (slices become `ptr, len` slots; `stage`
validates the declared layout against the actual `.entry` signature).

**Everything on the box dies; everything resumes.** The idle alarm,
dead-man switch, and spot reclaim make eviction normal. Results checkpoint
after every candidate (atomic rename); search orders are pure functions of
(plan, seed) so a rerun of the same command continues where it stopped;
unsafe candidates write their `timeout` record *before* launching, because
a hung kernel can only be escaped by killing the process.

**Budgets are honored between candidates.** A spent `--budget` stops the
sweep resumably and is recorded in results (`budget_exhausted`), along with
GPU-seconds and the strategy — candidates-per-GPU-hour is a first-class
output because the supply is ~66 GPU-hours/month.
