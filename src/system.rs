//! `SystemClock` — real wall+monotonic reads (`std` or `libc` path).
//!
//! Wall: `SystemTime` under `std`, `clock_gettime(CLOCK_REALTIME)` under
//! core+libc. Monotonic: an `Instant`-anchored counter under `std`,
//! `clock_gettime(CLOCK_MONOTONIC)` under libc. Both readings pass through
//! the process-wide monotonic floor (REQ-CLK-001) and clamp (never panic)
//! on misconfigured machines.

use crate::clock::Clock;
use crate::monotonic::mono_floor;
use crate::quality::{ClockSource, SyncQuality};
use crate::timestamp::{Timestamp, WallTime};

#[cfg(all(feature = "std", feature = "ptp"))]
use core::sync::atomic::{AtomicU8, Ordering};
#[cfg(all(not(feature = "std"), feature = "libc"))]
use libc::{CLOCK_MONOTONIC, CLOCK_REALTIME};
#[cfg(feature = "std")]
use std::sync::OnceLock;
#[cfg(feature = "std")]
use std::time::Instant;

/// Real system clock (REQ-CLK-001/002). Zero-sized; `Clock` is implemented
/// on a shared reference so one instance can be handed across threads.
///
/// With the `ptp` feature, `quality()` lazily runs
/// [`detect_sync_quality`](crate::ptp::detect_sync_quality) once and caches
/// it process-wide (G2: advisory, read at discovery time).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SystemClock;

impl SystemClock {
    /// Create the clock.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp {
            wall: WallTime::from_unix_nanos(wall_nanos()),
            mono: mono_floor(monotonic_nanos()),
        }
    }

    fn quality(&self) -> SyncQuality {
        #[cfg(feature = "ptp")]
        {
            cached_quality()
        }
        #[cfg(not(feature = "ptp"))]
        {
            // Nothing detected → fail-closed to distrust (G2).
            SyncQuality::Fallback
        }
    }

    fn source(&self) -> ClockSource {
        #[cfg(feature = "std")]
        {
            ClockSource::Instant
        }
        #[cfg(not(feature = "std"))]
        {
            ClockSource::LibcMono
        }
    }
}

/// Wall nanos since the Unix epoch (0 on pre-epoch misconfiguration).
pub(crate) fn wall_nanos() -> u64 {
    #[cfg(feature = "std")]
    {
        crate::timestamp::WallTime::from_system_now().unix_nanos()
    }
    #[cfg(all(not(feature = "std"), feature = "libc"))]
    {
        crate::low::clock_gettime_ns(CLOCK_REALTIME).unwrap_or(0)
    }
}

/// Monotonic nanos: `Instant` elapsed since a process-wide anchor under
/// `std`; `CLOCK_MONOTONIC` under libc.
pub(crate) fn monotonic_nanos() -> u64 {
    #[cfg(feature = "std")]
    {
        static ANCHOR: OnceLock<Instant> = OnceLock::new();
        let anchor = ANCHOR.get_or_init(Instant::now);
        u64::try_from(anchor.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
    #[cfg(all(not(feature = "std"), feature = "libc"))]
    {
        crate::low::clock_gettime_ns(CLOCK_MONOTONIC).unwrap_or(0)
    }
}

#[cfg(all(not(feature = "std"), feature = "libc"))]
use libc::{CLOCK_MONOTONIC, CLOCK_REALTIME};

#[cfg(feature = "ptp")]
pub(crate) fn cached_quality() -> SyncQuality {
    // 0 = undetected, 1..=3 = SyncQuality discriminants.
    static CACHE: AtomicU8 = AtomicU8::new(0);
    let seen = CACHE.load(Ordering::Acquire);
    if seen != 0 {
        return num_to_quality(seen);
    }
    let detected = crate::ptp::detect_sync_quality();
    CACHE.store(discriminant(detected), Ordering::Release);
    detected
}

#[cfg(feature = "ptp")]
fn discriminant(q: SyncQuality) -> u8 {
    match q {
        SyncQuality::Ptp => 1,
        SyncQuality::Ntp => 2,
        SyncQuality::Fallback => 3,
    }
}

#[cfg(feature = "ptp")]
fn num_to_quality(n: u8) -> SyncQuality {
    match n {
        1 => SyncQuality::Ptp,
        2 => SyncQuality::Ntp,
        _ => SyncQuality::Fallback,
    }
}

// Touches the OS clock (via `now()`) — out of miri scope (spec §Gate plan).
#[cfg(all(test, feature = "std", not(miri)))]
mod tests {
    use crate::clock::Clock;
    use crate::quality::{ClockSource, SyncQuality};

    /// The advisory surface is total: `quality()` returns a classification
    /// (and caches it — second call hits the same value), `source()` reports
    /// the active reference path. REQ-CLK-004/003 observability.
    /// Reads the OS clock — out of miri scope (spec §Gate plan).
    #[cfg(not(miri))]
    #[test]
    fn advisory_surface_is_total_and_cached() {
        let clock = crate::SystemClock::new();
        let q1 = clock.quality();
        let q2 = clock.quality();
        assert_eq!(q1, q2, "cached quality must be stable");
        assert!(matches!(
            q1,
            SyncQuality::Ptp | SyncQuality::Ntp | SyncQuality::Fallback
        ));

        #[cfg(feature = "ptp")]
        assert_eq!(q1, crate::ptp::detect_sync_quality());
        #[cfg(not(feature = "ptp"))]
        assert_eq!(q1, SyncQuality::Fallback);

        #[cfg(feature = "std")]
        assert_eq!(clock.source(), ClockSource::Instant);
        assert!(matches!(
            clock.source(),
            ClockSource::Instant | ClockSource::LibcMono
        ));

        // now() remains monotone alongside the advisory calls.
        let a = clock.now();
        let b = clock.now();
        assert!(b.mono >= a.mono);
        assert!(b.wall.unix_nanos() >= a.wall.unix_nanos() || a.mono == 0);
    }
}
