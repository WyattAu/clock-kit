# Coverage notes — clock-kit

Per the spec's gate plan (spec §Gate plan), the llvm-cov denominator for
clock-kit excludes arch-gated `tsc` code on non-native hosts; dedicated
per-target jobs measure their own branches. This file records the
residuals.

## Methodology (validkit per-config lesson)

`--all-features` unions features but a single host target cannot execute
foreign-arch branches — union coverage would understate. Therefore:

- **x86_64 job** (the shared tier-A `coverage` job): measures everything
  reachable on x86_64 — the `tsc` x86_64 branch executes (GitHub
  runners report the invariant-TSC CPUID leaf), the calibrated hot path,
  PTP detection, ring, mock, floor, trait, timestamps.
- **aarch64 job** (`coverage-aarch64` in `ci.yml`, `ubuntu-24.04-arm`):
  measures the `cntvct_el0` branch and its frequency sanity check; the
  x86_64 CPUID branch is cfg'd out there and vice versa. Both jobs gate
  at 90% lines over their own denominator.

## Current residuals (x86_64 job, `--all-features`)

Measured 2026-10-03 at v0.1.0: **96.24% lines** (threshold 90%).

Lines intentionally uncovered on this host class:

- `src/low.rs` — `invariant_counter_supported` early return (CPUID max
  extended leaf < 0x8000_0007): unreachable on any CI runner CPU; the
  branch is the guard for pre-2010 hardware. Cross-checked by reading:
  the branch is two instructions around a hardware probe that cannot be
  faked in-process.
- `src/calibrated.rs` — `CalError::NoProgress` / `dns == 0` /
  `UnstableCounter` error arms and the `Err(_) =>` transparent-fallback
  arm of `CalibratedClock::new`: require a dead or drifting counter.
  The fallback *behavior* is covered via `with_fallback()`; only the
  hardware-triggered construction path is not.
- `src/low.rs` — `clock_gettime` failure arms (`None`): `CLOCK_TAI` /
  `CLOCK_MONOTONIC` do not fail on supported kernels.
- `src/ptp.rs` — `ntp_runtime_hint` marker paths beyond the first:
  existence checks are short-circuit `any`; on CI hosts the first
  marker decides. (Host-state dependent, deterministic per host.)

The aarch64 job covers `counter::read` (CNTVCT), `counter_frequency_hz`,
and the aarch64 arm of `calibrate`; the x86_64 job covers `rdtsc` and the
CPUID invariant check. Neither host can cover the other's arch — by
design of the per-target gate.
