#[cfg(target_arch = "aarch64")]
pub mod neon_mythos {
    /// Mythos recurrent step via NEON intrinsics.
    /// Processes 4 lanes per call. Returns mask of passing lanes.
    pub unsafe fn mythos_step_neon(
        values: core::arch::aarch64::int32x4_t,
        lo: core::arch::aarch64::int32x4_t,
        hi: core::arch::aarch64::int32x4_t,
    ) -> core::arch::aarch64::int32x4_t {
        // GateLower: CMGE (compare signed greater-or-equal)
        let k1 = core::arch::aarch64::vreinterpretq_s32_u32(
            core::arch::aarch64::vcgeq_s32(values, lo)
        );
        // GateUpper: CMLE (compare signed less-or-equal)
        let k2 = core::arch::aarch64::vreinterpretq_s32_u32(
            core::arch::aarch64::vcleq_s32(values, hi)
        );
        // Warden.filter: AND the masks
        core::arch::aarch64::vandq_s32(k1, k2)
    }

    /// Full 16-lane Mythos constraint check via 4 NEON iterations.
    pub fn mythos_neon_16lane(
        values: &[i32; 16],
        lowers: &[i32; 16],
        uppers: &[i32; 16],
    ) -> [i32; 8] {
        unsafe {
            let mut result = [0i32; 8];
            for i in 0..4 {
                let v = core::arch::aarch64::vld1q_s32(values[i*4..].as_ptr());
                let lo = core::arch::aarch64::vld1q_s32(lowers[i*4..].as_ptr());
                let hi = core::arch::aarch64::vld1q_s32(uppers[i*4..].as_ptr());
                let mask = mythos_step_neon(v, lo, hi);
                core::arch::aarch64::vst1q_s32(result[i*4..].as_mut_ptr(), mask);
            }
            result
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_neon_step_x86_equiv() {
            let values = [1i32, 25, 50, 75, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            let lo = [0i32; 16];
            let hi = [50i32; 16];

            let result = mythos_neon_16lane(&values, &lo, &hi);
            assert_eq!(result[0], -1); // 1 >= 0 && 1 <= 50 = true
            assert_eq!(result[1], -1); // 25 passes
            assert_eq!(result[2], -1); // 50 passes
            assert_eq!(result[3], 0);  // 75 > 50 = false
        }
    }
}
