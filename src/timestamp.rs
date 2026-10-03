//! Wall/monotonic timestamp pair — REQ-CLK-002.
//!
//! Wall time and monotonic time are separate fields with separate rules:
//! wall time is comparable across processes and machines and *may step*
//! (NTP slew, PTP step, operator action); monotonic time is comparable only
//! within a process and *never steps*. No API derives one from the other
//! except [`Timestamp::mono_from_wall_lossy`], which is explicitly labelled
//! lossy.

#[cfg(feature = "std")]
use core::time::Duration;

/// Wall-clock instant: nanoseconds since the Unix epoch.
///
/// `SystemTime`-backed under `std`; a `u64` epoch-nanos wrapper under
/// `core` + `libc` (spec §Decisions #2). The portable accessor is
/// [`unix_nanos`](WallTime::unix_nanos) — identical on both builds.
/// Pre-epoch readings clamp to `0`: a clock library must not panic.
///
/// REQ-CLK-002.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WallTime(u64);

impl WallTime {
    /// The Unix epoch (0 nanos).
    pub const UNIX_EPOCH: Self = Self(0);

    /// Wrap raw epoch nanos.
    #[must_use]
    pub const fn from_unix_nanos(ns: u64) -> Self {
        Self(ns)
    }

    /// Nanoseconds since the Unix epoch — the identical accessor on every
    /// build (std, libc, core-only).
    ///
    /// REQ-CLK-002.
    #[must_use]
    pub const fn unix_nanos(&self) -> u64 {
        self.0
    }

    /// Reconstruct as `std::time::SystemTime` (std builds only).
    ///
    /// Saturation: absurd values beyond `SystemTime`'s range collapse to
    /// `UNIX_EPOCH` rather than panicking.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn system(&self) -> std::time::SystemTime {
        std::time::SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_nanos(self.0))
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    }

    /// Capture the current wall clock (std builds only). Pre-epoch system
    /// clocks clamp to `0`.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn from_system_now() -> Self {
        Self(now_wall_nanos())
    }
}

/// A paired wall+monotonic reading.
///
/// - `wall`: see [`WallTime`] — may step, comparable across machines.
/// - `mono`: nanos on a process-local monotonic scale — never steps and,
///   by REQ-CLK-001, never regresses across calls within a process.
///
/// No `From`/`Into` conversion exists between the two fields in either
/// direction; [`Timestamp::mono_from_wall_lossy`] is the single, explicitly
/// lossy escape hatch. REQ-CLK-002.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Timestamp {
    /// Wall time — may step, cross-process/machine comparable.
    pub wall: WallTime,
    /// Monotonic nanos — never regresses within a process (REQ-CLK-001).
    pub mono: u64,
}

impl Timestamp {
    /// Assemble a timestamp from its parts.
    #[must_use]
    pub const fn from_parts(wall: WallTime, mono: u64) -> Self {
        Self { wall, mono }
    }

    /// Derive a monotonic-style value from the wall field.
    ///
    /// **Explicitly lossy and labelled as such** (REQ-CLK-002): wall time
    /// steps, so the result may go backward; it lives on the wall scale,
    /// not the process-local monotonic scale. This is the *only*
    /// cross-derivation in the API surface.
    #[must_use]
    pub fn mono_from_wall_lossy(&self) -> u64 {
        self.wall.unix_nanos()
    }
}

#[cfg(feature = "std")]
fn now_wall_nanos() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => u64::try_from(d.as_nanos()).unwrap_or(u64::MAX),
        // Pre-epoch (misconfigured machine): clamp, never panic.
        Err(_) => 0,
    }
}
