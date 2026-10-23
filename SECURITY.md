# Security Policy: clock-kit

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅        |

## Reporting a vulnerability

Report privately via [GitHub security advisories] for this repository, or
email **wyatt_au@protonmail.com**. Do **not** open a public issue for
security reports.

You will receive an acknowledgement within **72 hours**. Coordinated
disclosure: we ask for up to 90 days before public disclosure while a
patch ships.

## Scope notes

`clock-kit` is time **substrate**: it produces timestamps and quality
hints for latency-critical paths. Security considerations for
integrators:

- **`SyncQuality` is advisory, never authentication.** A `Ptp`
  classification means "a PTP device exists and the kernel's TAI offset
  looks coherent at probe time": it can be spoofed by anything that can
  write sysfs or set the kernel TAI offset. Never use it as a security
  boundary; use it to decide whether timestamps may be correlated across
  machines.
- **Detection is read-only by design.** REQ-CLK-004 performs no PTP
  participation: no daemon, no PHC control writes, no `adjtimex`. The
  probe surfaces are `read_dir` on a sysfs path and `clock_gettime`:
  no I/O beyond that, no network, no secrets.
- **unsafe surface is one file.** The crate is
  `#![deny(unsafe_code)]` with every other module
  `#![forbid(unsafe_code)]`; the only unsafe is in `src/low.rs`: the
  x86_64 `rdtsc` intrinsic, two aarch64 `mrs` system-register reads, and
  two `clock_gettime(2)` FFI wrappers, each justified in place
  (`undocumented_unsafe_blocks` denied). No unsafe exists in the ring,
  mock, floor, timestamp, or trait code.
- **`stats()` stack scratch is `CAP × 8 B`.** The default (4096 → 32 KiB)
  assumes ≥ 64 KiB stacks; embedding consumers with smaller stacks must
  size `CAP` accordingly. No recursion exists in the sort (iterative
  heapsort) and no heap allocation on any path.
- **Multi-socket TSC caveat.** On hosts where sockets boot TSCs
  non-synchronously, counter-derived mono may be skewed across sockets;
  the invariant-TSC check does not detect per-socket desync. Consumers
  needing hard cross-socket accuracy should use the `Instant`/
  `CLOCK_MONOTONIC` fallback (`CalibratedClock::with_fallback`).
- **Calibration drift.** The ns-per-tick factor slowly drifts with
  temperature/frequency. `calibrate()` is repeatable by design; re-run it
  periodically for long-lived processes.

[GitHub security advisories]:
    https://github.com/WyattAu/clock-kit/security/advisories/new
