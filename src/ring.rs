//! Fixed-capacity latency statistics — REQ-CLK-006.
//!
//! [`LatencyRing`] is a sliding window of `u64` nanos: [`record`] is O(1)
//! and evicts the oldest sample when capacity is exceeded; [`stats`] is
//! **exact over the current window** with zero heap allocation — it sorts
//! a stack-resident copy (CAP × 8 B scratch; default 4096 → 32 KiB, chosen
//! for ≥ 64 KiB stacks) and reports min/max/mean/p50/p99/p99.9.
//!
//! Percentiles use the **nearest-rank** algorithm, re-implemented lean
//! from the percentile-kit pattern: percentile-kit is L2, and an L1 crate
//! must not depend upward (estate layers.md rule 1) — so the algorithm is
//! copied, not linked. No recursion anywhere (guaranteed for constrained
//! stacks): the sort is an iterative heapsort.
//!
//! [`record`]: LatencyRing::record
//! [`stats`]: LatencyRing::stats

/// Exact interval statistics over the current window (nanosecond
/// resolution). All zeros for an empty ring. REQ-CLK-006.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct IntervalStats {
    /// Smallest sample in the window.
    pub min: u64,
    /// Largest sample in the window.
    pub max: u64,
    /// Arithmetic mean, integer floor.
    pub mean: u64,
    /// 50th percentile (nearest rank).
    pub p50: u64,
    /// 99th percentile (nearest rank).
    pub p99: u64,
    /// 99.9th percentile (nearest rank).
    pub p999: u64,
}

/// Fixed-capacity (const-generic) ring of `u64` nanos — a sliding window,
/// not an accumulator. Allocation-free on every path: `stats()` works from
/// a stack-resident scratch copy sized by `CAP` (default 4096 → 32 KiB —
/// document your stack when embedding with larger capacities). REQ-CLK-006.
#[derive(Debug)]
pub struct LatencyRing<const CAP: usize = 4096> {
    buf: [u64; CAP],
    /// Index of the oldest live sample (next write slot).
    head: usize,
    /// Live samples, ≤ CAP.
    len: usize,
}

impl<const CAP: usize> LatencyRing<CAP> {
    /// An empty ring.
    #[must_use]
    pub const fn new() -> Self {
        // Const-eval guard: zero capacity would deadlock the ring math.
        const {
            assert!(CAP > 0, "LatencyRing capacity must be non-zero");
        }
        Self {
            buf: [0; CAP],
            head: 0,
            len: 0,
        }
    }

    /// Record one interval. O(1); when the window is full the oldest
    /// sample is evicted. REQ-CLK-006.
    pub fn record(&mut self, interval_ns: u64) {
        // Invariant: head < CAP (wraps below), len ≤ CAP — the index below
        // is always in bounds.
        #[allow(clippy::indexing_slicing)]
        {
            self.buf[self.head] = interval_ns;
        }
        self.head += 1;
        if self.head == CAP {
            self.head = 0;
        }
        if self.len < CAP {
            self.len += 1;
        }
    }

    /// Exact statistics over the current window. Zero heap allocation;
    /// scratch is a stack-resident `CAP`-sized copy (heapsorted in place —
    /// iterative, no recursion). REQ-CLK-006.
    // Indexing below is guarded by two invariants, documented inline:
    // (a) ring indices wrap below CAP; (b) only `0..written` is read, and
    // `written == self.len ≤ CAP == scratch.len()`.
    #[allow(clippy::indexing_slicing)]
    #[must_use]
    pub fn stats(&self) -> IntervalStats {
        if self.len == 0 {
            return IntervalStats::default();
        }
        // Live samples in recording order (oldest first), then sorted.
        // Oldest index: `head` when the window is full (it wrapped onto
        // the oldest slot); while still filling, data starts at 0.
        let mut start = self.head + CAP - self.len;
        if start >= CAP {
            start -= CAP;
        }
        let mut scratch = [0_u64; CAP];
        let mut written = 0;
        for i in 0..self.len {
            // (a) (start + i) mod CAP, branch instead of %.
            let idx = start + i;
            let idx = if idx >= CAP { idx - CAP } else { idx };
            scratch[written] = self.buf[idx];
            written += 1;
        }
        let live = &mut scratch[..written];
        heapsort(live);

        let n = self.len;
        let mut sum = 0_u128;
        let mut min = u64::MAX;
        let mut max = 0_u64;
        for &v in live.iter() {
            sum += u128::from(v);
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
        }
        IntervalStats {
            min,
            max,
            // Integer floor mean; sum of ≤ CAP u64s cannot exceed u128.
            #[allow(clippy::cast_possible_truncation)] // mean < 2^64
            mean: (sum / n as u128) as u64,
            p50: nearest_rank(live, n, 50, 100),
            p99: nearest_rank(live, n, 99, 100),
            p999: nearest_rank(live, n, 999, 1_000),
        }
    }

