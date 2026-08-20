# The safety gate

This document is the citable statement of `launchbound`'s safety decision
rule. It is stated here and in the README. **Changing
this rule requires explicit human sign-off** — it is the product.

## The invocation

For each candidate configuration, `launchbound` invokes:

```
cargo reconverge check --strict --message-format json --cc <target>
```

- `--strict` is **mandatory, not optional**. The findings this product exists
  to catch are warning-tier by `reconverge`'s design: a launch contract is a
  declaration, not a proof, so launch-shape-dependent findings never promote
  to a gate on their own and `cargo reconverge check` exits 0 on them at
  default confidence. An autotuner does not have that excuse — it is choosing
  the launch configuration itself, so it can supply the contract `reconverge`
  lacks and turn the warning into a decidable answer.
- `--cc <X.Y>` is supplied per target because shared-memory capacity context
  (RC004) depends on it, and shared-memory layout is a tuning dimension. A
  safety verdict obtained at one `--cc` does not transfer to another (T4 is
  `--cc 7.5`; A10G is `--cc 8.6`).
- Exit codes: `0` clean, `1` findings at deny/confirmed, `2` tool error. A `2`
  is a **hard stop for that candidate**, never a pass by omission.

`launchbound` parses `reconverge`'s `findings.v1` output and records, per
candidate: rule ID, confidence, and the source span. That record is what the
rejection report prints.

## The decision rule

For a candidate configuration evaluated **at its own launch configuration**:

1. **Disqualified** — `reconverge` reports a barrier or masked-collective
   finding whose divergence source is launch-shape-dependent.
2. **Admitted with a recorded caveat** — a finding exists but is
   launch-shape-independent. The caveat appears in the report.
3. **Clean** — otherwise.

A disqualified configuration is never selected, never ranked, and always
listed in the rejection report with its rule ID and source span — especially
when it was faster than the configuration chosen.

## Escape hatch

`--allow-unsafe` exists for users who know their launch contract differs from
the declared one. It requires an explicit reason string, the reason is
recorded verbatim in the report, and it is never the default. A missing
reason string is a usage error, not a warning.

## What a clean gate does not mean

The gate inherits `reconverge`'s documented limits wholesale: summary-based
interprocedural analysis, reducible control flow only, non-literal masks not
evaluable, and data races entirely out of scope. **A clean gate is not a
proof of correctness.** It means: at this launch configuration, the analyzer
found no barrier or masked-collective divergence hazard within its documented
coverage.

The Metal backend has **no gate at all** — there is no `reconverge`
equivalent for MSL and this project does not build one. Every Metal report
header carries that notice, and a test asserts the notice cannot be omitted.
