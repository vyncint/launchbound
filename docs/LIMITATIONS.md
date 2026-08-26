# Limitations

A tool that overstates its reach is worse than one that does less. These are
launchbound's, with numbers where we have them. Everything here was true on
2026-08-22 against the pins in rust-toolchain.toml and CONTRIBUTING.md.

## The gate inherits reconverge's limits, wholesale

A clean gate is **not a proof of correctness**. `reconverge` (v0.4.0) is
summary-based and interprocedural, handles reducible control flow only,
cannot evaluate non-literal masks, and puts data races entirely out of
scope. Its own documentation is the authority; launchbound adds no analysis
of its own on top — it only decides launch-shape questions the analyzer
deliberately leaves open.

The decision rule's launch-shape classifier currently recognizes the
`warp_id()` divergence family — the only family the simt-diff corpus
measured flipping (11 of 147 cases, all `warp_id()`), and the only one this
project's corpus reproduces. A launch-shape-dependent hazard from a source
the classifier does not recognize would be admitted **with a caveat**, not
refused.

## The Metal path has no gate at all

`reconverge` analyzes cuda-oxide kernels. No equivalent exists for MSL and
this project does not build one. Apple GPUs have 32-wide SIMD-groups and
simd-scoped collectives, so the same bug class exists there and is simply
**not checked**. Every Metal surface says so; a test asserts the notice
cannot be omitted. This asymmetry is permanent unless someone builds an MSL
analyzer.

## Refused configurations may not hang — and may be "faster"

On the A10G (driver 595.71.05), the refused `warp_id()`-guarded candidates
measured under `--allow-unsafe` **completed silently** — up to 3.00x faster
than the chosen safe configuration — because warps that exit a kernel
release `bar.sync`. That is undefined behavior manifesting as a plausible
timing, which is more dangerous than a hang: nothing looks wrong. Those
timings exist to make the rejection report concrete and are never published
as safe results. On other drivers or parts the same configurations may hang
forever.

Conversely: a configuration this tool refuses may be safe in a program
whose real launch contract differs from the declared one.

## Model output is an estimate, and its quality is a measured number

The analytical model ranks by occupancy and wave count, nothing else. Its
Spearman rank correlation against real A10G measurements, per corpus kernel
(n = candidates): stencil-1d **0.938** (45), histogram **0.861** (12),
reduce-stable **0.808** (11), matmul-tiled **0.653** (18), scan-block
**0.000** (4 — a space too small to rank). Kernels without a calibration
entry are reported as UNCALIBRATED. Every estimate carries the `estimated`
label and this correlation; an estimate presented as a measurement is a
release-blocking defect.

## Measurement noise floor

On the A10G, repeated sweeps of identical configurations reproduced within
their 95% CIs (11/11 candidates across independent sweeps 36 minutes
apart). Typical interval half-widths were under 1% of the median for
microsecond-scale kernels. Two configurations whose intervals overlap are
reported indistinguishable, never ranked. Kernel-only times come from CUDA
events; they exclude launch and transfer overhead, which a real application
pays.

## Results do not port

A tuning result is valid only for the GPU, driver, and compiler versions in
its provenance. In particular `sm_75` (T4) and `sm_86` (A10G) differ in SM
count, threads/SM, and shared-memory capacity, so neither timings **nor
safety verdicts at a given `--cc`** transfer between them. All published
numbers in this repository are from the A10G; nothing has been measured on
a T4.

## cuda-oxide is alpha

Its README says to expect bugs, incomplete features, and API breakage. The
pins (CONTRIBUTING.md) move together or not at all; both upstreams had already
moved past the pinned versions on the day this was written. cuda-oxide
emits `.target sm_80` PTX for this corpus, so `needs_cc = "8.0"` across the
board and nothing here runs on pre-Ampere parts. `cargo check` under the
reconverge driver does not evaluate all codegen-time consts (an invalid
`#[unroll]` factor passed the gate and failed the real compile), so a
gate-clean candidate can still fail to build.

## Every published timing has evidence

Timings in this repository trace to `.gpu-evidence` logs (gitignored;
evidence for the human, not repo content) and to re-runnable commands
recorded in docs/research-baseline.md. A number without
provenance is not published.
