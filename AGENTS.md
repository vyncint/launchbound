# Working on launchbound

Instructions for coding agents — and useful to humans. **launchbound** is a convergence-safe autotuner for Rust GPU kernels: it searches a kernel's launch and specialization space, finds the fastest configuration, and never returns one that is convergence-unsafe.

This file is the canonical brief; `CLAUDE.md` points here. `CONTRIBUTING.md`
is the full contributor document and wins wherever the two disagree.

## Layout

- `crates/launchbound-*` — one crate per stage: `space` (enumerate), `prune`
  (the safety gate), `build`, `bench`, `search`, `model`, `metal`, `report`,
  `tui`, `runner`, `cli`.
- `corpus/` — standalone kernel fixtures, deliberately **excluded** from the
  workspace: they path-depend on a sibling cuda-oxide checkout and are compiled
  by the prune tooling, never by `cargo test`.
- `docs/` — `ARCHITECTURE.md`, `SAFETY.md`, and `LIMITATIONS.md`. **Read
  LIMITATIONS before trusting a result**, and before claiming one.

## Build and test

```sh
just ci                       # fmt, clippy, test, deny, schemas
cargo test --workspace        # no GPU, no network, no checkout needed
just gate                     # the gate tests — needs cargo-reconverge + cuda-oxide
```

Pinned nightly for the analysis and compile paths (`rust-toolchain.toml`);
MSRV 1.88 for everything that does not need it.

## Things that will bite you here

- **No GPU is required for the part that matters.** `prune` is the safety
  gate and runs on any laptop; that is why it is its own verb. Do not write a
  test that needs silicon when the gate does not.
- **Goldens:** regenerate with `LAUNCHBOUND_BLESS=1 cargo test -p launchbound-tui
  --test tui`, then read every diff.
- **A model-derived ranking is never presented as a measurement.** Anything
  estimated says so on every surface it reaches. This is the project's central
  honesty claim — do not blur it to make output tidier.
- **The gate job never caches the analyzer.** It installs the published
  `cargo-reconverge` from crates.io every run, because a cached binary is not
  evidence about what is published today.

## The rules that will fail CI

Three, and they are the same in every one of these repositories.

1. **Conventional Commits.** `feat:`, `fix:`, `docs:`, `test:`, `ci:`,
   `chore:`, `refactor:`, `perf:` — imperative mood, subject line under 72
   characters, scope optional (`fix(screen): …`).
2. **DCO sign-off.** `git commit -s`, and the `Signed-off-by:` email must
   match the commit author's. Forgot? `git commit --amend -s --no-edit`, or
   `git rebase --signoff main` for a branch.
3. **No AI attribution.** See below — this one is about you, and it is the
   rule most likely to catch an agent out.

Run them yourself before pushing; both scripts take a commit range:

```sh
.github/scripts/check-dco.sh main..HEAD
.github/scripts/check-no-ai-attribution.sh main..HEAD
```

## Using AI here

**You are welcome.** Every one of these projects was built with AI assistance
and says so in its CONTRIBUTING. Use whatever helps.

**You are not a contributor.** Do not add yourself to the history:

- no `Co-Authored-By:` trailer naming an assistant, a model, or a vendor,
- no "Generated with …" footer, no robot emoji,
- no bot account as author or committer.

The human who opens the pull request is the author of record and takes
responsibility for the change under the DCO. That is what the sign-off
certifies, and it cannot be certified by a tool. `.claude/settings.json`
turns co-author trailers off for agents that read it; the check in CI is the
boundary, and it reads every commit in the range.

If CI catches one, the fix is to rewrite the message, not to argue with it:

```sh
git commit --amend            # the last commit
git rebase -i main            # several, marking each `reword`
git push --force-with-lease
```

## What good work looks like here

These repositories share a house style, and it is stricter than most:

- **Evidence over assertion.** A bug report says what was measured against
  which released version. "Reproduced against 0.4.0" is the standard; "the
  code looks wrong" is not. Issues in these repos read *Today / Why it is
  worth fixing / Fix / Done when*, with a concrete reproduction.
- **Every change lands with a test**, and the test must be able to fail. If
  you add a guard, prove it catches the thing — break it once and watch it go
  red before you commit.
- **Comments say *why*, never *what*.** The diff shows what. A comment earns
  its place by recording the reason, the alternative rejected, or the failure
  that motivated the line.
- **Say what you did not do.** A pull request that lists what it left out and
  why is worth more than one that implies completeness. If something is
  unverified, say so — an honest gap is cheap and a false claim is expensive.
- **Documentation is checked, not maintained.** Where a README states a fact
  the code owns, there is usually a test asserting the two agree. Do not
  break that pattern by hand-editing the doc.

## Pull requests

Branch from `main` (`feat/…`, `fix/…`, `docs/…`, `ci/…`). PRs are
**squash-merged**, so the PR title becomes the commit subject on `main` —
write it as a Conventional Commit. Update `CHANGELOG.md` under
`[Unreleased]` for anything user-facing.

Direct pushes to `main` are blocked by a ruleset; everything goes through a
pull request, including releases.

## Releasing

Tag `vX.Y.Z` on `main`; `release.yml` publishes the crates in dependency order via Trusted Publishing. See `docs/RELEASING.md`.
