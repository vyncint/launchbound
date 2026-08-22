# S0 — the three costs that decide the architecture

Every number here comes from a re-runnable command, recorded verbatim. The
pipeline is `enumerate → prune → compile → benchmark → rank`
(docs/ARCHITECTURE.md); this baseline measures what each of the three non-trivial steps costs, because
the ratio between them decides the search architecture.

Measured 2026-08-20.

## Provenance

| component | value |
|---|---|
| tier-0/1 host | Apple M4 Pro, 12 cores, 24 GB, macOS 26.6.1 |
| tier-1 guest | Apple `container` 1.2.0, **native arm64** Ubuntu 24.04 (no Docker, no Rosetta — operator requirement), CUDA toolkit 13.2 (sbsa), LLVM 21.1.8, container `cuda-oxide-dev` |
| tier-2 box | AWS `g5.xlarge` spot @ **$0.364/hr**, us-east-2c, **NVIDIA A10G** (`sm_86`, cc 8.6), driver 595.71.05, CUDA 13.2 (V13.2.51), LLVM 21.1.8 — chosen over the T4 by the operator; barely above T4 spot ($0.335/hr) |
| pinned toolchain | `nightly-2026-04-03` (`rustc 1.96.0-nightly (55e86c996 2026-04-02)`) |
| reconverge | `cargo-reconverge 0.1.11` (built at `~/Projects/reconverge/target/release`) — **the version these measurements were taken with**; the gate now pins 0.3.0, which was verified to admit the identical set (see below) |
| cuda-oxide | checkout `50d07314eb8b7d5ec821ba02b0048a753c20dd4e` — the tree synced to the box (the box AMI's own stale clone reports `e28248c1`, but `./gpu sync` replaces the working tree and excludes `.git`, so the synced tree is what compiled) |
| subject kernels | `s0-reduce` (device-only lib crate, dep `cuda-device` only, containing the README's known-flip reduction); cuda-oxide examples `vecadd` (small) and `tiled_gemm` (large) |
| evidence logs | `~/Projects/cuda-oxide/.gpu-evidence/20260820T{071248,071807,071959}Z.log` |

## Cost (a): `cargo reconverge check --strict` per candidate — tier 0, $0

Command, run in the subject crate:

```bash
PATH=~/Projects/reconverge/target/release:$PATH \
  cargo reconverge check --strict --message-format json --cc 7.5
```

| condition | wall time |
|---|---|
| cold (after `cargo clean`; builds `cuda-device` deps once) | 4.11 s |
| re-check after a real source change (tile 128→256→64→512) | **0.07–0.09 s** |
| re-check, no change | 0.04–0.05 s |

Re-analysis was verified real, not cached: deleting the `warp_id()` guard
makes the RC001 finding disappear (2 findings → 1); restoring it brings the
finding back, span tracking the source line.

## Cost (b): one full `cuda-oxide` compile per specialization — tier 1, $0

Command, in the container (`container exec cuda-oxide-dev bash -lc ...`),
against the pinned checkout mounted at `/work`:

```bash
cd /work && time cargo oxide inspect vecadd
```

| condition | container (arm64, M4 Pro) | box (x86_64, g5.xlarge) |
|---|---|---|
| first build of the workspace + backend | 1 m 33 s | 4 m 37 s (after first `./gpu sync` dirtied mtimes) |
| first build of one new example crate (`tiled_gemm`) | — | 72 s |
| re-compile after a real source change | **1.56–1.64 s** | **1.05–1.22 s** |
| no change (cache hit) | 1.56 s | 1.05 s |

Recompilation was verified real: a semantic change (`+`→`-`) produced
different PTX; reverting reproduced byte-identical PTX (determinism).

**The arm64 container emits byte-identical PTX to the x86_64 box** —
`diff` of `vecadd` PTX (82 lines, `.version 7.0`, `.target sm_80`) is empty.
The no-Docker/no-Rosetta tier-1 path is validated: compile on the Mac for
free, ship PTX to the box. Note `cargo oxide` fingerprints by content, not
mtime: a `touch` does not trigger recompile.

## Cost (c): one benchmark of one configuration — tier 2, $0.364/hr

Command (evidence-logged): `./gpu verify ~/Projects/cuda-oxide bash -lc
'cd ~/cuda-oxide && time cargo oxide run vecadd'`

| condition | wall time |
|---|---|
| first run (host binary compile + JIT + execute) | 5.54 s |
| warm run (JIT + launch + execute, 1024-element kernel) | **1.30 s** |

The warm-run number is the per-configuration floor: launchbound's harness
will load prebuilt PTX and add warmup + N timed repeats, which for
microsecond-scale kernels adds little to the ~1.3 s process overhead.
GPU-seconds consumed this session: ≈ 25 min box uptime ≈ **$0.16** for the
entire S0 measurement set (re-runnable: `./gpu cost`, which lags a day).

## The ratio, and what it decides

Steady-state, per candidate:

```
prune : compile : benchmark  ≈  0.08 s : 1.2–1.6 s : 1.3 s   ≈  1 : 15–20 : 16
```

- **Pruning before compiling is confirmed** — a pruned candidate costs 5%
  of a compiled one, and $0 instead of the box's hourly rate at benchmark
  time. The prune-before-compile ordering stands.
- **Assumption overturned:** the design brief expected compilation to *dominate the
  entire search*. Steady-state it does not — per-candidate compile is ~1.2 s,
  the same order as a benchmark. What actually dominates are **one-time
  costs**: workspace first-build (1.5–4.5 min per environment), first build
  of each kernel crate (~72 s), and box launch (~2 min). The search
  architecture should amortize per-kernel setup, batch candidates, and keep
  the artifact cache (S3) hot — not fear per-candidate compiles. (Caveat:
  measured on small example kernels; a heavy specialization taking 10 s
  would shift the ratio, not the ordering.)
- **A 30-minute budget affords roughly 1,300 measured candidates**
  (1800 s ÷ ~1.35 s/benchmark), i.e. the budget is bounded by benchmark
  process overhead, not compile time — and costs ≈ $0.18 of GPU time on the
  A10G. At the project's supply (~66 GPU-hours/month) that is ~130 such runs a
  month. Exhaustive search over spaces up to ~10³ configurations is
  therefore *affordable*; model-guided search earns its keep above that.
- **`--cc` for this hardware is 8.6** (A10G). A verdict at `--cc 8.6` does
  not transfer to the T4 (`--cc 7.5`); the corpus gate tests must pin the
  capability they assume.

## Tier-0 facts that reshape S1/S2 (assumptions overturned)

1. **`cargo reconverge check` on the Mac works only for device-only
   crates.** A crate depending on `cuda-core`/`cuda-host` (as every
   cuda-oxide example binary does) fails on macOS in `cuda-bindings`' build
   script — `could not find cuda.h` — and reconverge reports exit 2 (tool
   error), correctly refusing to pass by omission. Consequence: **corpus
   kernels are lib crates depending on `cuda-device` alone**, host-side
   runners live elsewhere, or the laptop prune path does not exist.
2. **Exit code 0 does not mean no findings.** On the known-flip kernel with
   `--strict`, reconverge emits RC001 (barrier under thread-divergent
   control, the known-flip finding) and RC005 (no launch contract) at `warning`
   confidence and exits 0. The S2 gate must parse `findings.v1`; exit codes
   alone are only usable for the exit-2 hard stop.

Findings on the subject kernel (`--strict`, `--cc 7.5`):

| code | confidence | message |
|---|---|---|
| RC005 | warning | kernel `reduce_flip` calls `index_1d()` without a launch contract |
| RC001 | warning | kernel `reduce_flip` may execute `sync_threads()` under thread-divergent control |

## Analyzer equivalence: 0.1.11 → 0.3.0

The gate's pinned analyzer moved from `cargo-reconverge` 0.1.11 to 0.3.0. A
newer analyzer can change **what the gate admits**, which is a change in
product behaviour rather than a dependency bump — so the corpus was re-run
under both, on the same toolchain, and compared.

Measured 2026-08-22 on `nightly-2026-04-03`, cuda-oxide `50d07314`, tier 0
(no GPU):

```console
$ cargo run -q -p launchbound-cli -- prune --cc 8.6
```

| kernel | clean | caveats | refused | tool errors |
|---|---|---|---|---|
| histogram | 12 | 0 | 0 | 0 |
| matmul-tiled | 18 | 0 | 0 | 0 |
| reduce-flip | 3 | 0 | **8** | 0 |
| reduce-stable | 11 | 0 | 0 | 0 |
| scan-block | 4 | 0 | 0 | 0 |
| stencil-1d | 45 | 0 | 0 | 0 |
| **total** | **93** | **0** | **8** | **0** |

**The two runs are byte-identical** — not merely equal in the totals, but the
same candidate hashes, the same `REFUSED RC001` lines, the same reasons. The
eight refusals are the `reduce-flip` candidates at block sizes above one warp,
which is the corpus's known flip and the behaviour the gate exists to produce.

The gate tests pass under 0.3.0 unchanged, including
`known_flip_kernel_disqualifies_above_one_warp` and
`known_stable_kernel_disqualifies_nothing`.

**What this does and does not establish.** It establishes that on *this*
corpus, at cc 8.6, the two analyzers decide identically — so the bump carries
no behaviour change this project can observe. It does not establish that they
are equivalent in general: reconverge gained multi-warp replay, bounded
inlining and unmasked warp-wrapper analysis between these versions, and a
kernel exercising those paths could well be decided differently. The corpus is
the evidence, and the corpus is six kernels.
