# clock-kit
[![Estate integration](https://github.com/WyattAu/estate-integration/actions/workflows/ci.yml/badge.svg)](https://github.com/WyattAu/estate-integration/actions/workflows/ci.yml)

Precision time substrate for Rust: monotonic/wall separation, TSC
calibration, PTP quality detection, and a deterministic test clock. The
estate's time source for latency-critical paths (feed handlers, matching
threads, latency measurement): it answers how expensive reading the clock
is, how much to trust it, and how to test against it deterministically.

- **Monotonic by construction (G1)**: `Timestamp::mono` never goes backward
  within a process: enforced by a process-wide `fetch_max` floor (with a
  critical-section fallback on targets without 64-bit atomics), not by
  trust in the underlying source. A regression is a bug, not a documented
  possibility.
- **Wall and monotonic are separate (REQ-CLK-002)**: `Timestamp` carries
  both with independent rules. Wall time may step (NTP slew, PTP step,
  operator action); mono never steps and never regresses. The single
  cross-derivation in the API is `Timestamp::mono_from_wall_lossy`,
  explicitly labelled lossy.
- **Cheap reads behind calibration (REQ-CLK-003)**: x86_64 `rdtsc` /
  aarch64 `cntvct_el0` behind a calibrated 32.32 multiply-shift (no
  division on the read path); invariant-counter check before enabling the
  fast path; transparent fallback to `Instant` / `CLOCK_MONOTONIC`, with
  the active source observable through `Clock::source()`.
- **PTP detection, not participation (REQ-CLK-004)**: `detect_sync_quality`
  classifies `Ptp` / `Ntp` / `Fallback` from `/sys/class/ptp` presence
  plus a coherent `CLOCK_TAI` offset probe. Read-only, best-effort,
  fail-closed to distrust. It runs no daemon and touches nothing.
- **Deterministic fiction (G3 / REQ-CLK-005)**: `MockClock` never consults
  the OS. Time advances only when the test advances it; `step_wall`
  simulates NTP/PTP wall steps without touching mono.
- **Allocation-free latency statistics (REQ-CLK-006)**: `LatencyRing<CAP>`
  is a fixed-capacity sliding window of nanos; `stats()` is *exact* over
  the current window (iterative heapsort, stack scratch, zero heap
  allocation) reporting min/max/mean/p50/p99/p99.9 at nearest rank.
- **`#![deny(unsafe_code)]`, `#![deny(missing_docs)]`**, clippy
  `unwrap_used`/`expect_used`/`panic`/`indexing_slicing` denied. The
  crate's only unsafe is the hardware-counter reads and the
  `clock_gettime` FFI wrappers, confined to `src/low.rs`: every other
  module re-establishes `#![forbid(unsafe_code)]` locally (see
  [SECURITY.md](SECURITY.md)).

## Install

```toml
[dependencies]
clock-kit = "0.1"
```

## Example

```rust
use clock_kit::{Clock, ClockSource, LatencyRing, MockClock, SyncQuality};
use std::time::Duration;

// Production: observe cost and trust, not just time.
let t = clock_kit::SystemClock::new().now();
let _ = (t.wall.unix_nanos(), t.mono);

// Deterministic tests: the OS is never consulted.
let clock = MockClock::start_at(1_000, 1_000_000);
clock.advance(Duration::from_millis(5));
clock.step_wall(-50_000); // NTP-style wall step; mono untouched
assert_eq!(clock.now().mono, 5_001_000);
assert_eq!(clock.source(), ClockSource::Mock);
assert_eq!(clock.quality(), SyncQuality::Fallback);

// Latency stats: exact over a fixed window, no allocation.
let mut ring = LatencyRing::<4096>::default();
ring.record(1_500);
ring.record(200);
let s = ring.stats();
assert_eq!((s.min, s.max), (200, 1_500));
```

## The chronoshift bridge: one estate time idiom

[chronoshift] (L0) owns time *abstraction*; clock-kit (L1) owns time
*precision and cost*. clock-kit does not mint a rival trait. With the
`chronoshift` feature, a blanket impl makes **every `chronoshift::Clock`
a `clock_kit::Clock`**: its `now_ns()` is bridged into a full `Timestamp`
(negative pre-epoch values clamp to 0) with the monotonic floor applied,
`quality()` fails closed to `Fallback`, and `source()` reports `External`.

```toml
[dependencies]
clock-kit = { version = "0.1", features = ["chronoshift"] }
```

Note: types implementing `chronoshift::Clock` become clock-kit `Clock`s
automatically: do not also `impl clock_kit::Clock` for them (coherence).
The blanket points one way; clock-kit's own clocks are not
`chronoshift::Clock`s.

[chronoshift]: https://crates.io/crates/chronoshift

## Feature flags

| Feature | Default | Description |
|---|---|---|
| `std` | yes | `SystemTime`-backed wall, `Instant`-anchored mono |
| `libc` | no | `clock_gettime(2)` wall/mono for no_std hosts |
| `tsc` | no | Hardware counters (x86_64 `rdtsc`, aarch64 `cntvct_el0`) behind calibration; per-arch `cfg(target_arch)` inside one feature |
| `ptp` | no | Sync-quality detection (`/sys/class/ptp` + `CLOCK_TAI`); implies `std` |
| `chronoshift` | no | Blanket `Clock` impl for every `chronoshift::Clock` |

## no_std posture

`--no-default-features` builds pure `core`: `Timestamp`, `WallTime`,
`Clock`, `MockClock`, `LatencyRing`, `SyncQuality`, and the public
`mono_floor`. Wall reads need `std` or `libc`; the calibrated clock needs
`std` or `libc` as its reference. On targets without 64-bit atomics
(e.g. `thumbv7em`), the floor and mock cells fall back to
`critical-section`: the impl is provided by the final binary, per
critical-section convention (the chronoshift pattern).

## Calibration model and drift caveat

`calibrate(reps)` brackets counter reads between reference-clock reads and
derives a 32.32 ns-per-tick factor (two independent measurements must
agree within 1%). Conversion is multiply-shift only:

```
ns = ticks * ns_per_tick + (ticks * frac) >> 32
```

Calibration is **repeatable, not continuous**: thermal drift and frequency
scaling slowly bias the factor. Consumers needing hard accuracy over long
runs should re-call `calibrate` periodically (the factor is cheap to swap:
`CalibratedClock::with_calibration`). The invariant-TSC CPUID check guards
against non-constant-rate counters at construction; a per-socket TSC skew
caveat applies to multi-socket hosts (documented; see SECURITY.md).

## Percentile semantics

`LatencyRing::stats` is **exact over the window** (sorts a bounded stack
copy), not a streaming approximation: HFT latency gates want exact
numbers over a known window. Percentiles are nearest-rank, re-implemented
lean from the percentile-kit pattern: percentile-kit is L2 and cannot be a
dependency of this L1 crate (estate layer rule: no upward edges). If the
ring is ever shared, the shared primitive must be lifted *down* to L0.

## MSRV

Rust 1.85 (edition 2021).

## License

MIT OR Apache-2.0.
