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

## Re-running the CUDA path

`gpu.yml` is dispatch-only, because it costs money. It never runs on a push
or a schedule.

```sh
gh workflow run gpu.yml \
  -f kernel=reduce-flip -f cc=8.6 -f budget_secs=900 -f runner=gpu
```

Two jobs, split the way `stage` and `launchbound-runner` were designed to be
split. `stage` prunes and compiles every admitted specialization on a plain
`ubuntu-latest` runner, builds the box-side binary beside the plan, and
uploads both as one artifact — so the expensive machine needs a driver and
nothing else: no toolchain, no cuda-oxide checkout, no analyzer. `measure`
runs on whatever `runner` names.

**Without a self-hosted runner**, dispatch with `-f measure=false`, take the
`bench-plan` artifact to any CUDA box, and run the same command the workflow
issues:

```sh
./launchbound-runner --budget-secs 900 plan.json results.json
```

### What it gates on

The structural claims, which port:

- the results declare `results.v1`
- no candidate in the results is absent from the plan
- no admitted candidate failed to run — one that does is the hole under
  "cuda-oxide is alpha" in [LIMITATIONS](LIMITATIONS.md), and each is worth
  reporting because it names another codegen-time const the gate does not
  evaluate
- something was measured

**Not the timings.** Those are valid only for the GPU, driver and compiler in
their provenance, and `sm_75` and `sm_86` do not transfer — so the workflow
prints the five fastest with the device, driver and plan capability beside
them, and uploads `results.json`, rather than asserting a number.

It also does not provision the machine. This repository holds no cloud
credentials, and adding one is a decision with a blast radius rather than a
workflow detail.
