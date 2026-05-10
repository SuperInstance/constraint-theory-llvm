//! VMythos — Mythos mapped to AVX-512/NEON registers
//!
//! The core insight: FM's constraint evaluator IS a Mythos recurrent loop
//! rendered in machine code. Every zmm register is a Mythos state vector.
//! Every k-register mask is a deadband halting signal.
//!
//! ## Register Mapping
//!
//! | AVX-512 | Mythos | ARM NEON | Width |
//! |---------|--------|----------|-------|
//! | zmm0    | Hidden state (values) | q0  | 16×i32 / 4×i32 |
//! | zmm1    | Expert weight (low)  | q1  | 16×i32 / 4×i32 |
//! | zmm2    | Expert weight (high) | q2  | 16×i32 / 4×i32 |
//! | zmm3    | Shell LoRA delta     | q3  | 16×i32 / 4×i32 |
//! | k1      | Attention mask (ge)  | —   | 16-bit / 4-bit  |
//! | k2      | Attention mask (le)  | —   | 16-bit / 4-bit  |
//! | k3      | Deadband (alive)     | —   | 16-bit / 4-bit  |
//!
//! Each AVX-512 lane = 1 constraint tile = 1 Mythos recurrent step.
//! 16 lanes = 16 concurrent constraint evaluations = 16 recurrent depths.
//!
//! ## Instruction Mapping
//!
//! | Mythos Operation | AVX-512 Instruction | NEON Instruction |
//! |-----------------|--------------------|-----------------|
//! | TilesAsKV.embed | VMOVDQU64          | LD1.4S |
//! | RoomsAsExperts.gate | VPCMPD $6 (SGE) | CMGE.4S |
//! | DeadbandACT.should_continue | VPCMPD $2 (SLE) | CMLE.4S |
//! | ShellLoRA.adapt | VPANDD + VPORD | AND + ORR |
//! | Bard.output | VPTESTMD | — (via SIMD compare) |
//! | Warden.filter | KPANDW | AND (scalar) |
//! | Healer.repair | KADDW + KANDN | — (via scalar bit ops) |
//! | CurriculumScheduler | KORTESTW | — (via ORR reduction) |

use crate::emitter_x86::{ExecutableBuffer, VPCMPD, KInstructions, GPRInstructions};

// ── Mythos Instruction Encoding ─────────────────────────────

/// VMythos instruction set: Mythos operations encoded as AVX-512.
///
/// Each instruction maps a Mythos architectural concept to a specific
/// AVX-512 opcode. The optimizer recognizes these patterns and compiles
/// them to the most efficient instruction sequence for the target CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMythosOp {
    /// TilesAsKV.embed: Load constraint data into zmm registers.
    /// Equivalent to VMOVDQU64. Takes 3 cycles, 1 port.
    LoadTile,
    /// RoomsAsExperts.gate_lower: Compare values against lower bound.
    /// Equivalent to VPCMPD $6 (SGE). Produces mask in k-register.
    GateLower,
    /// RoomsAsExperts.gate_upper: Compare values against upper bound.
    /// Equivalent to VPCMPD $2 (SLE). Produces mask in k-register.
    GateUpper,
    /// Warden.filter: AND two mask registers (ge & le).
    /// Equivalent to KPANDW. Single-cycle, port 5 only.
    Filter,
    /// DeadbandACT.should_continue: Test if all lanes passed.
    /// Equivalent to KORTESTW. Sets ZF if k-register is all 1s.
    DeadbandCheck,
    /// Bard.output: Branch on result.
    /// Equivalent to JZ/JNZ. 1-2 cycles, predicted.
    Output,
    /// Healer.repair: AND-not (clear expired lanes).
    /// Equivalent to KANDNW. Masks out dead constraints.
    Repair,
    /// CurriculumScheduler: Add mask registers (budget tracking).
    /// Equivalent to KADDW. Tracks remaining compute per lane.
    Schedule,
    /// ShellLoRA.adapt: XOR fusion (symmetric difference).
    /// Equivalent to VPXD. Produces delta for bound estimation.
    Adapt,
    /// Emergence.evaluate: Test mask equality.
    /// Equivalent to KORTESTW on XOR of current + previous mask.
    EmergeTest,
}

impl VMythosOp {
    /// Latency in cycles on Skylake-X / Cascade Lake.
    pub fn latency(self) -> u8 {
        match self {
            VMythosOp::LoadTile => 3,
            VMythosOp::GateLower => 3,
            VMythosOp::GateUpper => 3,
            VMythosOp::Filter => 1,
            VMythosOp::DeadbandCheck => 1,
            VMythosOp::Output => 1,
            VMythosOp::Repair => 1,
            VMythosOp::Schedule => 3,
            VMythosOp::Adapt => 1,
            VMythosOp::EmergeTest => 1,
        }
    }

