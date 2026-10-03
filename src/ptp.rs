//! Sync-quality detection — REQ-CLK-004 (feature `ptp`).
//!
//! Detects the **presence and classification** of a synchronized source:
//! `/sys/class/ptp/ptpN` device presence plus a `CLOCK_TAI` vs
//! `CLOCK_REALTIME` offset probe. Detection is read-only, best-effort, and
//! never blocks or fails the clock: it runs no daemon, opens no PHC device
//! for control, and performs no `adjtimex` writes. Anything ambiguous
//! classifies as [`SyncQuality::Fallback`] — fail-closed to distrust.
//!
//! Classification requires **both** device presence and a coherent TAI
//! offset for [`SyncQuality::Ptp`] (a labelled device alone may be in
//! holdover); NTP markers (chrony/ntpd/timesyncd) or a coherent TAI offset
//! without a PTP device yield [`SyncQuality::Ntp`].

use crate::quality::SyncQuality;
use std::path::Path;

/// Common NTP-discipline runtime markers. Existence-only checks — we parse
/// nothing (minimal parsing per the spec's fragility risk entry).
const NTP_MARKERS: [&str; 4] = [
    "/run/systemd/timesync/synchronized",
    "/run/chrony/chronyd.pid",
    "/var/run/ntpd.pid",
    "/run/ntpd.pid",
];

/// Classify the host's time quality.
///
/// Reads `/sys/class/ptp` and probes `CLOCK_TAI`. Best-effort: any read
/// error counts as "not detected", never an error. REQ-CLK-004.
#[must_use]
pub fn detect_sync_quality() -> SyncQuality {
    detect_sync_quality_in(Path::new("/sys/class/ptp"))
}

/// [`detect_sync_quality`] against an injectable sysfs root — the
/// testability seam used by `tests/ptp_detect.rs` (fixture sysfs trees)
/// and useful in containers.
///
/// REQ-CLK-004.
#[must_use]
pub fn detect_sync_quality_in(root: &Path) -> SyncQuality {
    let ptp_device = ptp_device_present(root);
    let tai = tai_offset_coherent();
    let ntp_hint = ntp_runtime_hint();
    classify_sync_quality(ptp_device, tai, ntp_hint)
}

/// Pure classification policy (exposed for exact tests).
///
/// - PTP device **and** coherent TAI offset → [`SyncQuality::Ptp`]
/// - coherent TAI offset alone, or an NTP runtime marker → [`SyncQuality::Ntp`]
/// - anything else → [`SyncQuality::Fallback`] (fail-closed)
///
/// REQ-CLK-004.
#[must_use]
pub fn classify_sync_quality(ptp_device: bool, tai_coherent: bool, ntp_hint: bool) -> SyncQuality {
    if ptp_device {
        // Both signals required; an uncoherent device is holdover-grade
        // ambiguity → distrust.
        if tai_coherent {
            SyncQuality::Ptp
        } else {
            SyncQuality::Fallback
        }
    } else if tai_coherent || ntp_hint {
        SyncQuality::Ntp
    } else {
        SyncQuality::Fallback
    }
}

/// `CLOCK_TAI − CLOCK_REALTIME` in nanoseconds, if the kernel exposes
/// `CLOCK_TAI` (Linux). `None` elsewhere or on probe failure. REQ-CLK-004.
#[must_use]
pub fn tai_offset_ns() -> Option<i64> {
    tai_offset_once()
}

/// Is the TAI offset *coherent* — present, sane, and stable across two
/// probes? Coherence gate for [`SyncQuality::Ptp`]/[`SyncQuality::Ntp`].
///
/// Sane window: `0 < offset ≤ 65_535 s`. A zero offset is the kernel
/// default meaning "nobody told us" — not evidence of discipline.
///
/// REQ-CLK-004.
#[must_use]
pub fn tai_offset_coherent() -> bool {
    #[cfg(target_os = "linux")]
    {
        const SANE_MAX: i64 = 65_535 * 1_000_000_000;
        const STABLE_NS: i64 = 1_000_000; // 1 ms between paired probes
        let Some(a) = tai_offset_once() else {
            return false;
        };
        let Some(b) = tai_offset_once() else {
            return false;
        };
        (0..=SANE_MAX).contains(&a) && (a - b).abs() <= STABLE_NS
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(target_os = "linux")]
fn tai_offset_once() -> Option<i64> {
    let realtime = crate::low::clock_gettime_ns_i64(libc::CLOCK_REALTIME)?;
    let tai = crate::low::clock_gettime_ns_i64(libc::CLOCK_TAI)?;
    Some(tai.wrapping_sub(realtime))
}

// `ptp` implies `libc` (see Cargo.toml); the clock_gettime FFI is confined
// to the `low` module — this file is unsafe-free.

#[cfg(not(target_os = "linux"))]
fn tai_offset_once() -> Option<i64> {
    None
}

/// Any PTP device under `root`? Names must match `ptp<digits>` — anything
/// else is ignored, never parsed for meaning (minimal parsing; malformed
/// layout degrades to "absent", and coherence decides the rest).
fn ptp_device_present(root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    entries
        .filter_map(core::result::Result::ok)
        .any(|e| is_ptp_device_name(&e.file_name().to_string_lossy()))
}

fn is_ptp_device_name(name: &str) -> bool {
    match name.strip_prefix("ptp") {
        Some(rest) => !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

fn ntp_runtime_hint() -> bool {
    NTP_MARKERS.iter().any(|p| std::path::Path::new(p).exists())
}

#[cfg(test)]
mod tests {
    use super::{classify_sync_quality, is_ptp_device_name};
    use crate::quality::SyncQuality;

    /// Pure policy: the full truth table, deterministic on every host.
    /// (Ptp requires BOTH device and coherent TAI; anything ambiguous
    /// fails closed.)
    #[test]
    fn classification_truth_table() {
        use SyncQuality::{Fallback, Ntp, Ptp};
        assert_eq!(classify_sync_quality(true, true, false), Ptp);
        assert_eq!(classify_sync_quality(true, true, true), Ptp);
        assert_eq!(classify_sync_quality(true, false, false), Fallback); // holdover-grade ambiguity
        assert_eq!(classify_sync_quality(true, false, true), Fallback);
        assert_eq!(classify_sync_quality(false, true, false), Ntp);
        assert_eq!(classify_sync_quality(false, false, true), Ntp);
        assert_eq!(classify_sync_quality(false, false, false), Fallback);
    }

    /// Minimal sysfs parsing: `ptp0`, `ptp42` yes; junk, `ptp`, `ptpX`,
    /// `ptp0a` no.
    #[test]
    fn ptp_name_parsing_is_minimal() {
        assert!(is_ptp_device_name("ptp0"));
        assert!(is_ptp_device_name("ptp42"));
        assert!(!is_ptp_device_name("ptp"));
        assert!(!is_ptp_device_name("ptpX"));
        assert!(!is_ptp_device_name("ptp0a"));
        assert!(!is_ptp_device_name("clock0"));
        assert!(!is_ptp_device_name(""));
    }
}