    /// Live samples in the window (≤ CAP).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// True when nothing has been recorded.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Clear the window (reuses the allocation-free buffer).
    pub fn reset(&mut self) {
        self.head = 0;
        self.len = 0;
    }
}

impl<const CAP: usize> Default for LatencyRing<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

/// Nearest-rank percentile: rank `ceil(num/den · n)`, 1-based, on a
/// sorted window. Percentile-kit's algorithm, re-implemented lean —
/// percentile-kit is L2 and cannot be a dependency of this L1 crate
/// (estate layers.md rule 1). REQ-CLK-006.
fn nearest_rank(sorted: &[u64], n: usize, num: usize, den: usize) -> u64 {
    // rank = ceil(n·num/den); n·num overflows usize only far past any
    // legal CAP — saturating regardless.
    let rank = (n.saturating_mul(num).saturating_add(den - 1)) / den;
    let idx = rank.saturating_sub(1).min(n - 1);
    // Invariant: 1 ≤ rank ≤ n, so idx < n == sorted.len().
    #[allow(clippy::indexing_slicing)]
    sorted[idx]
}

/// Iterative in-place heapsort (no recursion — the risk register requires
/// it for constrained stacks), no allocation. Sorts ascending.
// Indexing invariants: sift_down only touches indices proven < end (the
// child bounds are checked immediately above each access).
#[allow(clippy::indexing_slicing)]
fn heapsort(v: &mut [u64]) {
    let n = v.len();
    if n < 2 {
        return;
    }
    // Build max-heap: sift down from the last parent.
    let mut start = n / 2;
    while start > 0 {
        start -= 1;
        sift_down(v, start, n);
    }
    // Extract: swap root with end, shrink, re-heapify.
    let mut end = n;
    while end > 1 {
        end -= 1;
        v.swap(0, end);
        sift_down(v, 0, end);
    }
}

// Indexing invariants: `left`, `left + 1`, `child`, `r` are each guarded by
// `left < end` / `left + 1 < end` checks; `child ≤ left + 1 < end ≤ len`.
#[allow(clippy::indexing_slicing)]
fn sift_down(v: &mut [u64], root: usize, end: usize) {
    let mut r = root;
    loop {
        let left = 2 * r + 1;
        if left >= end {
            return;
        }
        // Invariant: left and left+1 (when < end) are valid children.
        let child = if left + 1 < end && v[left + 1] > v[left] {
            left + 1
        } else {
            left
        };
        if v[child] <= v[r] {
            return;
        }
        v.swap(r, child);
        r = child;
    }
}

#[cfg(test)]
mod tests {
    use super::{IntervalStats, LatencyRing};

    /// Small-cap exactness against a plain reference implementation —
    /// pure logic, mandated under miri (spec §Gate plan).
    #[test]
    fn ring_stats_exact_small_cap() {
        let mut ring = LatencyRing::<8>::new();
        ring.record(500);
        ring.record(100);
        ring.record(300);
        let s = ring.stats();
        assert_eq!(
            s,
            IntervalStats {
                min: 100,
                max: 500,
                mean: 300,
                p50: 300,
                p99: 500, // rank ceil(99·3/100) = 3 → 3rd of {100,300,500}
                p999: 500,
            }
        );
    }

    /// Sliding-window eviction: recording past CAP evicts the oldest; a
    /// reset empties without reallocation.
    #[test]
    fn ring_eviction_order_and_reset() {
        let mut ring = LatencyRing::<4>::new();
        for v in 1..=6_u64 {
            ring.record(v);
        }
        assert_eq!(ring.len(), 4);
        let s = ring.stats();
        // Window is {3, 4, 5, 6} — samples 1, 2 were evicted in order.
        assert_eq!(s.min, 3);
        assert_eq!(s.max, 6);
        assert_eq!(s.mean, 4);
        assert_eq!(s.p50, 4); // rank ceil(50·4/100)=2 → 2nd of sorted {3,4,5,6}

        ring.reset();
        assert!(ring.is_empty());
        assert_eq!(ring.stats(), IntervalStats::default());
    }

    /// Empty-ring and single-sample edges.
    #[test]
    fn ring_edges() {
        let mut ring = LatencyRing::<2>::new();
        assert!(ring.is_empty());
        assert_eq!(ring.stats(), IntervalStats::default());
        ring.record(u64::MAX);
        let s = ring.stats();
        assert_eq!(
            (s.min, s.max, s.mean, s.p50),
            (u64::MAX, u64::MAX, u64::MAX, u64::MAX)
        );
    }

    /// Mean floors (integer mean over u128 accumulation — no overflow at
    /// plausible sums).
    #[test]
    fn ring_mean_floors() {
        let mut ring = LatencyRing::<3>::new();
        ring.record(1);
        ring.record(1);
        ring.record(2);
        assert_eq!(ring.stats().mean, 1); // 4/3 floors to 1
    }
}
