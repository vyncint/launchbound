#!/usr/bin/env bash
# CI gate: the workspace version, every crate's version, and the internal
# `version =` pins in [workspace.dependencies] all agree.
#
# The pins are what a crates.io consumer resolves against. `launchbound-prune`
# 2.1.0 depending on `launchbound-space = "^2.0.0"` says it works with a
# release it was never built or tested against -- harmless for a path build,
# which is why it went unnoticed through the whole 2.1.0 line, and wrong as a
# record. It is not only cosmetic: a `3.0.0` workspace bump with the pins left
# at `2.0.0` fails `cargo metadata` outright ("candidate versions found which
# didn't match: 3.0.0"), so the drift breaks the next major release rather
# than the current one.
#
# Portability: macOS ships bash 3.2 -- no `declare -A`, no `local -n`, no
# GNU-only sed. (scripts/check-pins.sh, which learned this the hard way.)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

status=0

# `workspace.package.version` -- the one everything else must match. Anchored
# to the section so a dependency's `version =` cannot be read by mistake.
workspace_version=$(
  sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml | head -1
)
if [ -z "$workspace_version" ]; then
  echo "VERSION GATE: could not read workspace.package.version from Cargo.toml" >&2
  exit 1
fi
echo "workspace.package.version: $workspace_version"

# The internal path pins.
pins=$(sed -n 's/^\(launchbound-[a-z-]*\) = { path = "[^"]*", version = "\([^"]*\)".*/\1 \2/p' Cargo.toml)
if [ -z "$pins" ]; then
  echo "VERSION GATE: no internal version pins found -- has [workspace.dependencies] changed shape?" >&2
  exit 1
fi

count=0
while read -r name pin; do
  [ -z "$name" ] && continue
  count=$((count + 1))
  if [ "$pin" != "$workspace_version" ]; then
    echo "VERSION GATE: $name is pinned at $pin, workspace is $workspace_version" >&2
    status=1
  fi
done <<EOF
$pins
EOF
echo "  internal pins checked: $count"

# Every member crate inherits the workspace version rather than setting its
# own; a literal here would drift silently.
for manifest in crates/*/Cargo.toml; do
  own=$(sed -n '/^\[package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$manifest" | head -1)
  if [ -n "$own" ]; then
    echo "VERSION GATE: $manifest sets version = \"$own\" instead of version.workspace = true" >&2
    status=1
  fi
done

# The semver gate's baseline literal, which is the other version in this
# repository that nobody compiles and so nobody notices. `ci.yml` compares
# every pull request's public API against it, and the rule written beside it
# is that it moves in the release PR *after* the publish -- left behind, the
# gate measures against a release that is no longer the last one, and a break
# introduced in between is outside what it can see. 2.2.1 published on
# 2026-09-10 and the literal still said 2.2.0 twelve days later.
#
# The check is "equals the newest version on the index", which holds at both
# ends of a release: during the release PR the workspace is the unpublished
# X.Y.Z and the newest published is still the previous one, and after the
# publish this fails until the literal is moved -- which is the follow-up
# step docs/RELEASING.md already lists.
baseline=$(sed -n 's/^ *--baseline-version \([0-9][0-9.]*\).*/\1/p' .github/workflows/ci.yml | head -1)
if [ -z "$baseline" ]; then
  echo "VERSION GATE: could not read --baseline-version from .github/workflows/ci.yml" >&2
  status=1
else
  index=$(curl -fsS --max-time 20 https://index.crates.io/la/un/launchbound-cli 2>/dev/null || true)
  if [ -z "$index" ]; then
    # Offline is a normal way to run `just ci`; a gate that fails without a
    # network is a gate people learn to skip.
    echo "  semver baseline: $baseline (not checked -- the index was unreachable)"
  else
    newest=$(printf '%s\n' "$index" | sed -n 's/.*"vers":"\([^"]*\)".*/\1/p' | tail -1)
    if [ "$baseline" != "$newest" ]; then
      echo "VERSION GATE: the semver baseline is $baseline, the newest published launchbound-cli is $newest" >&2
      echo "  move --baseline-version in .github/workflows/ci.yml (docs/RELEASING.md)" >&2
      status=1
    else
      echo "  semver baseline: $baseline (the newest published)"
    fi
  fi
fi

if [ "$status" -eq 0 ]; then
  echo "every recorded version agrees"
fi
exit "$status"
