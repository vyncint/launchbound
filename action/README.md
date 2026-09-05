# launchbound safety gate — GitHub Action

Run the convergence safety gate over a cuda-oxide kernel's tuning space on
every push, with **no GPU anywhere**: every launch/specialization candidate
comes back admitted, admitted-with-caveats, or refused with a rule ID and
source span. This is the part of the tuning pipeline that finds the bugs,
and it runs on a plain runner.

## Paste this

```yaml
name: convergence-gate
on: [push, pull_request]

permissions:
  contents: read

jobs:
  gate:
    runs-on: ubuntu-latest
    steps:
      # Your repo, containing the kernel crate + kernel.toml.
      - uses: actions/checkout@v4
        with:
          path: myrepo
      # The kernel's cuda-device path dependency must resolve: check out the
      # cuda-oxide commit your crate pins, in the layout your Cargo.toml
      # expects (here: a sibling directory).
      - uses: actions/checkout@v4
        with:
          repository: NVlabs/cuda-oxide
          ref: 50d07314eb8b7d5ec821ba02b0048a753c20dd4e
          path: cuda-oxide

      - name: Safety gate
        id: gate
        uses: vyncint/launchbound/action@v2
        with:
          kernel: myrepo/kernels/my-reduction
          cc: "8.6"          # the part you will actually run on
          fail-on: refused   # keep this space all-clean

      - name: Use the outputs
        run: |
          echo "verdict: ${{ steps.gate.outputs.verdict }}"
          echo "${{ steps.gate.outputs.refused }} refused of \
          $(( ${{ steps.gate.outputs.admitted }} + ${{ steps.gate.outputs.caveats }} + ${{ steps.gate.outputs.refused }} )) candidates"
```

## What it needs from your kernel

The kernel directory must contain a `kernel.toml` declaring the tuning
space (see [the corpus](../corpus/) for six worked examples) and a
device-only crate whose `cuda-device` path dependency resolves in the
checkout layout above. `src/params.rs` carries the compile-time parameters
the gate specializes per candidate.

## Inputs

| input | default | |
|---|---|---|
| `kernel` | — | path to the kernel directory (required) |
| `cc` | — | target compute capability, e.g. `"8.6"` (required; verdicts do not transfer across parts) |
| `fail-on` | `tool-error` | `never`, `refused`, or `tool-error` |
| `version` | `latest` | launchbound-cli release to install |
| `reconverge-version` | `0.5.0` | reconverge release from crates.io — moves in lockstep with `toolchain` |
| `toolchain` | `nightly-2026-04-03` | the nightly that built that reconverge |
| `summary` | `"true"` | write the verdict table to the job summary |

## Outputs

`verdict` (`clean` / `caveats` / `refused` / `tool-error`), the counts
(`admitted`, `caveats`, `refused`, `tool-errors`), and `json` — the full
gate record with rule IDs and source spans, for anything the counts miss.

## The toolchain contract

The gate compiles and analyzes your kernel **under the action's
`toolchain` input, not your repository's `rust-toolchain.toml`** — that is
inherent to reconverge being a rustc driver that must pair with one exact
nightly. If your kernel needs a different nightly, pass a matching
`toolchain`/`reconverge-version` pair; changing one without the other
fails at install time.

## Honesty notes

A clean gate is **not a proof of correctness** — the gate inherits
reconverge's documented limits, and the launch-shape classifier recognizes
the measured `warp_id()` family (see
[docs/LIMITATIONS.md](../docs/LIMITATIONS.md)). The gate covers CUDA
kernels only; there is no MSL analyzer, so Metal code is not checked. The
first run builds reconverge from its crates.io release (a few minutes);
nothing is cached, so the gate can never be stale, poisoned, or the wrong version.
