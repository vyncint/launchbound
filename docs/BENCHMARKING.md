# Benchmarking methodology

A benchmark that reports a mean and no interval is not evidence. Every timing this project publishes carries all of:

- **warmup**: default 20 unmeasured launches per candidate;
- **repeats**: default 100 timed launches;
- **kernel-only time**: a cuEvent pair brackets each launch on the default
  stream (`GPUStartTime`/`GPUEndTime` on Metal) — transfers and process
  overhead are excluded, and the docs say so;
- **outlier rule**: Tukey fences at 1.5 IQR, with the rejected count
  reported;
- **median and a distribution-free 95% CI** on the median (order
  statistics, normal approximation to the binomial ranks);
- **GPU-seconds consumed**, per candidate and per sweep;
- **provenance**: device name, compute capability, driver version, gate cc,
  strategy, and whether the figure is `measured` or `estimated`.

**Indistinguishability.** Two configurations whose 95% CIs overlap are
reported as indistinguishable and never ranked against each other. The
chosen configuration lists its indistinguishable set explicitly.

**Reproducibility check (A10G, 2026-08-20).** Two independent exhaustive
sweeps of reduce-stable, 36 minutes apart under continuous load, reproduced
11/11 candidates within overlapping CIs. Input data is deterministic
(seeded xorshift), so reruns measure the same workload.

**What the numbers do not include.** Launch overhead, host-device
transfers, JIT time, and occupancy interactions with co-resident kernels. A
candidate that wins here can lose inside a larger application; verify with
a profiler (Nsight Compute) in context.

**Unsafe candidates.** Gate-refused configurations are measured only under
`--allow-unsafe --reason ...`, behind a watchdog with a pre-checkpointed
`timeout` record: a hang is a recorded result, not a crash. Their timings
appear only in the rejection report and are never presented as safe.
