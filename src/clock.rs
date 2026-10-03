//! The estate time idiom: [`Clock`] — and the chronoshift bridge.
//!
//! clock-kit does **not** mint a rival time abstraction (spec §Decisions #1,
//! recommended resolution): chronoshift (L0) owns time *abstraction*,
//! clock-kit owns time *precision and cost*. With the `chronoshift` feature
//! a blanket impl makes every `chronoshift::Clock` a clock-kit [`Clock`],
//! bridging `now_ns()` into a full [`Timestamp`] with the monotonic floor
//! applied.

#[cfg(feature = "chronoshift")]
use crate::monotonic::mono_floor;
use crate::quality::{ClockSource, SyncQuality};
use crate::timestamp::Timestamp;
#[cfg(feature = "chronoshift")]
use crate::timestamp::WallTime;

/// A time source producing wall+monotonic [`Timestamp`]s.
///
/// Object-safe; all implementations enforce REQ-CLK-001 by construction
/// (`mono` passes through a process-wide floor). Two advisory accessors
/// make cost and trust observable instead of guessed.
///
/// REQ-CLK-001, REQ-CLK-004.
pub trait Clock {
    /// Current reading. `t.mono` never decreases across calls within a
    /// process, even if the underlying source regresses. REQ-CLK-001.
    fn now(&self) -> Timestamp;

    /// Advisory sync quality of the underlying source (G2 — a hint, never a
    /// guarantee). REQ-CLK-004.
    fn quality(&self) -> SyncQuality;

    /// Which underlying source reads are actually served from. REQ-CLK-003.
    fn source(&self) -> ClockSource;
}

#[cfg(feature = "chronoshift")]
impl<T: chronoshift::Clock> Clock for T {
    fn now(&self) -> Timestamp {
        let ns = chronoshift::Clock::now_ns(self);
        // Bridge is lossy at the edges, documented: chronoshift nanos are
        // i64 (signed, wall scale). Negative (pre-epoch) clamps to 0 for
        // both fields; monotonic passes through the process-wide floor so
        // even a regressing bridged source honors REQ-CLK-001.
        let wall = u64::try_from(ns).unwrap_or(0);
        Timestamp {
            wall: WallTime::from_unix_nanos(wall),
            mono: mono_floor(wall),
        }
    }

    fn quality(&self) -> SyncQuality {
        // The bridge cannot know how the foreign clock is disciplined;
        // fail-closed to distrust (G2).
        SyncQuality::Fallback
    }

    fn source(&self) -> ClockSource {
        ClockSource::External
    }
}
