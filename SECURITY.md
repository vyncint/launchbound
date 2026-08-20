# Security policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately via
[GitHub security advisories](https://github.com/vyncint/launchbound/security/advisories/new).
Do not open a public issue for a security report.

You can expect an acknowledgement within a week. Coordinated disclosure is
preferred; please allow a fix to land before publishing details.

## Scope worth knowing about

`launchbound` executes compilers and benchmarks on the machine it runs on: it
invokes `cargo`, `cargo reconverge`, and `cargo oxide` as subprocesses, and
runs kernels you point it at. Treat tuning untrusted kernel sources the same
way you would treat running `cargo build` in an untrusted repository — build
scripts and procedural macros run arbitrary code. That is inherited from the
Rust toolchain, not a `launchbound` vulnerability, but reports about
`launchbound` making it worse (e.g. argument injection into the tools it
drives) are very much in scope.

The safety gate is a correctness feature, not a security boundary: a clean
gate is not a proof of correctness, and nothing in this tool sandboxes the
kernels it measures.
