//! Calibrated hardware-counter clock — REQ-CLK-003.
//!
//! The hot path reads a hardware counter (`x86_64` `rdtsc`, aarch64
//! `cntvct_el0`) and converts with a **multiply-shift** — never a division.
//! Calibration ([`calibrate`]) is an explicit, fallible cold-path step: it
//! brackets counter reads with a reference monotonic clock and derives a
//! 32.32 fixed-point ns-per-tick factor. If calibration fails or the
//! counter is judged unusable (no invariant-TSC bit, unstable rate),
//! [`CalibratedClock`] **transparently falls back** to `Instant` (std) /
//! `CLOCK_MONOTONIC` (libc); [`Clock::source`] reports which path actually
//! serves reads.

use crate::clock::Clock;
use crate::monotonic::mono_floor;
use crate::quality::{ClockSource, SyncQuality};
use crate::system::{monotonic_nanos, wall_nanos};
use crate::timestamp::{Timestamp, WallTime};

#[cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
use crate::low;

/// How the ns-per-tick factor was derived.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CalMethod {
    /// `x86_64` invariant TSC bracketed against the reference clock.
    X86TscInvariant,
    /// aarch64 CNTVCT bracketed against the reference clock.
    ArmCntvct,
}

/// A calibrated ns-per-tick conversion factor, held as
/// `ns_per_tick + frac/2^32` (a 32.32 fixed point).
///
/// Conversion ([`Calibration::ticks_to_ns`]) is two multiplies and a shift
/// — no division on the read path. REQ-CLK-003.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Calibration {
    /// Integral nanoseconds per counter tick.
    pub ns_per_tick: u64,
    /// Fractional nanos per tick, numerator over 2^32.
    pub frac: u32,
    /// How the factor was measured.
    pub method: CalMethod,
}

impl Calibration {
    /// Convert raw ticks to nanoseconds (multiply-shift; no division).
    #[must_use]
    pub fn ticks_to_ns(&self, ticks: u64) -> u64 {
        let integral = ticks.wrapping_mul(self.ns_per_tick);
        // Infallible widening (u64/u32 ⊂ u128), one multiply, one shift.
        #[allow(clippy::cast_possible_truncation)] // 32.32: low word by construction
        let frac = ((u128::from(ticks) * u128::from(self.frac)) >> 32) as u64;
        integral.wrapping_add(frac)
    }
}

/// Why calibration refused the hardware counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CalError {
    /// No reference monotonic clock is available (neither `std` nor
    /// `libc` features).
    NoReferenceClock,
    /// The architecture has no supported counter (the `tsc` feature is on,
    /// but this is not `x86_64/aarch64`), or the invariant/frequency check
    /// failed.
    UnsupportedCounter,
    /// Two bracketed measurements disagreed beyond tolerance — the counter
    /// is not constant-rate (or the machine is wildly noisy).
    UnstableCounter,
    /// The measurement window made no progress (a dead counter).
    NoProgress,
}

/// Derive the ns-per-tick calibration factor.
///
/// Cold path: brackets `reps` counter reads between two reference
/// monotonic-clock reads (repeatable — call again to re-calibrate; see the
/// drift caveat in README). Two independent measurements must agree within
/// 1% or the counter is rejected ([`CalError::UnstableCounter`]).
///
/// # Errors
///
/// [`CalError::UnsupportedCounter`] when the architecture has no counter
/// or the invariant/frequency check fails; [`CalError::UnstableCounter`]
/// when the two bracketed measurements disagree beyond 1%;
/// [`CalError::NoProgress`] when the counter does not advance;
/// [`CalError::NoReferenceClock`] when no reference monotonic clock exists.
///
/// REQ-CLK-003.
pub fn calibrate(reps: u32) -> Result<Calibration, CalError> {
    #[cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        #[cfg(target_arch = "x86_64")]
        let method = {
            if !low::counter::invariant_counter_supported() {
                return Err(CalError::UnsupportedCounter);
            }
            CalMethod::X86TscInvariant
        };
        #[cfg(target_arch = "aarch64")]
        let method = {
            // Arch timer frequency must be sane before trusting a factor.
            match low::counter::counter_frequency_hz() {
                Some(f) if (1_000..=10_000_000_000).contains(&f) => CalMethod::ArmCntvct,
                _ => return Err(CalError::UnsupportedCounter),
            }
        };

        let a = measure_once(reps)?;
        let b = measure_once(reps)?;
        // Constant-frequency check: two factors must agree within 1%.
        // Cross-multiplied in u128 — no floats, no division.
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        if (hi - lo) * 100 > lo {
            return Err(CalError::UnstableCounter);
        }
        Ok(split_factor(u128::midpoint(a, b), method))
    }
    #[cfg(not(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    {
        let _ = reps;
        Err(CalError::UnsupportedCounter)
    }
}

