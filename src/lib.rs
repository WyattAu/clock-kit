//! `clock-kit` — the estate's precision time substrate (L1).
//!
//! clock-kit answers three questions `std` leaves unresolved: **how expensive
//! is reading the clock** ([`Clock::source`]), **how much do we trust it**
//! ([`SyncQuality`]), and **how do we test against it deterministically**
//! ([`MockClock`]). It is the time source for latency-critical paths — feed
//! handlers, matching threads, latency measurement.
//!
//! # Design
//!
//! - **Guarantee grades.** [`Timestamp::mono`] never regresses within a
//!   process — enforced by a process-wide floor, not by trust in the
//!   underlying source (G1). [`SyncQuality`] is an advisory hint, never a
//!   guarantee (G2). [`MockClock`] never consults the OS: time moves only
//!   when the test moves it (G3).
//! - **Wall and monotonic are separate.** [`Timestamp`] carries both with
//!   independent rules; the only cross-derivation is the explicitly lossy
//!   [`Timestamp::mono_from_wall_lossy`] (REQ-CLK-002).
//! - **Cheap reads behind calibration.** With the `tsc` feature, `x86_64` reads
//!   `rdtsc` and aarch64 reads `cntvct_el0`, converted through a calibrated
//!   multiply-shift (no division on the read path); if calibration or the
//!   invariant-counter check fails, [`CalibratedClock`] transparently falls
//!   back to `Instant` / `CLOCK_MONOTONIC` (REQ-CLK-003).
//! - **PTP detection, not participation.** With the `ptp` feature,
//!   [`detect_sync_quality`] classifies the host's sync state by looking at
//!   `/sys/class/ptp` and probing `CLOCK_TAI`. It runs no daemon and touches
//!   nothing (REQ-CLK-004).
//! - **Allocation-free latency statistics.** [`LatencyRing`] is a
//!   fixed-capacity, sliding-window ring of `u64` nanos; [`stats`]
//!   is exact over the current window with zero heap allocation (REQ-CLK-006).
//!
//! # chronoshift bridge
//!
//! The estate keeps **one time idiom**: chronoshift (L0) owns time
//! *abstraction*, clock-kit owns time *precision and cost*. With the
//! `chronoshift` feature, every `chronoshift::Clock` is automatically a
//! [`Clock`] (blanket impl): its `now_ns()` is bridged into a full
//! [`Timestamp`] with the monotonic floor applied. clock-kit does not mint a
//! rival trait — its [`Clock`] extends the idiom with quality and source
//! observability.
//!
//! # Feature flags
//!
//! | Feature | Default | Description |
//! |---|---|---|
//! | `std` | yes | `SystemTime`-backed wall time, `Instant` monotonic reads |
//! | `libc` | no | `clock_gettime(2)` wall/monotonic reads for `no_std` hosts |
//! | `tsc` | no | Hardware-counter reads behind calibration (`x86_64/aarch64`) |
//! | `ptp` | no | Sync-quality detection (`/sys/class/ptp` + `CLOCK_TAI`); implies `std` |
//! | `chronoshift` | no | Blanket `Clock` impl for every `chronoshift::Clock` |
//!
//! # `no_std` posture
//!
//! Core-only (no `std`, no `libc`): [`Timestamp`], [`WallTime`], [`Clock`],
//! [`MockClock`], and [`LatencyRing`] build with `--no-default-features` —
//! pure `core`. Wall reads need `std` or `libc`. The critical-section
//! fallback for targets without 64-bit atomics follows the chronoshift
//! convention: the final binary provides the `critical-section` impl.
//!
//! # Example
//!
//! ```
//! use clock_kit::{Clock, ClockSource, MockClock, SyncQuality};
//! use std::time::Duration;
//!
//! let clock = MockClock::start_at(1_000, 1_000);
//! clock.advance(Duration::from_millis(5));
//! let t = clock.now();
//! assert_eq!(t.mono, 5_001_000);
//!
//! // Deterministic: the OS is never consulted.
//! assert_eq!(clock.source(), ClockSource::Mock);
//! assert_eq!(clock.quality(), SyncQuality::Fallback);
//! ```
//!
//! [`stats`]: LatencyRing::stats
//! [`CalibratedClock`]: crate::calibrated::CalibratedClock
//! [`detect_sync_quality`]: crate::ptp::detect_sync_quality

#![cfg_attr(not(feature = "std"), no_std)]

#[forbid(unsafe_code)]
pub mod clock;
#[forbid(unsafe_code)]
pub mod mock;
#[forbid(unsafe_code)]
pub mod monotonic;
#[cfg(feature = "ptp")]
#[forbid(unsafe_code)]
pub mod ptp;
#[forbid(unsafe_code)]
pub mod quality;
#[forbid(unsafe_code)]
pub mod ring;
#[cfg(any(feature = "std", feature = "libc"))]
#[forbid(unsafe_code)]
pub mod system;
#[forbid(unsafe_code)]
pub mod timestamp;

// The crate's only unsafe: counter reads + clock_gettime FFI wrappers.
#[cfg(any(feature = "std", feature = "libc"))]
#[forbid(unsafe_code)]
pub mod calibrated;
#[allow(unsafe_code)]
mod low;

pub use clock::Clock;
pub use mock::MockClock;
pub use monotonic::mono_floor;
pub use quality::{ClockSource, SyncQuality};
pub use ring::{IntervalStats, LatencyRing};
pub use timestamp::{Timestamp, WallTime};

#[cfg(feature = "chronoshift")]
pub use chronoshift;
// Raw counter reads are public API per the spec sketch (REQ-CLK-003) —
// convert through `calibrated::Calibration` before interpreting as time.
#[cfg(any(feature = "std", feature = "libc"))]
pub use calibrated::{calibrate, CalError, CalMethod, CalibratedClock, Calibration};
#[cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
pub use low::counter;
#[cfg(feature = "ptp")]
pub use ptp::{
    classify_sync_quality, detect_sync_quality, detect_sync_quality_in, tai_offset_coherent,
    tai_offset_ns,
};
#[cfg(any(feature = "std", feature = "libc"))]
pub use system::SystemClock;
