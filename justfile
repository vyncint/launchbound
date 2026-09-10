# All recipes run under the pinned toolchain in rust-toolchain.toml.

default: ci

# The full local gate. Never push a commit that fails this. There is no
# "all required jobs green" aggregator job in ci.yml — this recipe is the
# aggregator, and the `ci` job runs it verbatim on both OSes — so a new gate
# becomes required by being listed here.
ci: fmt-check clippy test docs deny schemas pins versions links skill

# Cargo errors on a memberless virtual workspace, so the cargo recipes no-op
# until the first crate lands in S1. `grep -c` prints 1 when packages is empty.
_empty := `cargo metadata --format-version 1 --no-deps | grep -c '"packages":\[\]' || true`

fmt:
    @[ "{{ _empty }}" = "1" ] && echo "fmt: skipped, workspace empty until S1" || cargo fmt --all

fmt-check:
    @[ "{{ _empty }}" = "1" ] && echo "fmt-check: skipped, workspace empty until S1" || cargo fmt --all --check

clippy:
    @[ "{{ _empty }}" = "1" ] && echo "clippy: skipped, workspace empty until S1" || cargo clippy --workspace --all-targets -- -D warnings

test:
    @[ "{{ _empty }}" = "1" ] && echo "test: skipped, workspace empty until S1" || cargo test --workspace

# Rustdoc with warnings denied, and `missing_docs` on in every library
# crate. It cost nothing to turn on -- `cargo doc` was already at zero
# warnings -- and it caught two broken intra-doc links in the very commit
# that added it, one of them pointing at a function whose name I had
# misremembered.
docs:
    @[ "{{ _empty }}" = "1" ] && echo "docs: skipped, workspace empty until S1" || RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

deny:
    @[ "{{ _empty }}" = "1" ] && echo "deny: skipped, workspace empty until S1" || cargo deny check

# MSRV check; CI runs this with RUSTUP_TOOLCHAIN pinned to the MSRV.
msrv:
    @[ "{{ _empty }}" = "1" ] && echo "msrv: skipped, workspace empty until S1" || cargo check --workspace --locked

# The gate tests — the most important tests in the repo. They need
# cargo-reconverge (LAUNCHBOUND_RECONVERGE or PATH) and the sibling
# cuda-oxide checkout; prune.yml provisions both in CI.
gate:
    cargo test -p launchbound-prune --test gate -- --ignored

# The safety gate over the whole corpus — reconverge only, NO GPU.
prune cc="8.6":
    cargo run -q -p launchbound-cli -- prune --cc {{ cc }}

# The recorded pin sites agree with each other. No network: the
# dispatch-only `pins.yml` asks upstream, this asks ourselves — and a
# watcher whose own baseline is stale reports drift from a version nothing
# installs, which is how #17 came to describe a pin two releases old.
pins:
    ./scripts/check-pins.sh

# The workspace version, every crate's version, and the internal `version =`
# pins in [workspace.dependencies] agree. The pins sat at 2.0.0 through the
# whole 2.1.0 line: harmless for a path build, wrong as a record, and fatal
# to the next major bump (`cargo metadata` refuses to resolve).
versions:
    ./scripts/check-versions.sh

# Every link in the README is absolute, names the right GitHub path kind,
# and points at something that exists. All eleven crates ship this file as
# their readme, and crates.io rewrites a relative link against the crate's
# directory -- so all eight were 404 on all eleven published 2.2.0 pages,
# and nothing here could see it.
links:
    ./scripts/check-readme-links.sh

# Golden + JSON Schema validation of report documents (S4).
schemas:
    cargo test -p launchbound-report --test schema_and_golden

# The vendored termlens skill names the version we actually depend on.
# AGENTS.md makes that copy normative for PTY tests, so a stale one is a
# wrong contract, not a stale doc — and the staleness is silent.
skill:
    ./.github/scripts/check-skill-version.sh

# The termlens-cli suite. `#[ignore]`d so a plain `cargo test` never
# `cargo install`s a binary behind a contributor's back (launchbound-tui is
# published); CI asks for it by name.
termlens-cli:
    cargo test -p launchbound-tui --test cli -- --ignored
