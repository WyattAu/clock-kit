//! Process-wide monotonic floor — REQ-CLK-001 (G1).
//!
//! Every real (non-mock) clock implementation clamps its `mono` through
//! this floor: a process-wide `AtomicU64` running `fetch_max`, so even a
//! misbehaving or poorly-synchronized source is clamped to the highest
//! value ever returned. On targets without 64-bit atomics the floor falls
//! back to a critical-section cell (the chronoshift `MockClock` pattern;
//! the `critical-section` impl is provided by the final binary per
//! critical-section convention).
//!
//! The floor is deliberately *not* applied to [`MockClock`](crate::MockClock):
//! G3 (deterministic fiction) wins there — a mock must not be perturbed by
//! ambient reads from other tests, and its mono cannot regress by
//! construction anyway (advancement is additive-only).
//!
//! Custom [`Clock`](crate::Clock) implementations should route their mono
//! through [`mono_floor`] to honor REQ-CLK-001 by construction.

#[cfg(all(not(loom), target_has_atomic = "64"))]
use core::sync::atomic::{AtomicU64, Ordering};
#[cfg(loom)]
use loom::sync::atomic::{AtomicU64, Ordering};

#[cfg(all(not(target_has_atomic = "64"), not(loom)))]
use core::cell::Cell;
#[cfg(all(not(target_has_atomic = "64"), not(loom)))]
use critical_section::Mutex;

#[cfg(not(loom))]
static MONO_FLOOR: Floor = Floor::new();

/// One floor cell: atomic max on 64-bit-atomic targets, critical-section
/// cell otherwise. Split out (operating on a fresh cell) so loom can model
/// the exact production logic per interleaving.
pub(crate) struct Floor {
    #[cfg(target_has_atomic = "64")]
    cell: AtomicU64,
    #[cfg(not(target_has_atomic = "64"))]
    cell: Mutex<Cell<u64>>,
}

impl Floor {
    // loom's AtomicU64::new is not const — only the real build needs a
    // const constructor (for the process-wide static).
    #[cfg(loom)]
    pub(crate) fn new() -> Self {
        Self {
            #[cfg(target_has_atomic = "64")]
            cell: AtomicU64::new(0),
            #[cfg(not(target_has_atomic = "64"))]
            cell: Mutex::new(Cell::new(0)),
        }
    }

    #[cfg(not(loom))]
    pub(crate) const fn new() -> Self {
        Self {
            #[cfg(target_has_atomic = "64")]
            cell: AtomicU64::new(0),
            #[cfg(not(target_has_atomic = "64"))]
            cell: Mutex::new(Cell::new(0)),
        }
    }

    /// Clamp `x` to at least the highest value this floor has ever handed
    /// out, and record it. Returned values are globally non-decreasing:
    /// the cell only ever moves up, and each call returns the cell's value
    /// at (or raised to) its own moment.
    pub(crate) fn clamp(&self, x: u64) -> u64 {
        #[cfg(target_has_atomic = "64")]
        {
            let prev = self.cell.fetch_max(x, Ordering::AcqRel);
            if x > prev {
                x
            } else {
                prev
            }
        }
        #[cfg(not(target_has_atomic = "64"))]
        {
            critical_section::with(|cs| {
                let c = self.cell.borrow(cs);
                let prev = c.get();
                let v = if x > prev { x } else { prev };
                c.set(v);
                v
            })
        }
    }

    /// Highest value ever handed out (observability; not a clock read).
    #[cfg(test)]
    pub(crate) fn peek(&self) -> u64 {
        #[cfg(target_has_atomic = "64")]
        {
            self.cell.load(Ordering::Acquire)
        }
        #[cfg(not(target_has_atomic = "64"))]
        {
            critical_section::with(|cs| self.cell.borrow(cs).get())
        }
    }
}

/// Clamp a monotonic reading through the **process-wide** floor.
///
/// Custom [`Clock`](crate::Clock) implementations call this on their mono
/// readings so the process-wide guarantee holds for their source too:
/// the returned value never goes backward across calls within a process,
/// even when the underlying source regresses. REQ-CLK-001.
#[must_use]
pub fn mono_floor(mono: u64) -> u64 {
    // Under loom only `Floor::clamp` is modeled (fresh cell per
    // interleaving); the process-wide static is a normal build artifact.
    #[cfg(loom)]
    {
        let _ = mono;
        0
    }
    #[cfg(not(loom))]
    {
        MONO_FLOOR.clamp(mono)
    }
}

#[cfg(all(test, loom))]
mod loom_tests {
    use super::Floor;

    /// Loom model: concurrent clamps through one floor. Tier-A
    /// concurrency-primitive gate for the lock-free atomic max
    /// (spec §Gate plan). Every handed-out value must be ≥ the attempted
    /// value, and the final floor must equal the max of everything seen.
    #[test]
    fn loom_floor_fetch_max_never_decreases() {
        loom::model(|| {
            let floor = loom::sync::Arc::new(Floor::new());
            let f1 = loom::sync::Arc::clone(&floor);
            let f2 = loom::sync::Arc::clone(&floor);
            let left = loom::sync::Arc::new(loom::sync::atomic::AtomicU64::new(0));
            let left2 = loom::sync::Arc::clone(&left);
            let right = loom::sync::Arc::new(loom::sync::atomic::AtomicU64::new(0));
            let right2 = loom::sync::Arc::clone(&right);

            let t1 = loom::thread::spawn(move || {
                for x in [7_u64, 3, 9] {
                    let v = f1.clamp(x);
                    assert!(v >= x);
                    left.fetch_max(v, loom::sync::atomic::Ordering::AcqRel);
                }
            });
            let t2 = loom::thread::spawn(move || {
                for x in [8_u64, 1, 5] {
                    let v = f2.clamp(x);
                    assert!(v >= x);
                    right.fetch_max(v, loom::sync::atomic::Ordering::AcqRel);
                }
            });

            // A panicking worker model means a broken invariant.
            assert!(t1.join().is_ok(), "worker 1 panicked");
            assert!(t2.join().is_ok(), "worker 2 panicked");

            // Floor holds the max of everything attempted, and each
            // thread's best handout never exceeded it.
            assert_eq!(floor.peek(), 9);
            assert!(left2.load(loom::sync::atomic::Ordering::Acquire) <= 9);
            assert!(right2.load(loom::sync::atomic::Ordering::Acquire) <= 9);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{mono_floor, Floor};

    /// Single-threaded clamp semantics: regressions are held at the floor,
    /// advances raise it. (Runs under miri — pure logic.)
    #[test]
    fn floor_holds_regressions_single_threaded() {
        let floor = Floor::new();
        assert_eq!(floor.clamp(100), 100);
        assert_eq!(floor.clamp(50), 100); // regression held
        assert_eq!(floor.clamp(100), 100); // equal passes through
        assert_eq!(floor.clamp(200), 200); // advance raises
        assert_eq!(floor.clamp(199), 200); // held again
        assert_eq!(floor.peek(), 200); // observability agrees
    }

    /// The process-wide global floor clamps an explicitly regressing
    /// reading to the highest value ever handed out (REQ-CLK-001's core
    /// promise, exercised through the public entry point).
    #[test]
    fn global_floor_clamps_regressions() {
        let high = mono_floor(u64::MAX - 3);
        assert_eq!(high, u64::MAX - 3);
        assert_eq!(mono_floor(5), u64::MAX - 3, "regression must clamp");
        assert_eq!(mono_floor(u64::MAX), u64::MAX);
        assert_eq!(mono_floor(1), u64::MAX);
    }
}