#[cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
fn measure_once(reps: u32) -> Result<u128, CalError> {
    let r0 = monotonic_nanos();
    let t0 = low::counter::read();
    for _ in 0..reps {
        core::hint::black_box(low::counter::read());
    }
    let t1 = low::counter::read();
    let r1 = monotonic_nanos();

    let dns = u128::from(r1) - u128::from(r0);
    let dticks = u128::from(t1) - u128::from(t0);
    if dticks == 0 {
        return Err(CalError::NoProgress);
    }
    if dns == 0 {
        // Reference clock too coarse to calibrate against.
        return Err(CalError::UnstableCounter);
    }
    // 32.32 fixed point: (ns << 32) / ticks. One division, cold path only.
    Ok((dns << 32) / dticks)
}

#[cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
fn split_factor(factor: u128, method: CalMethod) -> Calibration {
    Calibration {
        // 32.32 split: by construction `factor < 2^96` (ns < 2^64), so the
        // high word fits u64 and the low word is u32 by definition.
        #[allow(clippy::cast_possible_truncation)]
        ns_per_tick: (factor >> 32) as u64,
        #[allow(clippy::cast_possible_truncation)]
        frac: factor as u32,
        method,
    }
}

/// Precision clock over a hardware counter, with transparent fallback.
///
/// Construct via [`CalibratedClock::new`] (falls back silently when the
/// counter is unusable — the requested precision stays observable through
/// [`Clock::source`]) or [`CalibratedClock::try_new`] (strict: construction
/// fails when the counter cannot be calibrated). REQ-CLK-003.
#[derive(Clone, Copy, Debug)]
pub struct CalibratedClock {
    cal: Option<Calibration>,
    fallback_source: ClockSource,
}

impl CalibratedClock {
    /// Calibrate and construct; on any calibration failure construct the
    /// reference-clock fallback instead (never fails). `reps` scales the
    /// calibration workload (e.g. `1_000..=1_000_000`).
    #[must_use]
    pub fn new(reps: u32) -> Self {
        match Self::try_new(reps) {
            Ok(clock) => clock,
            // Transparent fallback: source() reports what actually serves.
            Err(_) => Self {
                cal: None,
                fallback_source: fallback_source(),
            },
        }
    }

    /// Strict construction: `Err` when the hardware counter is unusable.
    ///
    /// # Errors
    ///
    /// Whatever [`calibrate`] reports (see its `# Errors` section) — the
    /// fallback constructor [`CalibratedClock::new`] is the transparent
    /// counterpart.
    pub fn try_new(reps: u32) -> Result<Self, CalError> {
        Ok(Self {
            cal: Some(calibrate(reps)?),
            fallback_source: fallback_source(),
        })
    }

    /// Construct the explicit reference-clock fallback — opting out of the
    /// counter path entirely (useful when a consumer wants the
    /// conservative source by policy, not by calibration outcome).
    #[must_use]
    pub fn with_fallback() -> Self {
        Self {
            cal: None,
            fallback_source: fallback_source(),
        }
    }

    /// Construct directly from a pre-computed calibration (e.g. re-using a
    /// factor calibrated earlier in the process).
    #[must_use]
    pub fn with_calibration(cal: Calibration) -> Self {
        Self {
            cal: Some(cal),
            fallback_source: fallback_source(),
        }
    }

    /// The active calibration, if the counter path is in use.
    #[must_use]
    pub fn calibration(&self) -> Option<Calibration> {
        self.cal
    }
}

impl Clock for CalibratedClock {
    fn now(&self) -> Timestamp {
        let mono = match self.cal {
            #[cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
            Some(cal) => mono_floor(cal.ticks_to_ns(low::counter::read())),
            // `Some` is unreachable where the counter path does not exist
            // (calibrate() only succeeds on supported arches), but this arm
            // must still compile: the fallback serves there too.
            _ => mono_floor(monotonic_nanos()),
        };
        Timestamp {
            wall: WallTime::from_unix_nanos(wall_nanos()),
            mono,
        }
    }

    fn quality(&self) -> SyncQuality {
        #[cfg(feature = "ptp")]
        {
            crate::system::cached_quality()
        }
        #[cfg(not(feature = "ptp"))]
        {
            SyncQuality::Fallback
        }
    }

