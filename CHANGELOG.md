# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Entries that
change measured timings are marked `bench:`.

## [Unreleased]

## [2.0.0] - 2026-08-26

A major, and both reasons are in the "breaking" list this project keeps in
[docs/RELEASING.md](docs/RELEASING.md): a changed CLI flag, and a change to
what the gate admits.

### Changed — breaking

- **The safety gate pins `reconverge` 0.4.0**, up from 0.3.0, and 0.4.0
  reads a shared-memory length written as a **named const**. 0.3.0 could
  only read a literal: `SharedArray<f32, TILE>` arrived as an unevaluated
  const, the analyzer's `eval_target_usize` refused it, and the static was
  dropped from the RC004 budget with no finding and no diagnostic.

  This is not an abstract gap — it is the shape every tunable kernel has.
  `corpus/matmul-tiled` declares `SharedArray<f32, { TM * TK }>`, and
  launchbound rewrites `TM`/`TK` per candidate, so **every configuration
  this tool tries took the path RC004 could not see**. A space with an
  over-cap tile pruned as all-clean.

  Verified against the corpus: the six kernels' verdicts are unchanged
  (their tiles are well under the cap), and raising `matmul-tiled` to
  `TM = TK = 128` now produces
  `error[RC004]: kernel matmul declares 73728 bytes`, where 0.3.0 said
  nothing. Any kernel whose shared memory is sized by a const may now be
  refused where it previously passed — which is the gate working, and is
  why this is a major.

- **`--cc` is required by `tune`**, as it already was by `prune` and
  `model`. It defaulted to `8.6`, which meant the one command whose answer
  you act on quietly picked a device, while the two inspection commands
  made you choose. `prune --cc`'s own help says a verdict at one capability
  does not transfer to another; RC004 is a capacity check, and 8.6 offers
  164 KB per SM against 7.5's 64 KB.

  `launchbound tune <kernel> --backend model` now asks for `--cc`.

### Added

- **`--cc` is validated at the command line, and the CUDA spellings work.**
  A mistyped `--cc` used to be handed to `cargo reconverge` once per
  candidate: eleven subprocesses for `reduce-flip`, 101 over the corpus, and
  ninety lines of output in which the actual problem appeared nowhere. It is
  now one line in ~100ms with nothing spawned. `--cc 86` and `--cc sm_86`
  are normalized to `8.6` rather than rejected — for two digits the mapping
  is unambiguous, and it is the spelling a CUDA user already has.

### Fixed

- **A reconverge failure reports what reconverge said.** The tool error
  showed the *last* six lines of its stderr — a reasonable-looking default,
  since a failing tool usually fails last, and reliably the wrong six:
  reconverge prints its diagnosis first and its usage reference after it, so
  the tail was the exit-code legend. reconverge 0.4.0 stopped printing usage
  after a bad value, which fixes that case at the source; this reads the
  lines marked `error:` regardless, because no caller controls what its
  analyzer prints, and falls back to the head rather than the tail.

- **`model` says that it is not gated.** It ranks the whole space, and on
  `reduce-flip` its top five are all configurations the gate refuses — the
  fastest row was a kernel that hangs, under a header that carefully said
  "estimated, not a measurement" and nothing about safety. It still runs no
  gate and needs no `reconverge`; it now says so, and names `tune --backend
  model` as the gated form.

- **`tune --backend model` no longer leaves an empty run directory.** The
  directory was created before the backend match, for every backend, and the
  model path writes nothing — so every run littered `runs/`, which is
  checked in, and `launchbound report` on it failed with `verdicts.json: No
  such file`. An `--out` given to this backend is now answered rather than
  silently ignored.

- **`launchbound-tui --help` prints help.** It read `args()` directly, so
  every flag was taken as a run-directory path: `--help` came back as
  `run dir: --help/verdicts.json: No such file or directory`, which reads as
  a broken tool. `-h`, `--help`, `-V` and `--version` answer; any other
  leading dash is reported as an unknown option, which is what stops the
  next flag landing here as a path. This is a published binary.

- **The chosen configuration's interval is dropped, not cut.** At eighty
  columns — the default terminal size, and the width this suite mandates —
  the line ended `0.0400 ms [0.0398, `: a number with no upper bound and a
  dangling comma, on the one line carrying the result. The interval now goes
  whole when it does not fit; at 110 columns it is unchanged.

- **The TUI goldens wait for a finished frame.** They synced on a 150ms
  quiet period, which is a guess at how long a repaint takes; on a loaded
  runner the app pauses mid-repaint and the screen read is half-painted.
  This had already cost the suite once — `ranking_scrolls_a_long_candidate_list`
  carries a comment about a golden blessed from a too-early capture, which
  then verified nothing while passing — and the same shape failed
  reconverge's `main` on macOS. The binary already brackets every repaint in
  DEC 2026 synchronized updates, so `wait_frame` observes only whole frames.
  The 100-iteration stress gate went from **15.8s to 0.7s**.

### Documentation

- The CLI table listed `launchbound tui <run>`, which is not a subcommand —
  the binary is `launchbound-tui`. It also omitted `model`, and showed
  `tune` without the `--cc` it now requires.
- The Action's input table still gave `reconverge-version` as `0.1.11`, two
  releases stale.

## [1.2.0] - 2026-08-22

### Changed

