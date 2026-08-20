# The kernel corpus

Real, small, self-contained `cuda-oxide` kernels — the tuning subjects.
Fixtures, not products: minimal and readable beats clever.

| kernel | why it is here | expected gate behaviour |
|---|---|---|
| [reduce-flip](reduce-flip/) | the README's `warp_id()` guard — the case the product exists for | **known-flip**: disqualified at `block_x > 32`, admitted at 32 |
| [reduce-stable](reduce-stable/) | the same reduction with unconditional barriers | **known-stable**: nothing disqualified, ever |
| [matmul-tiled](matmul-tiled/) | genuinely large tile/block space, constraint-heavy | clean |
| [scan-block](scan-block/) | barrier-dense (2 per doubling round) in a uniform loop | clean — a false positive here poisons every scan |
| [histogram](histogram/) | shared-memory capacity pressure up to 48 KB (RC004, `--cc`) | clean at default budget |
| [stencil-1d](stencil-1d/) | register/unroll trade-offs, `launch_bounds` as a spec dim | clean — no barriers at all |

## Structure

Every kernel is a **standalone, device-only lib crate**:

- **Not a workspace member** — the main workspace builds and tests with no
  GPU, no CUDA SDK, and no cuda-oxide checkout.
- **Depends on `cuda-device` alone**, via a path dependency on a sibling
  checkout (`../../../cuda-oxide`). `cuda-core`/`cuda-host` pull in
  `cuda-bindings`, whose build script needs `cuda.h` — that would kill the
  laptop prune path (measured in docs/research-baseline.md).
- **`kernel.toml`** declares the tuning space: dimensions (launch-role or
  spec-role) and constraints. `launchbound space` reads it.
- **`src/params.rs`** holds the compile-time parameters at repo defaults.
  The tuner rewrites this file per candidate *in a scratch copy*; the repo
  copy never changes.
- **`needs_cc`** records the minimum compute capability (cuda-oxide emits
  `.target sm_80` PTX, so 8.0 across the board today).

## Prerequisite

```bash
git clone https://github.com/NVlabs/cuda-oxide ../cuda-oxide   # sibling of this repo
git -C ../cuda-oxide checkout 50d07314                          # the pinned commit
```