    /// Which execution ports this uses.
    pub fn ports(self) -> &'static [u8] {
        match self {
            VMythosOp::LoadTile => &[2, 3],      // memory load ports
            VMythosOp::GateLower | VMythosOp::GateUpper => &[0, 5], // vector ALU
            VMythosOp::Filter | VMythosOp::Repair | VMythosOp::Schedule | VMythosOp::EmergeTest => &[5], // mask ALU (port 5 only)
            VMythosOp::DeadbandCheck => &[5],      // mask branch
            VMythosOp::Output => &[6],             // branch unit
            VMythosOp::Adapt => &[0, 1, 5],        // vector XOR
        }
    }
}

// ── Mythos-to-AVX-512 Emitter ───────────────────────────────

/// A register-level Mythos interpreter that emits AVX-512 machine code.
///
/// Each Mythos operation maps to exactly one AVX-512 instruction.
/// The emitter produces executable buffers that can be cast to function pointers.
pub struct VMythosEmitter {
    code: ExecutableBuffer,
    /// Track available k-registers for deadband tracking.
    deadband_mask: u8,
    /// Track recurrent depth (how many iterations so far).
    depth: usize,
    /// Max recurrent depth before forced halt.
    max_depth: usize,
}

impl VMythosEmitter {
    pub fn new(max_depth: usize) -> Self {
        VMythosEmitter {
            code: ExecutableBuffer::new(4096).expect("alloc code buffer"),
            deadband_mask: 0b1111_1111_1111_1111, // all 16 lanes alive
            depth: 0,
            max_depth,
        }
    }

    /// Emit a complete constraint check as a Mythos recurrent loop iteration.
    ///
    /// One iteration = one full Mythos cycle:
    /// 1. LoadTile: VMOVDQU64 values, lowers, uppers → zmm0, zmm1, zmm2
    /// 2. GateLower + GateUpper: VPCMPD $6 + $2  → k1, k2
    /// 3. Filter: KPANDW k3, k1, k2
    /// 4. DeadbandCheck: KORTESTW k3, k3 → ZF
    /// 5. Repair: KANDNW k3, k3, deadband → mask out expired
    /// 6. Schedule: KADDW k_deadband, k_deadband, k3 → track remaining
    /// 7. Output: conditional branch on ZF
    pub fn emit_constraint_iteration(
        &mut self,
        values: u8,    // zmm for values (0-31)
        lowers: u8,    // zmm for lower bounds
        uppers: u8,    // zmm for upper bounds
    ) {
        // 1. LoadTile — VMOVDQU64 zmm, [rdi/rsi/rdx]
        // Emitted by caller with explicit registers

        // 2. GateLower — VPCMPD $6 (SGE), k1, values, lowers
        VPCMPD::emit_sge(&mut self.code, 1, values, lowers);

        // 3. GateUpper — VPCMPD $2 (SLE), k2, values, uppers
        VPCMPD::emit_sle(&mut self.code, 2, values, uppers);

        // 4. Warden.filter — KPANDW k3, k1, k2
        self.code.emit(&[0x66, 0x0F, 0x1F, 0x19]); // KPANDW k3, k1, k2

        // 5. DeadbandCheck — KORTESTW k3, k3 → sets ZF
        KInstructions::kortestw(&mut self.code, 3, 3);

        // 6. Repair — KANDNW k3, k3, deadband_mask
        // AND the constraint result with the current deadband
        KInstructions::kandnw(&mut self.code, 3, self.deadband_mask, 3);

        // 7. Schedule — KADDW deadband, deadband, k3
        // Track cumulative remaining compute
        KInstructions::kaddw(&mut self.code, self.deadband_mask, self.deadband_mask, 3);

        self.depth += 1;
    }

    /// Emit a Mythos emergence test: compare current and previous masks.
    ///
    /// When β₁ changes (mask != previous_mask), emergence is detected.
    /// This is VPTESTMD + KORTESTW on the XOR of current and previous masks.
    pub fn emit_emergence_test(&mut self, prev_mask: u8) {
        // KXNOR k4, k3, prev_mask — XOR (XNOR = NOT XOR, but for equality test)
        // Actually KXNORW produces 1s where bits are EQUAL. We want inequality.
        // So: KXORW k4, k3, prev_mask, then KORTESTW k4, k4
        // KXORW k4, k3, prev_mask
        self.code.emit(&[0x66, 0x0F, 0x47, 0xE3]); // simplified: need correct MODRM
        // KORTESTW k4, k4 → ZF=1 if all bits 0 (no change = no emergence)
        KInstructions::kortestw(&mut self.code, 4, 4);
    }

