//! Low-level probes — **the crate's only `unsafe` lives in this file.**
//!
//! Contents: the hardware counter reads and invariant-frequency checks
//! (REQ-CLK-003), and the libc `clock_gettime(2)` FFI wrappers the rest of
//! the crate consumes through safe functions. Every other module carries
//! `#![forbid(unsafe_code)]`; the manifest uses `unsafe_code = "deny"`
//! (not `forbid`) solely so this module can exist, with every block
//! justified in place and `undocumented_unsafe_blocks` denied.
//!
//! Per-arch gating is `#[cfg(target_arch)]` inside the single `tsc`
//! feature (spec §Decisions #4), so `--all-features` stays coherent on
//! every host.

/// Raw counter reads and invariant checks per target architecture.
#[cfg(feature = "tsc")]
pub mod counter {
    /// `x86_64` TSC read (unscaled — convert through
    /// [`crate::calibrated::Calibration`]). REQ-CLK-003.
    ///
    /// Serialization note: calibration brackets whole loops and callers
    /// comparing counter deltas to a reference clock tolerate the
    /// few-cycle skew of unserialized `rdtsc`; the fully serialized
    /// `rdtscp` buys nothing at our granularity.
    #[cfg(target_arch = "x86_64")]
    #[must_use]
    pub fn read() -> u64 {
        // SAFETY: `rdtsc` is a plain read of the timestamp counter
        // register. No memory access, no state change, no preconditions;
        // it cannot cause undefined behavior on any x86_64 CPU.
        unsafe { core::arch::x86_64::_rdtsc() }
    }

    /// aarch64 CNTVCT read (unscaled). REQ-CLK-003.
    #[cfg(target_arch = "aarch64")]
    #[must_use]
    pub fn read() -> u64 {
        let v: u64;
        // SAFETY: `mrs` of CNTVCT_EL0 is a plain read of the architecture
        // counter register. No memory is touched, no state changes, and it
        // is architecturally allowed at EL0 (userspace). `nomem`/`nostack`
        // document exactly that.
        unsafe {
            core::arch::asm!("mrs {}, cntvct_el0", out(reg) v, options(nomem, nostack));
        }
        v
    }

    /// Invariant-TSC support probe (`x86_64)`: CPUID leaf `0x8000_0007`,
    /// EDX bit 8 (`Invariant TSC`). False when the leaf is absent or the
    /// bit is clear — the TSC then may not be constant-rate across
    /// frequency changes and must not back a clock. REQ-CLK-003.
    #[cfg(target_arch = "x86_64")]
    #[must_use]
    pub fn invariant_counter_supported() -> bool {
        // `__cpuid_count` is a safe intrinsic: it reads CPU identification
        // registers only, writes nothing, and the leaves queried
        // (0x8000_0000-0x8000_0007) are defined by the architecture for
        // every processor that reports max extended leaf >= 0x8000_0007
        // (checked below before reading 0x8000_0007).
        let max_leaf = core::arch::x86_64::__cpuid_count(0x8000_0000, 0).eax;
        if max_leaf < 0x8000_0007 {
            return false;
        }
        let invariant = core::arch::x86_64::__cpuid_count(0x8000_0007, 0).edx;
        invariant & (1 << 8) != 0
    }

    /// Counter frequency (aarch64): `cntfrq_el0`, used as a sanity bound
    /// for the calibrated conversion. `None` when the register reads 0.
    #[cfg(target_arch = "aarch64")]
    #[must_use]
    pub fn counter_frequency_hz() -> Option<u64> {
        let v: u64;
        // SAFETY: `mrs` of CNTFRQ_EL0 is a plain read of the counter
        // frequency register; no memory or state effects. Readable at EL0.
        unsafe {
            core::arch::asm!("mrs {}, cntfrq_el0", out(reg) v, options(nomem, nostack));
        }
        if v == 0 {
            None
        } else {
            Some(v)
        }
    }
}

/// libc `clock_gettime(2)` wrapper, `Some(nanos)` on success.
///
/// The libc crate declares this FFI `unsafe`; the wrapper confines the
/// block here and exposes safe code to the rest of the crate.
#[cfg(all(feature = "libc", not(feature = "std")))]
pub(crate) fn clock_gettime_ns(clock: libc::clockid_t) -> Option<u64> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `clock_gettime` writes only through the valid, fully
    // initialized `timespec` we hand it; the clock id is one of the
    // kernel's documented constants.
    unsafe {
        if libc::clock_gettime(clock, &raw mut ts) == 0 {
            // tv_sec is `c_long`: i64 on LP64 hosts, i32 on ILP32 — the
            // cast is load-bearing on the 32-bit targets.
            #[allow(clippy::unnecessary_cast)]
            Some(
                (ts.tv_sec as u64)
                    .saturating_mul(1_000_000_000)
                    .saturating_add(ts.tv_nsec as u64),
            )
        } else {
            None
        }
    }
}

/// Signed variant for the TAI offset probe (clock values can exceed the
/// unsigned epoch scale on misconfigured hosts).
#[cfg(all(feature = "libc", target_os = "linux"))]
pub(crate) fn clock_gettime_ns_i64(clock: libc::clockid_t) -> Option<i64> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: same contract as `clock_gettime_ns`.
    unsafe {
        if libc::clock_gettime(clock, &raw mut ts) == 0 {
            // See the cast note in `clock_gettime_ns`.
            #[allow(clippy::unnecessary_cast)]
            Some(
                (ts.tv_sec as i64)
                    .saturating_mul(1_000_000_000)
                    .saturating_add(ts.tv_nsec as i64),
            )
        } else {
            None
        }
    }
}
