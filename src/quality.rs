//! Sync-quality classification and clock-source identity — REQ-CLK-004.

/// Advisory classification of the host's time synchronization.
///
/// A **hint** read at clock construction/discovery time — never a guarantee
/// (G2): a [`SyncQuality::Ptp`]-labelled source may be in holdover.
/// Consumers use it to decide whether derived timestamps may be correlated
/// across machines, never for correctness. REQ-CLK-004.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SyncQuality {
    /// PTP device present **and** coherent `CLOCK_TAI` offset. Still only a
    /// hint: the source may be in holdover.
    Ptp,
    /// Disciplined by a network sync stack (NTP markers, or a coherent TAI
    /// offset without a PTP device).
    Ntp,
    /// Nothing trustworthy detected — fail-closed to distrust.
    #[default]
    Fallback,
}

impl SyncQuality {
    /// Stable machine-readable name (`"ptp" | "ntp" | "fallback"`).
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Ptp => "ptp",
            Self::Ntp => "ntp",
            Self::Fallback => "fallback",
        }
    }
}

/// Which underlying source a [`Clock`](crate::Clock) is actually reading.
///
/// Observable per REQ-CLK-003: read cost and source used must be
/// discoverable — e.g. a `CalibratedClock` that fell back reports the
/// fallback, never the counter it failed to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClockSource {
    /// `x86_64` `rdtsc` behind a calibrated multiply-shift.
    Tsc,
    /// aarch64 `cntvct_el0` behind a calibrated multiply-shift.
    Cntvct,
    /// `std::time::Instant` (the calibrated-clock fallback under `std`).
    Instant,
    /// `clock_gettime(CLOCK_MONOTONIC)` via libc (`no_std` hosts, and the
    /// calibrated-clock fallback under the `libc` feature).
    LibcMono,
    /// Deterministic test clock — the OS is never consulted.
    Mock,
    /// Bridged `chronoshift::Clock` (the `chronoshift` feature).
    External,
}

impl ClockSource {
    /// Stable machine-readable name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Tsc => "tsc",
            Self::Cntvct => "cntvct",
            Self::Instant => "instant",
            Self::LibcMono => "libc-mono",
            Self::Mock => "mock",
            Self::External => "external",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ClockSource, SyncQuality};

    /// Stable names for machine-readable surfaces (logs, metrics labels).
    #[test]
    fn names_are_stable() {
        assert_eq!(SyncQuality::Ptp.as_str(), "ptp");
        assert_eq!(SyncQuality::Ntp.as_str(), "ntp");
        assert_eq!(SyncQuality::Fallback.as_str(), "fallback");
        assert_eq!(ClockSource::Tsc.as_str(), "tsc");
        assert_eq!(ClockSource::Cntvct.as_str(), "cntvct");
        assert_eq!(ClockSource::Instant.as_str(), "instant");
        assert_eq!(ClockSource::LibcMono.as_str(), "libc-mono");
        assert_eq!(ClockSource::Mock.as_str(), "mock");
        assert_eq!(ClockSource::External.as_str(), "external");
        // Default is fail-closed.
        assert_eq!(SyncQuality::default(), SyncQuality::Fallback);
    }
}
