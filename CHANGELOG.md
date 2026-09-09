# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Entries that
change measured timings are marked `bench:`.

## [Unreleased]

## [2.2.0] - 2026-09-09

The cuda-oxide pin was 133 commits and one toolchain behind, and the watch
that would have said so had its schedule commented out. 2.2.0 catches up,
turns the watch back on, widens the device model past Ampere, and closes the
panic and arithmetic gaps user input could reach — plus the semver and
documentation gates a published 2.x line should have had from the start.

Ten issues (#50–#59). **Four of them were wrong when filed**, and measuring
before implementing caught each one; the corrections are recorded on the
issues and repeated below, because an audit that miscounts is worth less
than no audit and the shape of the miscount is the useful part.

### Changed

- **The lockstep pins move to the current upstream**, at every recorded
  site: `nightly-2026-04-03` → **`nightly-2026-08-28`**, cuda-oxide
  `50d07314` → **`26754ae5`** (upstream `main` on the day), reconverge
  `0.5.0` → **`0.6.0`**. The prune leg never needed this — it is analysis
  only, which is why the staleness stayed invisible — but the compile leg
  shells out to `cargo oxide inspect` from the checkout the pin names, so a
  kernel written against a current `cuda-device` could not be built by the
  gate at all.

  Verified rather than assumed: the corpus ran under both pin sets and the
  outputs were diffed **byte for byte** — 93 clean, 8 refused, the same
  eight `REFUSED RC001` lines on `reduce-flip` above one warp, the same
  candidate hashes. All six corpus kernels lower to PTX at the new pin
  (`.target sm_80`, one `.visible .entry` each). Recorded in
  `docs/research-baseline.md` in the shape the 0.1.11 → 0.3.0 comparison
  established. (#50)

- **The README toolchain badge is a checked pin site.** It was not one, and
  it had drifted to a nightly nothing installed — the first pin a reader
  sees. `check-pins.sh` reports three nightly sites now and fails on badge
  drift, which was verified by drifting it on purpose. (#50)

- **The device model reaches Blackwell.** `DEVICES` held two rows, A10G
  (8.6) and T4 (7.5), so `--cc 9.0` on a Hopper part — the architecture a
  reader arriving from cuda-oxide is most likely to be holding — was a model
  error. Added 8.0 (A100), 8.9 (L4/L40), 9.0 (H100) and 10.0 (B200).

  Every field but `sm_count` is a compute-capability fact from the CUDA C++
  Programming Guide; `sm_count` is a product fact and each row names the
  part it came from. The new rows' shared-memory figures were cross-checked
  against reconverge 0.6.0's independent `cc.rs` table and agree exactly,
  allowing for the 1 KiB the driver reserves per SM on Ampere and later. An
  unknown capability now lists the known ones, sorted numerically — `"10.0"`
  precedes `"8.6"` as a string, which is the trap. (#52)

- **`launchbound_bench::Summary` is re-exported from `launchbound-report`.**
  It appears in the public fields of `CandidateReport`, `ChosenInfo` and
  `RejectedFaster`, so it was reachable and unnameable unless you also
  depended on the bench crate. (#55)

- **`launchbound-space` refuses an impossible block at load.** A block axis
  above CUDA's per-axis limit (x, y: 1024; z: 64) is now rejected by
  `KernelSpec` with a message naming the value and the limit, so an operator
  hears about a typo when the spec loads rather than as a launch failure ten
  minutes into a sweep. (#53)

- **The PTY test harness moves to termlens 0.10.1** (from 0.9). The upgrade
  itself is small — `drag` takes four column-first arguments now instead of
  two coordinate pairs — but it brings the accessor this suite was missing.

- **`tests/emulation.rs` pins what the emulator drops.** Every screen
  assertion in this crate reads a grid a VT emulator produced from the
  binary's bytes: five golden files, two border scans and a styled banner. A
  sequence that emulator does not implement makes the grid quietly wrong and
  every one of those assertions a claim about a plausible-looking fiction.
  0.10 made it checkable, and the answer is one `SGR 59` — underline colour,
  which changes no cell. Pinned exactly, so anything joining it has to be
  read before the suite is trusted again.

- **`tests/cli.rs` drives `termlens-cli`** against this crate's own saved
  screens: rendered, diffed with its 0/1/2 exit codes, and `inspect` pointed
  at the real binary. The tool is resolved at the version the lockfile names,
  so it and the library are one release.

- **The vendored agent skill (`.claude/skills/termlens/SKILL.md`) is
  refreshed to 0.10.1**, and `check-skill-version.sh` now fails when that copy
  and the dependency disagree on major.minor. It had drifted two releases
  behind with nothing to notice.

- **CI writes `TERMLENS_ARTIFACT_DIR` and renders failures into the job
  summary** via termlens's `report` action, so a red PTY test arrives as a
  picture rather than a grid in a log.

### Fixed

- **Six `partial_cmp(..).expect("no NaN")` sorts became `f64::total_cmp`.**
  The issue said five; there are seven `partial_cmp` sites and six carried
  an `expect`, because the audit grepped the *message string* rather than
  the construct and the two with different message text fell out of the
  count. Those two were the ones that mattered: `bench/stats.rs` sorts the
  **raw measured timings**, and `model/lib.rs` sits under **`spearman`, a
  `pub fn`** — so `spearman(&xs, &ys)` with a single NaN in either column
  aborted the process from safe code, with no unsafe input path required.
  That is a reachable panic, not a latent one.

  `total_cmp` is total, free, and identical to `partial_cmp` on every pair
  of non-NaN floats. NaN ordering is documented where it changes an outcome.
  In `summarize` it has a consequence worth stating: a NaN fails both Tukey
  fence comparisons and is **rejected as an outlier**; poison the quantiles
  badly enough and the answer is `None`, the honest summary of a sample that
  has none. (#54)

- **`Config::block_threads()` saturates instead of overflowing.** It was
  `.product()` over three `u64`s read straight from `kernel.toml`: a panic
  in a debug build, a wrapped value in release — and a wrapped value does
  not stop there, it feeds `estimate` and the gate's `threads > WARP_SIZE`
  test, so a block of 2^64 + 32 threads reads as a legal 32-thread block.

  The issue proposed `checked_mul` folding to `Result`, which is a break to
  a `pub fn` on a crate published at 2.1.0 and could not ship in a minor
  bump. It was also unnecessary, and the issue said why without noticing:
  `grid_blocks` is the sibling doing the same job with `saturating_mul`.
  A proptest over three arbitrary `u64` axes holds the line. (#53)

- **The summary invariant lives in the type.** `measured_fastest_first`
  filtered on `summary.is_some()` and returned bare candidates, so both
  callers re-`unwrap()`ed on the strength of a filter performed in another
  function. It returns `(&CandidateReport, &Summary)` pairs; the four
  unwraps are gone because they no longer compile. Same shape in
  `report::build`. Remaining `expect`s in functions that already return
  `Result` became `?`. (#55)

### Added

- **A semver gate, with the release type forced.** No workflow ran
  `cargo-semver-checks`, so removing a `pub fn` shipped without a signal.
  The gate forces `--release-type patch` and the reason is measured, not
  assumed: remove `Config::kernel` while bumping the manifest to 3.0.0 in
  the same PR and the *inferred* run prints `0 checks: 0 pass, 254 skip` and
  exits **0**, while forced `patch` prints `1 major and 0 minor checks
  failed` and exits **100**. cargo-semver-checks runs only the lints the
  declared bump does not already excuse, so a PR that declares its own break
  silences the gate. `patch` excuses nothing — and does not forbid
  additions, since only breaking changes are reported.

  A deliberate break carries the new `breaking` label, which switches the
  job to `major`. Three of the eleven published crates are binary-only; the
  eight with a library API are all covered. (#56)

- **`scripts/check-versions.sh`, in `just ci`.** `[workspace.dependencies]`
  pinned the path crates at 2.0.0 while the workspace was 2.1.0 — harmless
  for a path build, which is why it survived a whole minor line. Not
  harmless for the next major: with the pins stale, bumping to 3.0.0 makes
  `cargo metadata` refuse to resolve the workspace at all. **The drift
  breaks the release after next.** (#56)

- **A rustdoc gate, and 237 documented public items.** The issue said "about
  42". It is 237 — I counted items and never counted *fields*, and 144 of
  the 237 are public struct fields. In a crate family whose structs are the
  JSON wire formats, the fields are the part a consumer actually reads.

  The gate cost nothing to turn on (`cargo doc` was already at zero
  warnings) and earned its keep immediately by failing on two broken
  intra-doc links in the commit that added it — one naming a function that
  does not exist. (#57)

- **The pin watch runs weekly again, and reports a toolchain move on its own
  line.** The schedule had been commented out, so it spoke only when
  dispatched. The two kinds of drift are not alike: commit churn is a bump
  you can schedule, a channel move means our rustc can no longer build
  upstream's kernels and reconverge must be rebuilt. An extractor that reads
  nothing now fails the job instead of reporting "Pins current." forever.
  Exercised against a stubbed upstream across five scenarios. (#51)

- **The TUI's freedom from colour is now an invariant.** #58 expected a
  colour-only cue to vanish under `NO_COLOR`; measuring found there is no
  such cue — every style is `BOLD` or `BOLD | REVERSED` and `Color::`
  appears nowhere. So the tests assert the stronger claim: every cell of all
  four views has the terminal's default foreground and background, the metal
  banner keeps its emphasis under `NO_COLOR=1`, and a `NO_COLOR` frame is
  byte-identical to one without it. The first fires the day someone marks a
  refusal red. (#58)

  (The issue's other half — "the suite runs at one geometry" — was already
  false when filed: 2.1.0's suite runs nine tests at 80×24 and two at 60×30.)

### Documentation

- **`#[launch_bounds]` and register budgeting are not validated**, and the
  README and LIMITATIONS now say so and link to each other. launchbound
  never reads a register count and does not check `#[launch_contract]`
  against grid limits. The confusion is earned: `stencil-1d/kernel.toml`
  opens by narrating register pressure, `lb_max` is a real tuning dimension,
  and the tool is called *launchbound*. The one `.maxntid` relationship the
  corpus enforces holds because the kernel author wrote it as a constraint —
  launchbound attaches no meaning to `lb_max`. (#59)

- **The model's device table is narrower than the gate's**, recorded in
  LIMITATIONS: reconverge covers 7.0–12.0, so `prune --cc 12.0` can succeed
  where `tune --backend model --cc 12.0` refuses. Only 8.6 and 7.5 have ever
  had a kernel measured on them here; the other four rows are documented
  capacity, not experience.

- **Four false claims found by a deep read before the release.**
  `--allow-unsafe` was documented as if it were on `tune`; it is on `stage`,
  and `tune --allow-unsafe` is an "unexpected argument" error. The README's
  CLI block omitted `stage` entirely. `just ci` was described with a
  two-release-stale recipe list in three separate places. And
  research-baseline said "the gate now pins 0.3.0" in the present tense.

- `docs/RELEASING.md` gains the internal pin bump in step 1 and the
  post-publish `baseline-version` move as its own step.

## [2.1.0] - 2026-09-05

Thirteen findings, all reported against 2.0.0 with a measured reproduction.
Most of them reduce to one shape: something was consumed on a contract other
than the one it is written to, and the mismatch was reported as a fault of
whatever it was pointed at.

### Changed

- **The lockstep pins move to reconverge 0.5.0**, at every recorded site.
  2.0.0 moved the gate to 0.4.0 and updated four of the six the policy
  names, so `rust-toolchain.toml`, `CONTRIBUTING.md` and `pins.yml` still
  recorded 0.1.11 — and `pins.yml` measures upstream drift against its own
  `RECONVERGE_PIN`, so its weekly signal reported movement away from a
  version nothing installs. That is why #17 sat open describing a pin two
  releases old. `just pins` now asserts the sites agree with each other,
  with no network, before anything asks upstream. Reported in #46 and #17.

- **`apply --verify` is a real switch.** It was a `bool` with
  `default_value_t = true`, which clap gives a `SetTrue` action — so
  `--verify` set what was already set, `--no-verify` was an unknown
  argument, and the help read as an opt-in for something mandatory. There
  is a `--no-verify` now, and the help says verification is on. This is
  what makes `apply` usable on a machine that has the run directory but not
  the analyzer and the pinned toolchain. Reported in #36.

- **`prune`'s verdict line says what it checked.** The gate answers
  convergence and static shared-memory capacity at a `--cc`; it has no view
  of instruction availability, so a crate using an `sm_80+` intrinsic under
  `needs_cc = "7.5"` prunes to `12 clean` at `--cc 7.5` and fails only when
  something finally lowers it for that part. `needs_cc` is the author's
  claim and is taken on trust — defensible, and nowhere stated, so "clean"
  read as "this kernel is fine at cc 7.5". docs/LIMITATIONS.md carries the
  long form, including why the two stronger fixes were not built. Reported
  in #32.

### Fixed

- **A kernel crate with a bin target no longer hard-stops the gate.**
  reconverge prints one `findings.v1` document per analyzed target — its
  documented contract — and this reader handed the whole of stdout to a
  single `from_str`, so a `src/main.rs` beside a kernel library, the
  ordinary shape of a GPU crate, made every candidate a tool error at
  `trailing characters at line 2 column 1`. In the Action `fail-on`
  defaults to `tool-error`, so CI went red for a crate with nothing wrong
  with it, pointing at the analyzer's tracker. Stdout is read as JSONL and
  the findings are unioned: a deny finding in any target refuses, and the
  bin target's document is harmless to merge. The fail-safe always held —
  it held against a format the analyzer documents. Reported in #42.

- **The scratch copy is the whole crate.** It took only the entries of
  `src/` that are files, so `mod util;` with `src/util/mod.rs` — how Rust
  code is organised past one file — produced a scratch crate that could not
  compile, and the gate reported `error: could not compile` against a crate
  whose own `cargo check` is clean. The message told its author to fix
  build errors they do not have, or to reinstall their toolchain, and never
  said that what it compiled was not their crate. It copies recursively
  now, carries `build.rs` or whatever `package.build` names, skips
  `target/`, and a tool error names the scratch directory. Reported in #43.

- **rustc's diagnostics survive the tool-error filter.** #19 replaced a
  tail-six heuristic with a filter on marked lines, and picked the
  *secondary* marker: rustc's primary diagnostics begin `error[E0583]:`,
  a code before the colon. So what survived was cargo's summary and
  reconverge's generic hint — "see the errors above", with the one that was
  above removed. Both forms are accepted now, here and in the compile
  executor, which still had the tail heuristic #19 removed next door.
  Reported in #44.

- **`--budget` cannot be given a value that fails to bound anything.**
  `split_at(text.len() - 1)` on a trimmed-empty argument is `0usize - 1`,
  so `--budget ""` panicked at exit 101. Worse: `NaN` and `1e400` parsed,
  and the guard is `elapsed >= budget` — false for every value against NaN,
  never true against infinity — so a value that looked accepted produced an
  **unbounded** measured sweep on real silicon, which is the one failure a
  budget exists to prevent. Non-finite and negative values are rejected,
  the message names the flag and the accepted forms, and `min`/`hr` are
  accepted alongside `m`/`h` because that is what people type. `--budget 0`
  stays valid and means what it always did. Reported in #33.

- **`apply` decides about verification before it prints anything.** It
  emitted the `params.rs` and *then* verified, so "refusing to emit"
  arrived after the emission and a reader who had piped stdout to a file
  had the file. On a Metal run it could never succeed at all: that path has
  no convergence gate, deliberately, so the run records `gate_cc: "metal"`,
  and handing that sentinel to reconverge produced the correct answer to
  the wrong question ("`metal` is not a compute capability") dressed as a
  regression ("no longer passes the gate"). Nothing regressed; the gate
  never ran and cannot. It refuses by name now, before stdout, and points
  at `--no-verify` or `prune --cc <target>`. `Verdict` has a `Display`, so
  no user-facing message is a Rust struct literal. Reported in #34.

- **A `results.json` the report cannot read is an error, not "unmeasured".**
  A truncated file, an empty one, `null`, `[]`, a `results.v2` from a newer
  runner and a *directory* named `results.json` all rendered as "nothing
  measured yet": exit 0, nothing on stderr, and a JSON report that
  validated against the schema. The run directory is the hand-off between
  two machines and those two conditions call for opposite actions — wait,
  or go and look. Only `NotFound` is `Ok(None)` now; everything else names
  the path and the cause, the way `verdicts.v1` already did fifteen lines
  away in the same function. `model --results` names the cause too, and
  `tune`'s end-of-run report inherits all of it. Reported in #45.

- **Nothing is cut mid-value or mid-word at the panel border.** #24 fixed
  this on the chosen line and left it in the two views below: the ranking
  lost every closing bracket at eighty columns, so each interval read as a
  number with no upper bound, and the rejections view — the one the README
  calls the point of the tool — lost the clause that says what to do,
  stopping at `splits a 64-threa` and never reaching `safe only at one warp
  (<= 32 threads)`. An interval is a field and is dropped whole through the
  same helper the chosen line uses; a reason is a sentence and wraps. A
  scan over every golden now fails on a value or a word ending at the
  border without an ellipsis — confirmed red against the shipped pre-fix
  frames, and it would have caught #24. Reported in #35.

- **A missing `cargo oxide` is diagnosed by name.** cargo's own help
  relayed `cargo search cargo-oxide`, and cargo-oxide is not on crates.io:
  cuda-oxide is a pinned git checkout. The one actionable-looking line sent
  the reader to a package that does not exist, on the first wall of `stage`
  and `tune --backend cuda`. The message now names the pin, gives the three
  commands CI uses, and says that `prune` needs none of it. The pin is a
  constant `just pins` checks, so a bump moves the message with it. In the
  same path, `exit Some(101)` is an exit code again and the compile failure
  reports the compiler's errors rather than its summary. Reported in #37.

- **No flag on `tune` is accepted and silently ignored.** `--budget`,
  `--order` and `--seed` are inert with `--backend model`, and `--seed` is
  inert with `--order exhaustive` on any backend. Each says so once, the
  way `--out` has since #22 — somebody who passes `--budget 30m` reasonably
  believes something is bounded. Reported in #38.

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

### Migrating from 1.x

- **The Action's floating tag is now `@v2`.** `uses:
  vyncint/launchbound/action@v1` keeps working and stays on 1.2.0, which is
  the point of a floating major tag — but it stays on reconverge 0.3.0 with
  it, and 0.3.0 is the analyzer that cannot see a named-const shared-memory
  size. Move to `@v2` to get the gate this release is about.
- **Add `--cc` to any `launchbound tune` invocation.** It has no default now.

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
