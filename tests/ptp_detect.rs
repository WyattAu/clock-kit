//! REQ-CLK-004 traceability: `ptp_detection_classifies_synthetic_sysfs`.
//!
//! Detection is exercised against fixture sysfs trees through the
//! injectable-root seam; host-dependent TAI state is never assumed —
//! assertions are deterministic on every host.

#![cfg(feature = "ptp")]
#![forbid(unsafe_code)]

use clock_kit::ptp::{classify_sync_quality, detect_sync_quality_in, tai_offset_ns};
use clock_kit::SyncQuality;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

/// Unique fixture root per call (no tempfile dependency — L1-lean).
fn fixture_root(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("clock-kit-ptp-{}-{}-{tag}", std::process::id(), n));
    fs::create_dir_all(&dir).expect_or_fail("fixture mkdir");
    dir
}

trait ExpectOrFail<T> {
    fn expect_or_fail(self, what: &str) -> T;
}
impl<T> ExpectOrFail<T> for std::io::Result<T> {
    fn expect_or_fail(self, what: &str) -> T {
        match self {
            Ok(v) => v,
            // Test-only failure abort: an assert_eq-style unwind.
            #[allow(clippy::panic)]
            Err(e) => panic!("{what} failed: {e}"),
        }
    }
}

/// REQ-CLK-004: with a PTP device in the fixture tree, classification is
/// Ptp exactly when the host's TAI offset is coherent — the AND gate that
/// keeps a holdover PTP device from masquerading as synchronized.
#[test]
fn ptp_detection_classifies_synthetic_sysfs() {
    let tai_coherent = clock_kit::ptp::tai_offset_coherent();

    // Fixture: /sys/class/ptp/ptp0 present.
    let root = fixture_root("device");
    fs::create_dir_all(root.join("ptp0")).expect_or_fail("ptp0 mkdir");

    let q = detect_sync_quality_in(&root);
    assert_eq!(
        q == SyncQuality::Ptp,
        tai_coherent,
        "device+coherent TAI must be Ptp; device alone must not"
    );

    // Empty tree: never Ptp.
    let empty = fixture_root("empty");
    let q = detect_sync_quality_in(&empty);
    assert_ne!(q, SyncQuality::Ptp);

    // Malformed layout: junk names are ignored (minimal parsing), never
    // counted as devices — and can never promote the classification.
    let junk = fixture_root("junk");
    for name in ["ptp", "ptpX", "README", "ptp0a"] {
        fs::create_dir_all(junk.join(name)).expect_or_fail("junk mkdir");
    }
    let q = detect_sync_quality_in(&junk);
    assert_ne!(q, SyncQuality::Ptp, "junk names are not PTP devices");

    // Missing root: best-effort — no error, classified Fallback-grade
    // (device absent), never a panic.
    let missing = PathBuf::from("/nonexistent/clock-kit-probe");
    let q = detect_sync_quality_in(&missing);
    assert_ne!(q, SyncQuality::Ptp);
}

/// The pure policy table, pinned (see also the lib unit test — this is the
/// public-API seam consumers can pin against).
#[test]
fn classify_policy_is_fail_closed() {
    assert_eq!(
        classify_sync_quality(true, true, false),
        SyncQuality::Ptp,
        "device + coherent TAI"
    );
    assert_eq!(
        classify_sync_quality(true, false, true),
        SyncQuality::Fallback,
        "device without coherent TAI is holdover-grade ambiguity"
    );
    assert_eq!(classify_sync_quality(false, true, false), SyncQuality::Ntp);
    assert_eq!(classify_sync_quality(false, false, true), SyncQuality::Ntp);
    assert_eq!(
        classify_sync_quality(false, false, false),
        SyncQuality::Fallback
    );
}

/// `tai_offset_ns` is either Some(sane i64) or None — never a panic — and
/// two probes on a quiet host agree closely.
#[test]
fn tai_probe_is_total_and_stable() {
    let a = tai_offset_ns();
    if let Some(a) = a {
        let b = tai_offset_ns().unwrap_or(a);
        assert!(
            (a - b).abs() <= 1_000_000_000,
            "TAI probes must agree: {a} vs {b}"
        );
    }
}
