//! Deterministic mock clock — REQ-CLK-005 (G3).
//!
//! [`MockClock`] never consults the OS. Time advances only when the test
//! advances it, so property tests and replay are reproducible
//! bit-for-bit. Wall and monotonic fields move independently:
//! [`step_wall`](MockClock::step_wall) simulates NTP/PTP wall steps for
//! skew tests without touching monotonic time.

use crate::clock::Clock;
use crate::quality::{ClockSource, SyncQuality};
use crate::timestamp::{Timestamp, WallTime};
#[cfg(not(target_has_atomic = "64"))]
use core::cell::Cell;
#[cfg(target_has_atomic = "64")]
use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use core::time::Duration;
#[cfg(not(target_has_atomic = "64"))]
use critical_section::Mutex;

/// Monotonic cell: saturating-add only — a mock cannot regress by
/// construction. Atomic on 64-bit-atomic targets, critical-section cell
/// otherwise (chronoshift pattern; impl provided by the final binary).
#[derive(Debug)]
struct MonoCell {
    #[cfg(target_has_atomic = "64")]
    ns: AtomicU64,
    #[cfg(not(target_has_atomic = "64"))]
    ns: Mutex<Cell<u64>>,
}

impl MonoCell {
    const fn new(start_ns: u64) -> Self {
        Self {
            #[cfg(target_has_atomic = "64")]
            ns: AtomicU64::new(start_ns),
            #[cfg(not(target_has_atomic = "64"))]
            ns: Mutex::new(Cell::new(start_ns)),
        }
    }

    fn saturating_add(&self, dt: u64) {
        #[cfg(target_has_atomic = "64")]
        {
            // CAS loop keeps the advance saturating instead of wrapping.
            let mut cur = self.ns.load(Ordering::Relaxed);
            loop {
                let next = cur.saturating_add(dt);
                match self
                    .ns
                    .compare_exchange_weak(cur, next, Ordering::AcqRel, Ordering::Relaxed)
                {
                    Ok(_) => return,
                    Err(seen) => cur = seen,
                }
            }
        }
        #[cfg(not(target_has_atomic = "64"))]
        {
            critical_section::with(|cs| {
                let c = self.ns.borrow(cs);
                c.set(c.get().saturating_add(dt));
            });
        }
    }

    fn load(&self) -> u64 {
        #[cfg(target_has_atomic = "64")]
        {
            self.ns.load(Ordering::Acquire)
        }
        #[cfg(not(target_has_atomic = "64"))]
        {
            critical_section::with(|cs| self.ns.borrow(cs).get())
        }
    }
}

/// Wall cell: signed, steps freely backward/forward (that is the point of
/// [`MockClock::step_wall`]). Saturates at `i64` bounds.
#[derive(Debug)]
struct WallCell {
    #[cfg(target_has_atomic = "64")]
    ns: AtomicI64,
    #[cfg(not(target_has_atomic = "64"))]
    ns: Mutex<Cell<i64>>,
}

impl WallCell {
    const fn new(start_ns: i64) -> Self {
        Self {
            #[cfg(target_has_atomic = "64")]
            ns: AtomicI64::new(start_ns),
            #[cfg(not(target_has_atomic = "64"))]
            ns: Mutex::new(Cell::new(start_ns)),
        }
    }

    fn saturating_add(&self, dt: i64) {
        #[cfg(target_has_atomic = "64")]
        {
            let mut cur = self.ns.load(Ordering::Relaxed);
            loop {
                let next = cur.saturating_add(dt);
                match self
                    .ns
                    .compare_exchange_weak(cur, next, Ordering::AcqRel, Ordering::Relaxed)
                {
                    Ok(_) => return,
                    Err(seen) => cur = seen,
                }
            }
        }
        #[cfg(not(target_has_atomic = "64"))]
        {
            critical_section::with(|cs| {
                let c = self.ns.borrow(cs);
                c.set(c.get().saturating_add(dt));
            });
        }
    }

    fn load(&self) -> i64 {
        #[cfg(target_has_atomic = "64")]
        {
            self.ns.load(Ordering::Acquire)
        }
        #[cfg(not(target_has_atomic = "64"))]
        {
            critical_section::with(|cs| self.ns.borrow(cs).get())
        }
    }
}

/// Deterministic test clock (REQ-CLK-005).
///
/// - Advances **only** by explicit calls — the OS is never consulted (G3).
/// - `mono` is additive-only and can never go backward.
/// - `wall` is a separate signed field; [`step_wall`](MockClock::step_wall)
///   moves it without touching `mono`, simulating NTP slews and PTP steps.
/// - Shared-handle use is safe: cells are atomic (or critical-section)
///   backed, and single-threaded use — the spec's determinism contract — is
///   exactly reproducible.
///
/// The process-wide monotonic floor is deliberately **not** applied here
/// (see `crate::monotonic`): ambient real-clock reads elsewhere in a test
/// binary must not perturb a fiction.
///
/// # Example
///
/// ```
/// use clock_kit::{Clock, ClockSource, MockClock};
/// use std::time::Duration;
///
/// let clock = MockClock::start_at(10, 1_000_000);
/// assert_eq!(clock.now().mono, 10);
/// clock.advance(Duration::from_nanos(90));
/// assert_eq!(clock.now().mono, 100);
/// clock.step_wall(-500_000); // wall steps, mono does not
/// assert_eq!(clock.now().mono, 100);
/// assert_eq!(clock.source(), ClockSource::Mock);
/// ```
#[derive(Debug)]
pub struct MockClock {
    mono: MonoCell,
    wall: WallCell,
}

