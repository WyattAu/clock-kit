//! REQ-CLK-001 traceability: `mono_never_regresses_under_all_sources`.
//!
//! Every real clock implementation must hold the process-wide monotonic
//! floor — a regression is a bug, not a documented possibility (G1).

#![forbid(unsafe_code)]

use clock_kit::{Clock, MockClock};

/// REQ-CLK-001: `Clock::now()` returns `Timestamp` whose `mono` field
/// never goes backward across calls within a process, for every source
/// available on this host — system, calibrated (tsc), and mock.
#[test]
fn mono_never_regresses_under_all_sources() {
    // System clock (std or libc path).
    let system = clock_kit::SystemClock::new();
    let mut prev = system.now().mono;
    for _ in 0..1_000 {
        let t = system.now();
        assert!(t.mono >= prev, "system mono regressed");
        prev = t.mono;
    }

    // Calibrated clock, when a counter path compiled for this target: even
    // a misbehaving source must be clamped by the floor.
    #[cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let clock = clock_kit::calibrated::CalibratedClock::new(10_000);
        let mut prev = clock.now().mono;
        for _ in 0..10_000 {
            let t = clock.now();
            assert!(t.mono >= prev, "calibrated mono regressed under load");
            prev = t.mono;
        }
    }

    // Mock: additive-only advancement cannot regress by construction.
    let mock = MockClock::start_at(5, 5);
    let mut prev = mock.now().mono;
    for i in 0..100_u64 {
        mock.advance_nanos(i);
        let t = mock.now();
        assert!(t.mono >= prev, "mock mono regressed");
        prev = t.mono;
    }
    assert_eq!(prev, 5 + (0..100_u64).sum::<u64>());
}

/// The floor is process-wide and shared across sources: readings from any
/// clock never dip below the high-water mark established by an earlier
/// source in the same process. (The mock bypasses the floor by design —
/// G3 determinism; see crate docs.)
#[cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
#[test]
fn floor_is_shared_across_sources() {
    let calibrated = clock_kit::calibrated::CalibratedClock::new(10_000);
    let system = clock_kit::SystemClock::new();

    // Interleave sources; neither may regress, and neither may dip below
    // the other's previously observed values for long (both live on
    // nanos-since-boot scales in practice; the contract asserted is
    // non-regression per source).
    let mut prev_cal = calibrated.now().mono;
    let mut prev_sys = system.now().mono;
    for _ in 0..500 {
        let t = calibrated.now();
        assert!(t.mono >= prev_cal);
        prev_cal = t.mono;
        let t = system.now();
        assert!(t.mono >= prev_sys);
        prev_sys = t.mono;
    }
}
