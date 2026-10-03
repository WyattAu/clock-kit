//! REQ-CLK-006 allocation proof: `record`/`stats` never touch the heap.
//!
//! This is a **single-test binary on purpose**: the counting global
//! allocator observes the entire process, so any sibling test allocating
//! concurrently (proptest shrinking, etc.) would poison the zero-delta
//! assertion. In its own process, this test is alone on its thread.

use clock_kit::LatencyRing;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::alloc::{GlobalAlloc, Layout, System};

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// SAFETY: pure pass-through to the system allocator; the counter is
// bookkeeping only. This is the one sanctioned unsafe in the test estate.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: forwards to System with the identical layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwards to System with the layout from alloc.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Zero allocations across `record` and `stats` at the default capacity
/// (4096 → 32 KiB stack scratch, no heap).
#[test]
fn stats_and_record_never_allocate() {
    let mut ring = LatencyRing::<4096>::new();
    for i in 0..4_096_u64 {
        ring.record(i % 1_000);
    }
    let before = ALLOCS.load(Ordering::Relaxed);
    let stats = ring.stats();
    let mid = ALLOCS.load(Ordering::Relaxed);
    for i in 0..1_000_u64 {
        ring.record(i);
        let _ = ring.stats();
    }
    let after = ALLOCS.load(Ordering::Relaxed);

    assert_eq!(mid - before, 0, "stats() allocated");
    assert_eq!(after - mid, 0, "record()/stats() loop allocated");
    assert!(stats.mean > 0 || stats.min == 0);
    // Sanity: the window is exactly the last 4096 samples.
    assert_eq!(ring.len(), 4096);
}
