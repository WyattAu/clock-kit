//! REQ-CLK-006 traceability: `interval_stats_exact_over_window_no_alloc`.
//!
//! Property test vs a sorted reference — 500 cases per the spec's
//! property gate. Three ring capacities force eviction-heavy, partially
//! filled, and exact-fit windows. (The allocation-proof harness lives in
//! its own test binary `ring_alloc.rs`: a counting global allocator sees
//! the whole process, so it must not share a process with other tests.)

use clock_kit::{IntervalStats, LatencyRing};
use proptest::prelude::*;

/// Reference implementation: sort the current window, report nearest-rank
/// percentiles — independent of the ring's internals.
fn reference_stats(window: &[u64]) -> IntervalStats {
    let mut sorted = window.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    if n == 0 {
        return IntervalStats::default();
    }
    let rank = |num: u64, den: u64| -> u64 {
        let r = (n as u128 * u128::from(num)).div_ceil(u128::from(den));
        let idx = usize::try_from(r)
            .unwrap_or(usize::MAX)
            .saturating_sub(1)
            .min(n - 1);
        sorted.get(idx).copied().unwrap_or(0)
    };
    let sum: u128 = sorted.iter().map(|&v| u128::from(v)).sum();
    IntervalStats {
        min: sorted.first().copied().unwrap_or(0),
        max: sorted.last().copied().unwrap_or(0),
        #[allow(clippy::cast_possible_truncation)] // mean < 2^64
        mean: (sum / n as u128) as u64,
        p50: rank(50, 100),
        p99: rank(99, 100),
        p999: rank(999, 1_000),
    }
}

fn check_ring<const CAP: usize>(values: &[u64]) {
    let mut ring = LatencyRing::<CAP>::new();
    for &v in values {
        ring.record(v);
    }
    let live: Vec<u64> = values
        .iter()
        .skip(values.len().saturating_sub(CAP))
        .copied()
        .collect();
    assert_eq!(ring.len(), live.len(), "window size for CAP={CAP}");
    assert_eq!(
        ring.stats(),
        reference_stats(&live),
        "stats must be exact over the last-{CAP} window"
    );
}

// REQ-CLK-006 property: over random recording sequences, ring stats are
// **exact** vs the sorted reference over the current (possibly evicted)
// window. 500 cases per the spec's property gate.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn interval_stats_exact_over_window_no_alloc(
        values in proptest::collection::vec(0_u64..=u64::MAX / 8, 0..=192),
        cap_div in 1_u8..=3,
    ) {
        // Eviction-heavy (7), mid (32), and exact-fit-plus-overflow (64).
        match cap_div {
            1 => check_ring::<7>(&values),
            2 => check_ring::<32>(&values),
            _ => check_ring::<64>(&values),
        }
    }
}
