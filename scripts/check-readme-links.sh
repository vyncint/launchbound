#!/usr/bin/env bash
# Every link in the packaged README must be absolute, must name the right
# GitHub path kind, and must point at something that exists here.
#
# All eleven crates set `readme = "../../README.md"`, so this file is shipped
# as each one's readme and crates.io renders it there. crates.io rewrites a
# *relative* link against the crate's directory in the repository, not the
# repository root -- so `docs/SAFETY.md` became
# `…/blob/HEAD/crates/launchbound-space/docs/SAFETY.md`, and all eight
# relative links were 404 on all eleven published 2.2.0 pages. Nothing here
# could see it: the same file renders correctly on GitHub, where it *is* at
# the root.
#
# So: absolute links only, and the path each names must exist. The
# existence check is offline and deterministic, which is the point -- it
# catches a renamed or deleted target, and a link checker that only asks
# GitHub would not, because GitHub answers 200 for a redirect.
#
# `blob` vs `tree` is checked for the same reason: `blob/main/action/` is a
# directory and GitHub 301s it to `tree/main/action`. It resolves, so a
# status-code checker calls it fine; it is still the wrong URL to publish.
#
# Portability: macOS ships bash 3.2 and BSD grep/sed.
set -euo pipefail

readme="${1:-README.md}"
root=$(cd "$(dirname "$0")/.." && pwd)
repo="https://github.com/vyncint/launchbound"
status=0

# `[text](target)` whose target is neither absolute nor a bare fragment.
relative=$(grep -oE '\]\([^)]+\)' "$readme" \
  | sed -E 's/^\]\(//; s/\)$//' \
  | grep -vE '^(https?:|#|mailto:)' || true)
if [ -n "$relative" ]; then
  echo "$relative" | while IFS= read -r link; do
    echo "README LINK: relative target \"$link\" — crates.io rewrites it against" >&2
    echo "  crates/<crate>/, where it does not exist. Use $repo/blob/main/$link" >&2
  done
  status=1
fi

checked=0
bad=0
for url in $(grep -oE "$repo/(blob|tree)/main/[^)]+" "$readme" | sort -u); do
  kind=$(printf '%s' "$url" | sed -E "s|^.*/(blob\|tree)/main/.*$|\1|")
  path=$(printf '%s' "$url" | sed -E "s|^.*/(blob\|tree)/main/||; s|#.*$||; s|/$||")
  checked=$((checked + 1))
  if [ ! -e "$root/$path" ]; then
    echo "README LINK: \"$path\" is linked but not in the repository" >&2
    bad=$((bad + 1))
    continue
  fi
  # A directory is `tree`, a file is `blob`. GitHub redirects the wrong one
  # rather than failing, so only this says so.
  if [ -d "$root/$path" ] && [ "$kind" != tree ]; then
    echo "README LINK: \"$path\" is a directory; use /tree/main/ not /blob/main/" >&2
    bad=$((bad + 1))
  elif [ -f "$root/$path" ] && [ "$kind" != blob ]; then
    echo "README LINK: \"$path\" is a file; use /blob/main/ not /tree/main/" >&2
    bad=$((bad + 1))
  fi
done
if [ "$bad" -gt 0 ]; then
  status=1
fi

if [ "$status" -eq 0 ]; then
  echo "readme links: $checked absolute link(s), every target present and the right kind"
fi
exit "$status"
