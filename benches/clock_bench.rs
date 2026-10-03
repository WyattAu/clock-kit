//! Criterion baselines required by the spec's gate plan: `now()` cost per
//! source (TSC vs Instant vs libc vs mock), and `record()`/`stats()` cost
//! vs CAP. Run the full set with `cargo bench --all-features`.

// criterion_group! emits undocumented items — a bench-harness concern, not
// library API.
#![allow(missing_docs)]
#![forbid(unsafe_code)]

use clock_kit::{Clock, LatencyRing, MockClock};
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use std::time::Duration;

fn now_cost(c: &mut Criterion) {
    let mock = MockClock::start_at(0, 0);
    c.bench_function("now/mock", |b| b.iter(|| mock.now()));

    #[cfg(any(feature = "std", feature = "libc"))]
    {
        let system = clock_kit::SystemClock::new();
        c.bench_function("now/system", |b| b.iter(|| system.now()));
    }

    #[cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let calibrated = clock_kit::calibrated::CalibratedClock::new(200_000);
        let cal = calibrated.calibration();
        if cal.is_some() {
            c.bench_function("now/calibrated_tsc", |b| b.iter(|| calibrated.now()));
            c.bench_function("counter_read/raw", |b| b.iter(clock_kit::counter::read));
        }
    }
}

fn ring_cost(c: &mut Criterion) {
    // record() is CAP-independent (O(1)); stats() scales with CAP.
    let mut ring_small = LatencyRing::<64>::new();
    let mut ring_default = LatencyRing::<4096>::new();
    for i in 0..4_096_u64 {
        ring_small.record(i);
        ring_default.record(i);
    }

    c.bench_function("record/cap4096", |b| {
        b.iter(|| ring_default.record(black_box(1)));
    });
    c.bench_function("stats/cap64", |b| b.iter(|| ring_small.stats()));
    c.bench_function("stats/cap4096", |b| b.iter(|| ring_default.stats()));
}

fn mock_advance_cost(c: &mut Criterion) {
    let mock = MockClock::start_at(0, 0);
    c.bench_function("mock/advance", |b| {
        b.iter(|| mock.advance(black_box(Duration::from_nanos(1))));
    });
}

criterion_group!(benches, now_cost, ring_cost, mock_advance_cost);
criterion_main!(benches);
