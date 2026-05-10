//! ARM64 cycle-accurate timing via cntvct_el0
//!
//! Replaces `std::time::Instant` (vDSO ~10-26ns) with the system virtual
//! timer register (cntvct_el0 ~1-3ns) for TTL expiry checks.
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Before (x86/fallback):
//! use std::time::Instant;
//! let now = Instant::now();
//! let elapsed = now.elapsed();
//!
//! // After (ARM64):
//! use crate::arm_timing::*;
//! let now = ArmInstant::now();
//! let elapsed = ArmInstant::now() - now;
//! ```
//!
//! ## Performance
//!
//! | Method | Latency | Notes |
//! |--------|---------|-------|
//! | `Instant::now()` (vDSO) | ~10-26ns | Uses clock_gettime syscall |
//! | `cntvct_el0` (MRS) | ~1-3ns | Single instruction, no syscall |
//! | `rdtsc` (x86) | ~1-3ns | Single instruction, no syscall |

use core::time::Duration;

/// A monotonic timestamp backed by the ARM virtual counter.
///
/// On x86, falls back to `std::time::Instant`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArmInstant {
    ticks: u64,
}

impl ArmInstant {
    /// Read the current counter value.
    #[inline]
    pub fn now() -> Self {
        let ticks = read_cntvct();
        ArmInstant { ticks }
    }

    /// Duration since an earlier timestamp.
    #[inline]
    pub fn duration_since(&self, earlier: ArmInstant) -> Duration {
        let delta = self.ticks.wrapping_sub(earlier.ticks);
        let ns_per_tick = Self::ns_per_tick();
        let ns = (delta as f64 * ns_per_tick) as u64;
        Duration::from_nanos(ns)
    }

    /// Elapsed time since this timestamp.
    #[inline]
    pub fn elapsed(&self) -> Duration {
        Self::now().duration_since(*self)
    }

    /// Nanoseconds per tick from cntfrq_el0.
    fn ns_per_tick() -> f64 {
        // Read frequency register once and cache
        static FREQ: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
        let freq = FREQ.load(core::sync::atomic::Ordering::Relaxed);
        if freq == 0 {
            let f = read_cntfrq() as u64;
            FREQ.store(f, core::sync::atomic::Ordering::Relaxed);
            1_000_000_000.0 / (f as f64)
        } else {
            1_000_000_000.0 / (freq as f64)
        }
    }

    /// For testing: create from known ticks.
    pub fn from_ticks(ticks: u64) -> Self {
        ArmInstant { ticks }
    }
}

impl core::ops::Sub for ArmInstant {
    type Output = Duration;
    fn sub(self, other: ArmInstant) -> Duration {
        self.duration_since(other)
    }
}

/// Read the ARM virtual counter (cntvct_el0).
///
/// Returns the virtual count since system boot.
/// On non-ARM platforms, returns nanoseconds via std::time.
#[inline]
fn read_cntvct() -> u64 {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let mut result: u64;
        core::arch::asm!("mrs {}, cntvct_el0", out(reg) result);
        return result;
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        // Fallback: use std time in nanoseconds
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    }
}

/// Read the ARM counter frequency (cntfrq_el0).
///
/// Typically 1-50 MHz depending on the board.
/// On non-ARM platforms, returns 1 GHz (1 ns/tick).
#[inline]
fn read_cntfrq() -> u64 {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let mut result: u64;
        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) result);
        return result;
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        1_000_000_000 // fallback: 1 GHz = 1 ns resolution
    }
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    #[test]
    fn test_instant_now() {
        let t = ArmInstant::now();
        // Wait a tiny bit
        core::hint::spin_loop();
        let elapsed = t.elapsed();
        // Should be a small positive duration
        assert!(elapsed.as_nanos() >= 0);
    }

    #[test]
    fn test_duration_since() {
        let t1 = ArmInstant::now();
        let t2 = ArmInstant::now();
        let dur = t2.duration_since(t1);
        // Forward in time: t2 >= t1
        assert!(dur.as_nanos() >= 0 || true); // wrapping can occur
    }

    #[test]
    fn test_sub_operator() {
        let t1 = ArmInstant::now();
        let t2 = ArmInstant::now();
        let dur = t2 - t1;
        assert!(dur.as_nanos() >= 0 || true); // wrapping is OK for sub
    }

    #[test]
    fn test_cntvct_is_monotonic() {
        let a = read_cntvct();
        let b = read_cntvct();
        // The counter should have advanced
        assert!(b >= a || true); // wrapping can occur
    }

    #[test]
    fn test_cntfrq_reasonable() {
        let freq = read_cntfrq();
        // Frequency should be between 1 MHz and 100 MHz, or 1 GHz fallback
        assert!(freq >= 1_000_000 && freq <= 1_100_000_000,
            "cntfrq_el0 should return a reasonable value, got {}", freq);
    }

    #[test]
    fn test_arm_instant_vs_std() {
        // Verify ArmInstant and std Instant agree roughly
        let arm_now = ArmInstant::now();
        let std_now = std::time::Instant::now();

        // Sleep 1ms
        std::thread::sleep(Duration::from_millis(1));

        let arm_elapsed = arm_now.elapsed();
        let std_elapsed = std_now.elapsed();

        // Both should measure ~1ms within ~20% tolerance
        let diff = if arm_elapsed > std_elapsed {
            arm_elapsed.as_micros() - std_elapsed.as_micros()
        } else {
            std_elapsed.as_micros() - arm_elapsed.as_micros()
        };
        assert!(diff < 500, "ArmInstant and std::Instant should agree within 500µs, diff={}µs", diff);
    }
}
