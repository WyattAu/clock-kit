//! REQ-CLK-005 traceability: `mock_advance_is_deterministic_and_step_wall_works`.

#![forbid(unsafe_code)]

use clock_kit::{Clock, MockClock};
use std::time::Duration;

/// REQ-CLK-005: advances only by explicit manual calls, fully
/// deterministic under single-threaded use, and supports simulated wall
/// steps for skew tests.
#[test]
fn mock_advance_is_deterministic_and_step_wall_works() {
    // Determinism: the same script applied to two fresh mocks yields
    // bit-identical timestamps.
    let script = |c: &MockClock| {
        c.advance(Duration::from_millis(250));
        c.advance_nanos(1);
        c.advance(Duration::from_secs(1));
        c.step_wall(50_000_000_000);
        c.advance_nanos(999);
        c.step_wall(-75_000_000_000);
        c.now()
    };
    let a = script(&MockClock::start_at(1_000, 1_000_000_000));
    let b = script(&MockClock::start_at(1_000, 1_000_000_000));
    assert_eq!(a, b, "same script must reproduce exactly (G3)");
    assert_eq!(a.mono, 1_000 + 250_000_000 + 1 + 1_000_000_000 + 999);
    // Wall: 1e9 + 50e9, then stepped back 75e9 → negative → clamped to 0.
    assert_eq!(a.wall.unix_nanos(), 0);

    // Advancement is manual only: a mock left alone never moves.
    let idle = MockClock::start_at(42, 43);
    let t1 = idle.now();
    std::thread::sleep(Duration::from_millis(2));
    let t2 = idle.now();
    assert_eq!(t1, t2, "the OS must never advance a mock");

    // step_wall moves wall independently of mono (NTP slew / PTP step).
    let c = MockClock::start_at(0, 10);
    c.step_wall(-100); // wall steps backward past zero — clamps at 0
    c.advance_nanos(5);
    let t = c.now();
    assert_eq!(t.mono, 5);
    assert_eq!(t.wall.unix_nanos(), 0);
}

/// Duration conversion saturates instead of panicking at extremes.
#[test]
fn mock_advance_saturates() {
    let c = MockClock::new();
    c.advance(Duration::from_secs(u64::MAX));
    let t1 = c.now().mono;
    c.advance(Duration::from_secs(u64::MAX));
    assert_eq!(c.now().mono, t1, "saturates at u64::MAX, never wraps");
}
