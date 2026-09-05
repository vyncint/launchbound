#!/usr/bin/env bash
# CI gate: the recorded pin sites agree with each other.
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
#
# **Portability: no `declare -A`, no `local -n`.** macOS ships bash 3.2,
# where an associative array is silently an indexed one — the first version
# of this script passed on Linux and died on macOS with
# `rust: unbound variable`. That is the same defect as the GNU-only `sed -i`
# in the sibling repository's gate scripts, in a gate written to prevent
# exactly this class of drift.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

status=0

# Prints; the caller decides the exit status. `agree` runs in a pipeline and
# therefore in a subshell, so a `status=1` set inside it would be discarded
# — which would print the failure and exit 0, the precise shape of bug this
# gate exists to catch.
fail() {
  echo "PIN DISAGREEMENT: $1" >&2
}

# One `<file><TAB><value>` line per site. A tab separates them because a path
# cannot contain one and a version certainly cannot.
site() {
  printf '%s\t%s\n' "$1" "$2"
}

# reconverge. Two sites spell it with a leading `v` and the rest without, so
# the bare version is what is compared.
reconverge_sites() {
  site "rust-toolchain.toml" \
    "$(sed -n 's/^#   reconverge  v\([0-9][0-9.]*\).*/\1/p' rust-toolchain.toml)"
  site "CONTRIBUTING.md" \
    "$(sed -n 's/.*reconverge \([0-9][0-9.]*\) (installed from crates.io).*/\1/p' CONTRIBUTING.md)"
  site ".github/workflows/pins.yml" \
    "$(sed -n 's/^  RECONVERGE_PIN: v\([0-9][0-9.]*\).*/\1/p' .github/workflows/pins.yml)"
  site ".github/workflows/prune.yml" \
    "$(sed -n 's/^  RECONVERGE_VERSION: "\([0-9][0-9.]*\)".*/\1/p' .github/workflows/prune.yml)"
  site "action/action.yml" \
    "$(sed -n '/^  reconverge-version:/,/^  [a-z]/s/^    default: "\{0,1\}\([0-9][0-9.]*\)"\{0,1\}.*/\1/p' action/action.yml)"
  site "action/README.md" \
    "$(sed -n 's/^| `reconverge-version` | `\([0-9][0-9.]*\)`.*/\1/p' action/README.md)"
  site "docs/LIMITATIONS.md" \
    "$(sed -n 's/.*`reconverge` (v\([0-9][0-9.]*\)).*/\1/p' docs/LIMITATIONS.md)"
}

# cuda-oxide, recorded as a full SHA. The third site is the "cargo oxide is
# not installed" message, which quotes the pin at the reader and tells them
# to check that commit out — a message naming a stale commit is worse than
# the cargo help it replaced.
cuda_oxide_sites() {
  site "rust-toolchain.toml" \
    "$(sed -n 's/^#   cuda-oxide  \([0-9a-f]\{40\}\).*/\1/p' rust-toolchain.toml)"
  site ".github/workflows/pins.yml" \
    "$(sed -n 's/^  CUDA_OXIDE_PIN: \([0-9a-f]\{40\}\).*/\1/p' .github/workflows/pins.yml)"
  site "crates/launchbound-build/src/compile.rs" \
    "$(sed -n 's/^pub const CUDA_OXIDE_PIN: &str = "\([0-9a-f]\{40\}\)".*/\1/p' crates/launchbound-build/src/compile.rs)"
}

# The nightly, which `rust-toolchain.toml` owns and the action installs.
toolchain_sites() {
  site "rust-toolchain.toml" "$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)"
  # The action quotes some defaults and not others, so the quotes are
  # optional here. Requiring them made this site unreadable, and the first
  # version of this script treated "unreadable" as "nothing to check" — so
  # the nightly was never actually compared. An extractor that silently
  # matches nothing is the same failure as a stale pin, one level up.
  site "action/action.yml" \
    "$(sed -n '/^  toolchain:/,/^  [a-z]/s/^    default: "\{0,1\}\(nightly-[0-9-]*\)"\{0,1\}.*/\1/p' action/action.yml)"
}

# Read `<file><TAB><value>` lines on stdin; every value must match.
agree() {
  label=$1
  expected=""
  count=0
  ok=1
  tab=$(printf '\t')
  while IFS="$tab" read -r site_name value; do
    [ -z "$site_name" ] && continue
    count=$((count + 1))
    if [ -z "$value" ]; then
      fail "$label: could not read a pin from $site_name (has its shape changed?)"
      ok=0
      continue
    fi
    if [ -z "$expected" ]; then
      expected="$value"
    elif [ "$value" != "$expected" ]; then
      fail "$label: $site_name records $value, but another site records $expected"
      ok=0
    fi
  done
  if [ "$count" -eq 0 ]; then
    fail "$label: no sites were read at all"
    return 1
  fi
  # Only claim agreement when there was some: a summary line contradicting
  # the failure printed above it is the exact shape of bug this gate exists
  # to catch, and printing one here would be embarrassing.
  if [ "$ok" -eq 1 ]; then
    echo "  $label $expected ($count sites agree)"
    return 0
  fi
  return 1
}

echo "lockstep pin set:"
# A pipeline's status is its last command's, which is `agree`.
reconverge_sites | agree reconverge || status=1
cuda_oxide_sites | agree cuda-oxide || status=1
toolchain_sites | agree nightly || status=1

if [ "$status" -eq 0 ]; then
  echo "every recorded pin site agrees"
fi
exit "$status"