impl MockClock {
    /// A mock starting at `mono_ns` / `wall_ns` (both nanos; wall is
    /// signed-epoch, mono is process-local scale).
    #[must_use]
    pub fn start_at(mono_ns: u64, wall_ns: u64) -> Self {
        Self {
            mono: MonoCell::new(mono_ns),
            wall: WallCell::new(clamp_i64(wall_ns)),
        }
    }

    /// A mock starting everything at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::start_at(0, 0)
    }

    /// Advance mono (and mono only) by `dt`. Deterministic.
    pub fn advance(&self, dt: Duration) {
        let ns = u64::try_from(dt.as_nanos()).unwrap_or(u64::MAX);
        self.mono.saturating_add(ns);
    }

    /// Advance mono by `ns` nanoseconds. Deterministic.
    pub fn advance_nanos(&self, ns: u64) {
        self.mono.saturating_add(ns);
    }

    /// Step the wall field by `offset_ns` (negative = backward, e.g. an
    /// NTP correction) without touching mono. REQ-CLK-005 skew tests.
    pub fn step_wall(&self, offset_ns: i64) {
        self.wall.saturating_add(offset_ns);
    }

    /// Current wall reading, epoch nanos.
    #[must_use]
    pub fn wall_nanos(&self) -> u64 {
        let ns = self.wall.load();
        u64::try_from(ns).unwrap_or(0)
    }

    /// Current monotonic reading, nanos.
    #[must_use]
    pub fn mono_nanos(&self) -> u64 {
        self.mono.load()
    }
}

impl Default for MockClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MockClock {
    fn now(&self) -> Timestamp {
        Timestamp {
            wall: WallTime::from_unix_nanos(self.wall_nanos()),
            mono: self.mono_nanos(),
        }
    }

    fn quality(&self) -> SyncQuality {
        // A fiction is not a synchronized source; fail-closed (G2).
        SyncQuality::Fallback
    }

    fn source(&self) -> ClockSource {
        ClockSource::Mock
    }
}

/// Wall starts are `u64` per the API sketch; values above `i64::MAX`
/// (year 2942 in wall nanos) saturate instead of wrapping.
// The guarded cast below is wrap-safe by the branch.
#[allow(clippy::cast_possible_wrap)]
const fn clamp_i64(wall_ns: u64) -> i64 {
    if wall_ns > i64::MAX as u64 {
        i64::MAX
    } else {
        wall_ns as i64
    }
}

#[cfg(test)]
mod tests {
    use super::{MockClock, WallTime};
    use crate::clock::Clock;
    use crate::quality::{ClockSource, SyncQuality};
    use core::time::Duration;

    /// Deterministic fiction: identical operation sequences produce
    /// identical timestamps (G3). Pure logic — runs under miri.
    #[test]
    fn mock_sequence_is_reproducible() {
        let run = || {
            let c = MockClock::start_at(1_000, 5_000);
            c.advance(Duration::from_millis(2));
            c.advance_nanos(500);
            c.step_wall(-1_234);
            c.now()
        };
        let a = run();
        let b = run();
        assert_eq!(a, b);
        assert_eq!(a.mono, 2_000_500 + 1_000);
        assert_eq!(a.wall, WallTime::from_unix_nanos(5_000 - 1_234));
    }

    /// `step_wall` moves wall without touching mono (NTP/PTP skew tests).
    #[test]
    fn mock_step_wall_independent_of_mono() {
        let c = MockClock::start_at(0, 1_000_000_000);
        c.advance_nanos(10);
        c.step_wall(-2_000_000_000); // big backward step
        let t = c.now();
        assert_eq!(t.mono, 10);
        assert_eq!(t.wall.unix_nanos(), 0); // clamped, never negative
        assert_ne!(t.wall.unix_nanos(), t.mono);
    }

    /// Saturating bounds: a mock must not wrap or panic at extremes.
    #[test]
    fn mock_saturates_at_bounds() {
        let c = MockClock::start_at(u64::MAX - 5, 0);
        c.advance_nanos(100);
        assert_eq!(c.mono_nanos(), u64::MAX);
        c.step_wall(i64::MIN);
        c.step_wall(i64::MIN);
        assert_eq!(c.wall_nanos(), 0);
    }

    /// Clock trait surface: mock reports Mock/Fallback.
    #[test]
    fn mock_reports_source_and_quality() {
        let c = MockClock::new();
        assert_eq!(c.source(), ClockSource::Mock);
        assert_eq!(c.quality(), SyncQuality::Fallback);
        assert_eq!(c.now().mono, 0);
    }
}
