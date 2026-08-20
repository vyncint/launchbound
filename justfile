# All recipes run under the pinned toolchain in rust-toolchain.toml.

default: ci

# The full local gate. Never push a commit that fails this.
ci: fmt-check clippy test deny schemas

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

# Golden + JSON Schema validation of report documents (S4).
schemas:
    cargo test -p launchbound-report --test schema_and_golden
