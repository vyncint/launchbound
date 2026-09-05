#!/usr/bin/env bash
# CI gate: the six recorded pin sites agree with each other.
#
# The policy is "every site, or not at all" (CONTRIBUTING §9), and 2.0.0 did
# four of six: it moved the gate to reconverge 0.4.0 in `action.yml`,
# `action/README.md`, `prune.yml` and `docs/LIMITATIONS.md`, and left
# `rust-toolchain.toml`, `CONTRIBUTING.md` and `pins.yml` recording 0.1.11.
#
# The cost was not cosmetic. `pins.yml` measures upstream drift against its
# own `RECONVERGE_PIN`, so with a stale baseline its weekly signal reports
# movement away from a version nothing installs — which is why #17 sat open
# describing a pin the gate had not used for a release. A watcher whose
# baseline is wrong is worse than no watcher: it produces noise that looks
# like a finding.
#
# This asks nothing of the network, so it runs in the ordinary CI job rather
# than in the dispatch-only watch.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

status=0

fail() {
  echo "PIN DISAGREEMENT: $1" >&2
  status=1
}

# --- reconverge. Recorded with a leading `v` in two places and without it in
# four, so compare the bare version.
declare -A RECONVERGE=(
  ["rust-toolchain.toml"]="$(sed -n 's/^#   reconverge  v\([0-9][0-9.]*\).*/\1/p' rust-toolchain.toml)"
  ["CONTRIBUTING.md"]="$(sed -n 's/.*reconverge \([0-9][0-9.]*\) (installed from crates.io).*/\1/p' CONTRIBUTING.md)"
  [".github/workflows/pins.yml"]="$(sed -n 's/^  RECONVERGE_PIN: v\([0-9][0-9.]*\).*/\1/p' .github/workflows/pins.yml)"
  [".github/workflows/prune.yml"]="$(sed -n 's/^  RECONVERGE_VERSION: "\([0-9][0-9.]*\)".*/\1/p' .github/workflows/prune.yml)"
  ["action/action.yml"]="$(sed -n '/^  reconverge-version:/,/^  [a-z]/s/^    default: "\([0-9][0-9.]*\)".*/\1/p' action/action.yml)"
  ["action/README.md"]="$(sed -n 's/^| `reconverge-version` | `\([0-9][0-9.]*\)`.*/\1/p' action/README.md)"
  ["docs/LIMITATIONS.md"]="$(sed -n 's/.*`reconverge` (v\([0-9][0-9.]*\)).*/\1/p' docs/LIMITATIONS.md)"
)

# --- cuda-oxide, recorded as a full SHA in three places.
declare -A CUDA_OXIDE=(
  ["rust-toolchain.toml"]="$(sed -n 's/^#   cuda-oxide  \([0-9a-f]\{40\}\).*/\1/p' rust-toolchain.toml)"
  [".github/workflows/pins.yml"]="$(sed -n 's/^  CUDA_OXIDE_PIN: \([0-9a-f]\{40\}\).*/\1/p' .github/workflows/pins.yml)"
  # The "cargo oxide is not installed" message quotes the pin at the reader
  # and tells them to check that commit out. A message that names a stale
  # commit is worse than the cargo help it replaced.
  ["crates/launchbound-build/src/compile.rs"]="$(sed -n 's/^pub const CUDA_OXIDE_PIN: &str = "\([0-9a-f]\{40\}\)".*/\1/p' crates/launchbound-build/src/compile.rs)"
)

# --- the nightly, which `rust-toolchain.toml` owns.
TOOLCHAIN=$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)

agree() {
  local -n table=$1
  local label=$2 expected="" site ok=1
  for site in "${!table[@]}"; do
    local value="${table[$site]}"
    if [ -z "$value" ]; then
      fail "$label: could not read a pin from $site (has its shape changed?)"
      ok=0
      continue
    fi
    if [ -z "$expected" ]; then
      expected="$value"
    elif [ "$value" != "$expected" ]; then
      fail "$label: $site records $value, but another site records $expected"
      ok=0
    fi
  done
  # Only claim agreement when there was some: a summary line contradicting
  # the failure printed above it is the exact shape of bug this gate exists
  # to catch, and printing one here would be embarrassing.
  if [ "$ok" -eq 1 ] && [ -n "$expected" ]; then
    echo "  $label $expected (${#table[@]} sites agree)"
  fi
}

echo "lockstep pin set:"
agree RECONVERGE reconverge
agree CUDA_OXIDE cuda-oxide
echo "  nightly $TOOLCHAIN"

# The nightly is also named in the action, which installs it.
action_toolchain=$(sed -n '/^  toolchain:/,/^  [a-z]/s/^    default: "\(nightly-[0-9-]*\)".*/\1/p' action/action.yml)
if [ -n "$action_toolchain" ] && [ "$action_toolchain" != "$TOOLCHAIN" ]; then
  fail "nightly: action/action.yml records $action_toolchain, rust-toolchain.toml records $TOOLCHAIN"
fi

if [ "$status" -eq 0 ]; then
  echo "every recorded pin site agrees"
fi
exit "$status"