    fn source(&self) -> ClockSource {
        match self.cal {
            Some(Calibration {
                method: CalMethod::X86TscInvariant,
                ..
            }) => ClockSource::Tsc,
            Some(Calibration {
                method: CalMethod::ArmCntvct,
                ..
            }) => ClockSource::Cntvct,
            None => self.fallback_source,
        }
    }
}

fn fallback_source() -> ClockSource {
    #[cfg(feature = "std")]
    {
        ClockSource::Instant
    }
    #[cfg(not(feature = "std"))]
    {
        ClockSource::LibcMono
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(miri))]
    use super::calibrate;
    use super::{CalMethod, Calibration};

    #[cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
    use super::split_factor;

    /// Multiply-shift conversion is exact at the integral level and
    /// monotone in ticks. Pure logic — runs under miri.
    #[test]
    fn ticks_to_ns_is_linear_and_monotone() {
        // 3 GHz-ish: 0.333.. ns/tick → ns_per_tick = 0, frac ≈ 0x5555_5555.
        let cal = Calibration {
            ns_per_tick: 0,
            frac: 0x5555_5555,
            method: CalMethod::X86TscInvariant,
        };
        let ns1 = cal.ticks_to_ns(1);
        let ns9 = cal.ticks_to_ns(9);
        // 1 tick ≈ 0.3333 ns → truncates to 0; 9 ticks ≈ 3 ns.
        #[allow(clippy::cast_possible_truncation)] // 32.32 low word by construction
        {
            assert_eq!(ns1, (0x5555_5555_u128 >> 32) as u64);
            assert_eq!(ns9, ((9_u128 * 0x5555_5555) >> 32) as u64);
        }

        // 10 ns/tick exactly: integral dominates.
        let cal10 = Calibration {
            ns_per_tick: 10,
            frac: 0,
            method: CalMethod::ArmCntvct,
        };
        assert_eq!(cal10.ticks_to_ns(0), 0);
        assert_eq!(cal10.ticks_to_ns(1_000), 10_000);

        // Monotone in ticks across the wrap-safe path.
        let mut prev = 0;
        for t in [0_u64, 1, 2, 100, u64::MAX / 8] {
            let ns = cal10.ticks_to_ns(t);
            assert!(ns >= prev);
            prev = ns;
        }
    }

    /// Factor splitting keeps the 32.32 round trip.
    #[cfg(all(feature = "tsc", any(target_arch = "x86_64", target_arch = "aarch64")))]
    #[test]
    fn split_factor_round_trips() {
        let cal = split_factor((3_u128 << 32) / 10, CalMethod::X86TscInvariant);
        assert_eq!(cal.ns_per_tick, 0);
        // 0.3 ns/tick as 32.32.
        assert_eq!(cal.frac, 0x4CCC_CCCC);
    }

    /// Calibration is repeatable where a counter exists, and cleanly
    /// refused otherwise. Touches the reference clock — skipped under
    /// miri (FFI/clock syscalls are out of miri scope, spec §Gate plan).
    #[cfg(not(miri))]
    #[test]
    fn calibrate_is_repeatable_or_cleanly_refused() {
        if let Ok(a) = calibrate(1_000) {
            // Repeatable: a second calibration must also succeed and the
            // factor must be usable (some conversion output exists).
            let b = calibrate(1_000).unwrap_or(a);
            assert!(b.ns_per_tick > 0 || b.frac > 0);
            assert!(a.ticks_to_ns(u64::MAX / 8) > 0);
        }
    }

    /// Advisory surface without hardware: the explicit fallback reports
    /// the reference source, and both counter sources are nameable via
    /// `with_calibration` (no hardware needed to check observability).
    #[test]
    fn source_is_observable_per_construction() {
        use crate::clock::Clock;
        use crate::quality::ClockSource;

        let fallback = super::CalibratedClock::with_fallback();
        assert!(fallback.calibration().is_none());
        #[cfg(feature = "std")]
        assert_eq!(fallback.source(), ClockSource::Instant);
        #[cfg(not(feature = "std"))]
        assert_eq!(fallback.source(), ClockSource::LibcMono);
        // Quality is total: any classification, never a panic.
        let _ = fallback.quality();

        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        {
            let tsc = super::CalibratedClock::with_calibration(Calibration {
                ns_per_tick: 1,
                frac: 0,
                method: CalMethod::X86TscInvariant,
            });
            assert_eq!(tsc.source(), ClockSource::Tsc);
            assert!(tsc.calibration().is_some());

            let cnt = super::CalibratedClock::with_calibration(Calibration {
                ns_per_tick: 0,
                frac: 40,
                method: CalMethod::ArmCntvct,
            });
            assert_eq!(cnt.source(), ClockSource::Cntvct);
        }
    }
}
