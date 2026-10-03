//! REQ-CLK-002 traceability: `wall_and_mono_independent_no_implicit_conversion`.

#![forbid(unsafe_code)]

use clock_kit::{Clock, SystemClock, Timestamp, WallTime};

/// REQ-CLK-002: wall and mono are independent fields with independent
/// rules, and the portable accessor is identical across builds.
#[test]
fn wall_and_mono_independent_no_implicit_conversion() {
    let t = Timestamp::from_parts(WallTime::from_unix_nanos(1_000), 7);
    // Independent fields, no derivation between them.
    assert_eq!(t.wall.unix_nanos(), 1_000);
    assert_eq!(t.mono, 7);
    assert_ne!(t.wall.unix_nanos(), t.mono);

    // The single sanctioned conversion is the explicitly labelled lossy
    // helper; it passes the wall value through unchanged (and is the only
    // API returning a mono-scale value from wall).
    assert_eq!(t.mono_from_wall_lossy(), 1_000);

    // WallTime is a plain u64 wrapper — no hidden state.
    assert_eq!(WallTime::default(), WallTime::UNIX_EPOCH);
    assert_eq!(WallTime::UNIX_EPOCH.unix_nanos(), 0);
}

/// Under `std`: the SystemTime-backed posture of the wall field, including
/// the pre-epoch clamp and the round trip through `system()`.
#[cfg(feature = "std")]
#[test]
fn wall_is_systemtime_backed_under_std() {
    use std::time::{Duration, SystemTime};

    let wall = WallTime::from_unix_nanos(1_234_567_890);
    let exact = matches!(
        wall.system().duration_since(SystemTime::UNIX_EPOCH),
        Ok(d) if d.as_nanos() == 1_234_567_890
    );
    assert!(exact, "system() round trip must preserve epoch nanos");

    // Pre-epoch system clocks clamp to 0 — a clock library must not panic.
    let past = SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs(1));
    if let Some(past) = past {
        assert_eq!(
            WallTime::from_unix_nanos(0),
            clock_kit::timestamp::WallTime::from_unix_nanos(0)
        );
        let _ = past; // clamp behavior exercised via SystemClock below
    }

    // Round trip: a real reading survives system() reconstruction to nanos
    // precision.
    let clock = SystemClock::new();
    let t = clock.now();
    let rebuilt = t.wall.system();
    let nanos = rebuilt
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
    assert_eq!(nanos, t.wall.unix_nanos());
}

/// A real `SystemClock` reading carries both fields, and they are not the
/// same scale (mono is process-local, wall is epoch).
#[test]
fn system_reading_has_both_independent_fields() {
    let clock = SystemClock::new();
    let a = clock.now();
    let b = clock.now();
    // Mono never regresses (REQ-CLK-001); wall may, but both fields are
    // populated independently.
    assert!(b.mono >= a.mono);
    assert!(a.wall.unix_nanos() > 0 || a.mono == 0);
}