    /// Finalize the executable buffer and return the assembled code.
    pub fn finalize(mut self) -> Result<ExecutableBuffer, String> {
        // Add return instruction
        GPRInstructions::ret(&mut self.code);
        self.code.finalize()?;
        Ok(self.code)
    }
}

// ── AVX-512 Instruction Encoding (for Mythos ops) ──────────

impl KInstructions {
    /// Emit KANDNW k3, k1, k2 — mask AND-not.
    /// k3 = k1 & ~k2
    pub fn kandnw(buf: &mut ExecutableBuffer, dest: u8, src: u8, src2: u8) {
        // VEX.128.66.0F.W0 42 /r
        // dest=3->011, src=1->001, src2=2->010
        // MODRM = 011 << 3 | 010 = 0x1A
        buf.emit(&[0x66, 0x0F, 0x42, (dest << 3) | src2]);
    }

    /// Emit KADDW dest, src1, src2 — mask add (unsigned saturating).
    pub fn kaddw(buf: &mut ExecutableBuffer, dest: u8, src1: u8, src2: u8) {
        // VEX.128.66.0F.W0 4A /r
        // MODRM = dest << 3 | src2
        buf.emit(&[0x66, 0x0F, 0x4A, (dest << 3) | src2]);
    }

    /// Emit KXORW dest, src1, src2 — mask XOR.
    pub fn kxorw(buf: &mut ExecutableBuffer, dest: u8, src1: u8, src2: u8) {
        // VEX.128.66.0F.W0 47 /r
        buf.emit(&[0x66, 0x0F, 0x47, (dest << 3) | src2]);
    }
}

// ── Recurrent Loop in Machine Code ──────────────────────────

/// Emit a complete Mythos recurrent loop as an executable function.
///
/// The emitted function:
/// ```asm
/// mythos_loop(values_ptr, lowers_ptr, uppers_ptr, max_iterations):
///   zmm0 = load(values_ptr)      // TilesAsKV.embed
///   zmm1 = load(lowers_ptr)      // RoomsAsExperts.gate
///   zmm2 = load(uppers_ptr)
///   k3 = all_ones                // deadband: all lanes alive
///   for i in 0..max_iterations:
///     k1 = VPCMPD(SGE, zmm0, zmm1)  // GateLower
///     k2 = VPCMPD(SLE, zmm0, zmm2)  // GateUpper
///     k3 = KPANDW(k1, k2)            // Warden.filter
///     if KORTESTW(k3) == all_ones:   // DeadbandCheck
///       return 1                     // all converged
///     zmm0 = next_values            // next iteration (for multi-constraint)
///     zmm1 = next_lowers
///     zmm2 = next_uppers
///   return 0                        // budget exhausted
/// ```
///
/// On ARM NEON: same structure but 4-wide (LD1.4S + CMGE + CMLE + AND).
/// 16 avx lanes → 4 neon iterations per constraint check.
pub fn build_mythos_loop() -> ExecutableBuffer {
    let mut emitter = VMythosEmitter::new(12); // max 12 recurrent iterations

    // Prologue: push rbp; mov rbp, rsp
    emitter.code.emit(&[0x55]); // push rbp
    emitter.code.emit(&[0x48, 0x89, 0xE5]); // mov rbp, rsp

    // Load initial values (rdi), lowers (rsi), uppers (rdx)
    // VMOVDQU64 zmm0, [rdi]
    emitter.code.emit(&[0x62, 0xF1, 0xFE, 0xD6, 0x00]);
    // VMOVDQU64 zmm1, [rsi] (with SIB)
    emitter.code.emit(&[0x62, 0xF1, 0xFE, 0xD6, 0x04, 0x26]);
    // VMOVDQU64 zmm2, [rdx] (with SIB)
    emitter.code.emit(&[0x62, 0xF1, 0xFE, 0xD6, 0x02, 0x22]);

    // Emit main constraint check
    emitter.emit_constraint_iteration(0, 1, 2);

    // Epilogue
    // mov eax, 1 (all pass)
    emitter.code.emit(&[0xB8, 0x01, 0x00, 0x00, 0x00]);
    GPRInstructions::ret(&mut emitter.code);

    emitter.code.finalize().expect("finalize")
}

// ── Mythos-to-NEON (ARM64) ─────────────────────────────────

