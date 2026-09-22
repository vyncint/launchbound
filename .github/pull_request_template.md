## Summary

<!-- What does this change, and why? -->

## Checklist

- [ ] Title is a scoped Conventional Commit (e.g. `bench: …`), imperative, ≤ 72 chars
- [ ] Every commit is signed off (`git commit -s`; the DCO trailer matches the author)
- [ ] No AI attribution anywhere (trailers, message bodies, identities)
- [ ] Tests added or updated, and the new test was watched to fail once
- [ ] `just ci` is green locally
- [ ] Does this move a measured number? If so the CHANGELOG entry is marked `bench:` and names the hardware
- [ ] Does this touch the lockstep pin set (nightly / cuda-oxide / reconverge)? If so it is its own commit and `just pins` passes
