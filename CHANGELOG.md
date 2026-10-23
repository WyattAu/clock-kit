# Changelog

All notable changes to this project are documented here. Format: [Keep a
Changelog](https://keepachangelog.com/): versions follow [semver](https://semver.org).

## [0.1.0] - 2026-10-03

### Added

Initial release, per `specs/clock-kit.md` (REQ-CLK-001…006).

- **`Timestamp`** (REQ-CLK-002): wall (`WallTime`, `SystemTime`-backed
  under `std`, `u64` epoch-nanos under core+libc: identical
  `unix_nanos()` accessor on both) + monotonic `u64` nanos, separate
  fields with separate rules; the only cross-derivation is the explicitly
  lossy `Timestamp::mono_from_wall_lossy`.
- **Process-wide monotonic floor** (REQ-CLK-001): `mono_floor()` public
  for custom `Clock` authors; `fetch_max` atomic on 64-bit-atomic
  targets, critical-section cell fallback (chronoshift pattern) on
  targets without them.
- **`Clock` trait**: `now() -> Timestamp`, `quality() -> SyncQuality`,
  `source() -> ClockSource`; object-safe; cost and trust observable.
- **chronoshift bridge** (feature `chronoshift`): blanket impl: every
  `chronoshift::Clock` is a clock-kit `Clock` (one estate time idiom;
  L1→L0 edge, ceiling-legal). `pub use chronoshift` re-export.
- **`SystemClock`** (std or libc path): `SystemTime`+`Instant` under
  `std`, `CLOCK_REALTIME`+`CLOCK_MONOTONIC` under `libc`; quality cached
  under the `ptp` feature.
- **`CalibratedClock` + `calibrate()`** (REQ-CLK-003, feature `tsc`):
  x86_64 `rdtsc` (invariant-TSC CPUID check), aarch64 `cntvct_el0`
  (frequency sanity), 32.32 multiply-shift conversion (no division on
  the read path), 1% two-measurement stability gate, transparent
  fallback to `Instant`/`CLOCK_MONOTONIC`; `with_fallback()`,
  `with_calibration()`, `try_new()` constructors.
- **`detect_sync_quality()` / `tai_offset_ns()` / `tai_offset_coherent()`
  / `classify_sync_quality()`** (REQ-CLK-004, feature `ptp`):
  `/sys/class/ptp` discovery (minimal `ptp<digits>` parsing) + coherent
  `CLOCK_TAI` probe + NTP runtime markers; fail-closed classification;
  injectable sysfs root for tests.
- **`MockClock`** (REQ-CLK-005): `start_at` / `advance` /
  `advance_nanos` / `step_wall`: deterministic fiction, floor-free by
  design (G3), saturating at bounds.
- **`LatencyRing<const CAP: usize = 4096>`** (REQ-CLK-006): fixed-capacity
  sliding window of `u64` nanos; O(1) `record` with oldest-eviction;
  exact `stats()` (min/max/mean/p50/p99/p99.9, nearest rank: the
  percentile-kit algorithm re-implemented lean: percentile-kit is L2 and
  cannot be an L1 dependency) with zero heap allocation and no
  recursion (iterative heapsort).
- **Gates**: tier-A estate matrix pinned to engineering-standards main;
  loom model for the atomic-max floor; miri on the ring + mock suites;
  per-target coverage jobs (x86_64 + aarch64) with arch residuals in
  `COVERAGE-NOTES.md`; thumbv7em embedded check; criterion baselines
  (`now()` per source, `record`/`stats` vs CAP).

[0.1.0]: https://github.com/WyattAu/clock-kit/releases/tag/v0.1.0
