# Releasing launchbound

One page, copy-pasteable. Maintainers only. The same shape as the sibling
projects' release docs — [termlens], [mossaic], [reconverge] — so a
maintainer moving between them is not relearning the process.

## Prerequisites

- **crates.io Trusted Publishing**, linked to this repository and
  `release.yml`. No token is stored anywhere; the publish job mints a
  short-lived one over OIDC.
- **`v*.*.*` tags protected by a ruleset**, so only a maintainer can push one.
- Eight crates publish **in dependency order**, and `cargo publish` waits for
  each to appear on the index before the next. A rate-limited run can be
  resumed with `workflow_dispatch`, which skips crates already on the
  registry.

## Cutting vX.Y.Z

```sh
# 0. Green main, and no flakes. The gate runs per PR; the hunt does not.
gh workflow run stress.yml -f iterations=100
gh run watch                    # ten shards, both OSes

# 1. Bump the version. It appears once per crate plus the workspace pins.
$EDITOR Cargo.toml              # version = "X.Y.Z"
cargo check --workspace         # refreshes Cargo.lock

# 2. Move the CHANGELOG section: [Unreleased] -> [X.Y.Z] - YYYY-MM-DD,
#    leaving an empty [Unreleased] above it.

# 3. Bump every version the docs name. Two kinds go stale: the
#    `action@vN` refs people copy, and "pin a number" examples a reader
#    reasonably reads as current. This finds both:
grep -rEn "launchbound/action@v|[0-9]+\.[0-9]+\.[0-9]+" docs action README.md \
  | grep -v CHANGELOG

# 4. Land it.
git switch -c release/vX.Y.Z
git commit -sam "release: vX.Y.Z"
gh pr create --fill

# 5. Tag the squash-merged commit on main.
git switch main && git pull
git tag vX.Y.Z && git push origin vX.Y.Z
```

Pushing the tag runs `release.yml`, which gates, then publishes each crate in
order via Trusted Publishing.

## After the tag

- **The GitHub Release is created by hand**, from the CHANGELOG section:
  `gh release create vX.Y.Z --title "launchbound X.Y.Z" --notes-file …`.
  Every released version has one; do not skip it.
- **Verify what was published, not what was built.** `install.yml` installs
  from crates.io into a clean directory and runs the binaries; dispatch it
  once the version is live:
  ```sh
  gh workflow run install.yml
  ```

## What a version number means here

- **Breaking** (minor pre-1.0, major after): a removed or renamed public item,
  a changed CLI flag, or a change to what the gate admits that a user would
  have to relearn.
- **Not breaking**: new flags, new backends, a corpus addition, a report field.
- **MSRV and pinned-toolchain bumps are minor**, never patch, and never land
  in the same change as a behaviour change.

## If something fails mid-release

- **Before publish**: fix, delete the tag (`git push --delete origin vX.Y.Z`),
  re-tag. Nothing was published; the world never saw it.
- **Part-way through the eight crates**: re-run `release.yml` by dispatch. It
  skips what is already on the registry.
- **After publish**: crates.io is immutable. Ship `X.Y.Z+1`. Yank only if the
  release is actively harmful — a yanked crate still breaks downstream
  lockfiles.

[termlens]: https://github.com/vyncint/termlens/blob/main/docs/RELEASING.md
[mossaic]: https://github.com/vyncint/mossaic/blob/main/docs/RELEASING.md
[reconverge]: https://github.com/vyncint/reconverge/blob/main/docs/RELEASING.md