- **The safety gate now pins `reconverge` 0.3.0**, up from 0.1.11 — two minor
  versions of analyzer the gate was not getting. The pin moves in four places:
  the corpus workflow, the Action's `reconverge-version` default, and the two
  documents that name it.

  **The gate admits exactly the same set.** A newer analyzer can change what
  the gate refuses, which is a change in product behaviour rather than a
  dependency bump, so the corpus was re-run under both versions on the same
  toolchain and compared: **93 clean, 8 refused, 0 caveats, 0 tool errors**,
  and the two runs are **byte-identical** — same candidate hashes, same
  `REFUSED RC001` lines, same reasons. The eight refusals are `reduce-flip`
  above one warp, which is the corpus's known flip and the behaviour the gate
  exists to produce. The measurement is recorded in
  [docs/research-baseline.md](docs/research-baseline.md#analyzer-equivalence-0111--030).

  No toolchain change was needed: launchbound and reconverge 0.3.0 already pin
  the same `nightly-2026-04-03`, so the rule that the analyzer and the
  toolchain move together is satisfied without moving either.

  What this does *not* establish is general equivalence. reconverge gained
  multi-warp replay, bounded inlining and unmasked warp-wrapper analysis
  between these versions; a kernel exercising those paths could be decided
  differently. Six kernels are the evidence, and six kernels are what they are.

- `docs/LIMITATIONS.md` now describes the limits of the analyzer the gate
  actually runs, and carries the date it was re-checked.

## [1.1.0] - 2026-08-22

### Added

- **The overview shows the field, not just the winner.** Under the chosen
  configuration it now lists every measured candidate fastest-first, with what
  each costs relative to the fastest. An autotuner's output is not a
  configuration, it is the claim that the configuration is *worth choosing*,
  and that claim is unreadable without the alternatives: a field inside a
  percent says the tuning did not matter, one spanning 4x says it did.

  The percentage is anchored on the **fastest** measured candidate rather than
  on the chosen one, deliberately. Where a refused configuration measured
  faster — the case the overview already warns about in bold — anchoring on
  the chosen would bury the number that matters. On the `reduce-flip` fixture
  it now reads plainly: the disqualified candidate is fastest and the safe
  choice costs 90.5%.

  The list is the head of view 2's, from one shared ordering, so the two
  cannot disagree. Where it does not fit, it truncates and says how many more
  view 2 holds.

### Changed

- **The overview fills the pane it was given.** It drew four lines into a
  twenty-five-row box; the rest was blank.
- **termlens 0.5 → 0.6** for the TUI test harness. No source change was
  needed: the only breaking change in 0.6 is `GraphicsSeen` becoming `Clone`
  rather than `Copy`, and this suite asserts on text. What the upgrade buys
  is a fix that matters here — `openpty` is now retried when the machine is
  briefly out of PTY devices, which macOS is whenever a suite runs one test
  per core.

### CI

- **The stress workflow gained a flake hunt.** The existing gate is unchanged —
  the TUI suite once per OS on every push and pull request. The hunt runs the
  same suite many times over, split across five machines that each use a
  different `--test-threads`, on dispatch or weekly. Five machines because a
  race that only loses on a slow runner gets five rolls rather than one; five
  concurrencies because that is the axis a PTY suite's faults live on, and
  running one point five times only samples that point harder.
- **A published-package check**, at release and weekly: `cargo install
  launchbound-cli` into a clean directory from crates.io, then real work with
  no GPU, no network and no checkout — enumerating a config space whose
  constraint rules out exactly one of six pairs. It is the only check here
  that can fail without anybody touching the repository, because `cargo
  install` resolves dependencies fresh where CI resolves against the lockfile.
- **The gate job no longer caches the analyzer.** It cached
  `cargo-reconverge` and `reconverge-driver` and skipped the install on a hit;
  a cached binary is not evidence that the gate holds against the *published*
  analyzer. Nothing in this repository's CI or its action caches anything now.
- **Squash merges no longer fail the DCO check.** GitHub rewrites a
  web-flow squash commit's author email after the sign-off is written, so the
  exact match the check required was impossible by construction. Such commits
  must still carry a sign-off; every other rule is unchanged.

## [1.0.2] - 2026-08-20

### Fixed

- Reports with no measurements are labelled `unmeasured` instead of
  `measured` (schema gains the value; a run directory with only a gate
  record no longer misstates itself).
- The release workflow refuses to publish when the pushed tag disagrees
  with the workspace version, reads the version deterministically, and
  warns loudly on manual dispatch; the trigger glob is now defense-in-depth
  rather than the only barrier.
- Every step of the GitHub Action pins `RUSTUP_TOOLCHAIN`, so a
  rust-toolchain.toml in the consumer's checkout can no longer hijack the
  launchbound install or the gate; the toolchain contract (the gate builds
  your kernel under the action's toolchain) is documented. A truncated or
  empty gate record now surfaces the prune failure instead of a parser
  traceback.
- The lockstep pins are written once per workflow (env), the pin-watch
  issue names every site a bump must touch, CONTRIBUTING gains the release
  checklist (including the easy-to-forget floating-v1 move), and the
  action README references the floating tag.
- rustdoc warnings and leftover internal section references cleaned up.

### Changed

- reconverge (cargo-reconverge + reconverge-driver) installs from its
  crates.io releases instead of the git tag, in both the action and
  prune.yml; the action input renames to `reconverge-version`.

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