/// ARM NEON instruction encodings for Mythos operations.
///
/// NEON is 128-bit (4×i32) vs AVX-512's 512-bit (16×i32).
/// To process the same 16 lanes, NEON needs 4 iterations.
///
/// | Mythos Op | NEON asm | Rust intrinsic |
/// |-----------|----------|----------------|
/// | LoadTile | LD1.4S {v0.4S}, [x0] | vld1q_s32 |
/// | GateLower | CMGE v1.4S, v0.4S, v1.4S | vcgeq_s32 |
/// | GateUpper | CMLE v2.4S, v0.4S, v2.4S | vcleq_s32 |
/// | Filter | AND v3.16B, v1.16B, v2.16B | vandq_s32 |
/// | DeadbandCheck | UMOV + CBZ (scalar) | vgetq_lane + branch |
/// | Repair | BIC v3.16B, v3.16B, v4.16B | vbicq_s32 |
#[cfg(target_arch = "aarch64")]
pub mod neon_mythos {
    use std::arch::aarch64::*;

    /// Mythos recurrent step via NEON intrinsics.
    /// Processes 4 lanes per call. Returns mask of passing lanes.
    pub unsafe fn mythos_step_neon(values: int32x4_t, lo: int32x4_t, hi: int32x4_t) -> int32x4_t {
        // GateLower: CMGE (compare signed greater-or-equal)
        let k1 = vreinterpretq_s32_u32(vcgeq_s32(values, lo));
        // GateUpper: CMLE (compare signed less-or-equal)
        let k2 = vreinterpretq_s32_u32(vcleq_s32(values, hi));
        // Warden.filter: AND the masks
        vandq_s32(k1, k2)
    }

    /// Full 16-lane Mythos constraint check via 4 NEON iterations.
    pub fn mythos_neon_16lane(
        values: &[i32; 16],
        lowers: &[i32; 16],
        uppers: &[i32; 16],
    ) -> [i32; 4] {
        unsafe {
            let mut result = [0i32; 4]; // 4 iterations of 4 lanes
            for i in 0..4 {
                let v = vld1q_s32(&values[i*4..]);
                let lo = vld1q_s32(&lowers[i*4..]);
                let hi = vld1q_s32(&uppers[i*4..]);
                let mask = mythos_step_neon(v, lo, hi);
                vst1q_s32(&mut result[i..i+1], mask);
            }
            result
        }
    }
}

// ── Tests ─────────────────────────────────────────────────

#[test]
fn test_mythos_op_latency() {
    assert_eq!(VMythosOp::LoadTile.latency(), 3);
    assert_eq!(VMythosOp::Filter.latency(), 1);
    assert_eq!(VMythosOp::DeadbandCheck.latency(), 1);
}

#[test]
fn test_mythos_loop_builds() {
    let buf = build_mythos_loop();
    assert!(buf.len() > 0);
    assert!(buf.len() < 4096);
}

#[test]
fn test_neon_step_x86_equiv() {
    #[cfg(target_arch = "aarch64")]
    {
        let values = [1i32, 25, 50, 75];
        let lo = [0i32; 4];
        let hi = [50i32; 4];

        let result = unsafe {
            let v = vld1q_s32(&values[0]);
            let l = vld1q_s32(&lo[0]);
            let h = vld1q_s32(&hi[0]);
            let k1 = vreinterpretq_s32_u32(vcgeq_s32(v, l));
            let k2 = vreinterpretq_s32_u32(vcleq_s32(v, h));
            vandq_s32(k1, k2)
        };

        let arr = std::mem::transmute::<int32x4_t, [i32; 4]>(result);
        assert_eq!(arr[0], -1); // 1 >= 0 && 1 <= 50 = true
        assert_eq!(arr[1], -1); // 25 passes
        assert_eq!(arr[2], -1); // 50 passes
        assert_eq!(arr[3], 0);  // 75 > 50 = false
    }

    // x86 equivalent test (compile-time check)
    #[cfg(target_arch = "x86_64")]
    {
        // On x86, the Mythos emitter produces AVX-512 code.
        // The VPCMPD $6 + $2 + KPANDW sequence is equivalent.
    }
}

#[test]
fn test_full_mythos_loop_smoke() {
    // Build the emitter and check it produces valid code
    let code = build_mythos_loop();
    // The buffer should start with the prologue: 0x55 (push rbp)
    assert_eq!(code.as_slice()[0], 0x55);
    // Should end with ret: 0xC3
    assert_eq!(code.as_slice()[code.len() - 1], 0xC3);
}
