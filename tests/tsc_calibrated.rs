//! REQ-CLK-003 traceability: `tsc_read_matches_instant_within_tolerance`.
//!
//! Arch-gated integration: compiled and *executed* where the counter
//! exists (`x86_64`, aarch64); elsewhere the `cfg` skips it entirely
//! (per-target coverage methodology, spec §Gate plan).

#![cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
#![forbid(unsafe_code)]

use clock_kit::calibrated::{calibrate, CalibratedClock};
use clock_kit::{Clock, ClockSource};
use std::time::{Duration, Instant};

/// Calibration sanity: TSC/CNTVCT-derived elapsed must track `Instant`
/// elapsed within tolerance; the calibrated clock falls back cleanly where
/// the counter is judged unusable.
#[test]
fn tsc_read_matches_instant_within_tolerance() {
    let clock = CalibratedClock::new(50_000);
    if clock.source() == ClockSource::Instant || clock.source() == ClockSource::LibcMono {
        // Counter judged unusable on this host (no invariant-TSC bit or
        // unstable rate): the fallback contract IS the behavior under
        // test — documented skip, matching the spec's "skipped where
        // unsupported".
        eprintln!("skipping TSC comparison: calibrated clock fell back");
        return;
    }

    // Calibration is repeatable (the drift-bound contract).
    assert!(calibrate(50_000).is_ok());

    let start = clock.now();
    let instant_start = Instant::now();
    std::thread::sleep(Duration::from_millis(20));
    let instant_elapsed = instant_start.elapsed();
    let end = clock.now();

    let inst_ns = u64::try_from(instant_elapsed.as_nanos()).unwrap_or(u64::MAX);
    let tsc_elapsed = end.mono - start.mono;

    // Tolerance: both paths measure the same interval; the calibrated
    // counter must land within 10% of Instant plus 1 ms of absolute
    // slack for sleep scheduling jitter.
    let tol = instant_elapsed.mul_f64(0.10) + Duration::from_millis(1);
    let tsc = Duration::from_nanos(tsc_elapsed);
    // Both subtractions are guarded by the branch — no underflow possible.
    #[allow(clippy::unchecked_time_subtraction)]
    let (diff, bigger) = if tsc > instant_elapsed {
        (tsc - instant_elapsed, "counter")
    } else {
        (instant_elapsed - tsc, "instant")
    };
    assert!(
        diff <= tol,
        "{bigger} ran long: counter {tsc_elapsed} ns vs instant {inst_ns} ns (diff {diff:?})"
    );

    // Source observability (REQ-CLK-003): report which counter serves.
    assert!(
        clock.source() == ClockSource::Tsc || clock.source() == ClockSource::Cntvct,
        "calibrated clock must report its counter"
    );
}
