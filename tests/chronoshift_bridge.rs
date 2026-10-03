//! chronoshift bridge — spec §Decisions #1 (recommended resolution:
//! wrap/re-export, one estate time idiom).
//!
//! With the `chronoshift` feature, every `chronoshift::Clock` is
//! automatically a `clock_kit::Clock` (blanket impl). This test pins the
//! bridge semantics.
//!
//! Note on mono assertions: the bridged path clamps through the
//! **process-wide** floor, which other tests in this binary also raise.
//! Mono is therefore asserted with the floor contract (`>=`, never
//! backward); wall assertions stay exact (no floor applies to wall).

#![cfg(feature = "chronoshift")]

use chronoshift::Clock as CsClock;
use clock_kit::{Clock, ClockSource, MockClock, SyncQuality};
use std::time::Duration;

/// A chronoshift mock plugged into clock-kit: full Timestamp plumbing,
/// floor applied, External/Fallback advisory surface.
#[test]
fn chronoshift_clocks_are_clock_kit_clocks() {
    let cs = chronoshift::mock::MockClock::new(1_000);
    let t0 = cs.now(); // clock_kit::Clock::now via the blanket impl
    assert_eq!(t0.wall.unix_nanos(), 1_000);
    assert!(t0.mono >= t0.wall.unix_nanos(), "floor may only raise mono");
    assert_eq!(Clock::source(&cs), ClockSource::External);
    assert_eq!(Clock::quality(&cs), SyncQuality::Fallback);

    cs.advance_ns(500);
    let t1 = cs.now();
    assert_eq!(t1.wall.unix_nanos(), 1_500);
    assert!(t1.mono >= t0.mono, "mono never regresses (REQ-CLK-001)");

    // The derived helpers of the L0 trait remain usable through the bridge
    // (they read the raw i64 nanos, outside our floor).
    cs.set_ns(1_500_000); // 1.5 ms
    assert_eq!(chronoshift_now_ms(&cs), 1);
}

fn chronoshift_now_ms<C: CsClock>(c: &C) -> i64 {
    c.now_ms()
}

/// The bridged path enforces REQ-CLK-001: a chronoshift clock whose time
/// moves backward (via `set_ns`) can never lower the observed mono.
#[test]
fn bridged_clock_honors_monotonic_floor() {
    let cs = chronoshift::mock::MockClock::new(10_000);
    let first = cs.now().mono;
    cs.set_ns(5_000); // deliberate regression attempt
    let after = cs.now().mono;
    assert!(
        after >= first,
        "bridged source regression must be clamped, not observed"
    );
}

/// clock-kit's own clocks remain independent of the chronoshift trait
/// (the blanket impl points one way only — no conflicting impls). Mock is
/// floor-free (G3), so absolute values are exact here.
#[test]
fn kit_clocks_are_not_chronoshift_clocks() {
    let kit = MockClock::start_at(1, 2);
    kit.advance(Duration::from_nanos(3));
    assert_eq!(kit.now().mono, 4);
    assert_eq!(kit.source(), ClockSource::Mock);
}
