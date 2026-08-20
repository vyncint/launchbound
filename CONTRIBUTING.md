# Contributing to launchbound

Thanks for your interest. Issues and small PRs are welcome; large features
are better discussed first.

## Dev setup

The default feature set builds and tests **without any GPU, CUDA SDK, or
Metal** — that is a hard project rule, and CI enforces it on plain GitHub
runners.

```bash
rustup show                # installs the pinned nightly from rust-toolchain.toml
cargo install just         # or: brew install just
just ci                    # fmt, clippy -D warnings, tests, cargo-deny, schemas
```

`cargo reconverge check` also needs no GPU: it runs over Stable MIR as a
wrapped `cargo build`. Everything except an actual benchmark can be developed
on a laptop.

## The pin policy

Three pins move together or not at all: the nightly in
`rust-toolchain.toml`, the `cuda-oxide` pin, and the `reconverge` pin.
`reconverge` is a rustc-driver tool — it must be built by the exact rustc it
wraps — and `cuda-oxide` requires the same pin. A bump is its own commit,
never mixed with a behaviour change, and re-runs the affected stage gates.
The scheduled `pins.yml` workflow reports upstream movement by opening an
issue; it never bumps anything. Current pins: nightly-2026-04-03,
cuda-oxide 50d07314, reconverge 0.1.11 (installed from crates.io).

## Commit requirements

Every commit needs all three of:

1. **DCO sign-off** — `git commit -s`, with a `Signed-off-by:` trailer
   matching the author.
2. **Cryptographic signature** — `git commit -S` (SSH or GPG signing both
   work; the signature is what "verified" means on GitHub).
3. **Conventional Commits** — `feat:`, `fix:`, `docs:`, `test:`, `ci:`,
   `chore:`, `refactor:`, `perf:`, plus `bench:` for a change that alters
   measured timings. Scope optional, e.g. `feat(prune): …`.

Treat `git commit -sS` as the only spelling. Fix a missed sign-off with
`git commit --amend -s --no-edit`, or a branch with
`git rebase --signoff main`.

## AI tooling policy

You may use whatever tools you like to write your contribution. What lands in
this repository carries **no AI attribution of any kind**: no
`Co-Authored-By` naming an AI, bot or agent; no "Generated with …" lines; no
robot emoji or watermarks in commits, PRs, comments or docs; no `*[bot]` or
vendor noreply authors. You sign off on your commits as your own work — that
is what the DCO trailer means. CI (`no-ai-attribution.yml`) enforces this.

## Testing policy

- The default feature set must pass `just ci` with no GPU present.
- Hardware-dependent tests live behind the `hardware` feature and
  `#[ignore]`, and never run in CI.
- Every measured number that reaches a report, a doc or a commit message must
  come from a re-runnable command, and GPU-measured numbers carry a
  `.gpu-evidence` log. Never round in a favourable direction.
- A model-derived number is labelled `estimated` everywhere. Reporting an
  estimate as a measurement is a release-blocking defect.

## Releasing

1. Bump the workspace version (Cargo.toml, one place) and CHANGELOG.md via
   PR; CI must be green.
2. Tag the merge commit `vX.Y.Z` (signed) and push it. The release
   workflow refuses to publish if the tag disagrees with Cargo.toml, and
   publishes via crates.io Trusted Publishing (no token anywhere).
3. Move the floating action tag: `git tag -f -s v1 && git push -f origin
   v1`. **This step is manual and easy to forget** — `@v1` consumers keep
   running the old action until it happens. Only move it to a commit that
   is green on main.

## License

By contributing, you agree that your contributions will be dual-licensed
under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) without additional
terms, and you certify the [Developer Certificate of
Origin](https://developercertificate.org/) via your sign-off.
