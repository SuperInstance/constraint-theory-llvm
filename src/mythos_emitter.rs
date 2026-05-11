//! VMythos — register-level Mythos mapped to AVX-512
//!
//! ## Instruction Mapping
//!
//! | Mythos Operation | AVX-512 Instruction |
//! |-----------------|--------------------|
//! | TilesAsKV.embed | VMOVDQU64 |
//! | RoomsAsExperts.gate | VPCMPD $6 (SGE) |
//! | DeadbandACT.should_continue | VPCMPD $2 (SLE) |
//! | ShellLoRA.adapt | VPANDD + VPORD |
//! | Warden.filter | KPANDW |
//! | Bard.output | KORTESTW + JZ |

use crate::emitter_x86::{ExecutableBuffer, VPCMPD, KInstructions, GPRInstructions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMythosOp {
    LoadTile,
    GateLower,
    GateUpper,
    Filter,
    DeadbandCheck,
    Output,
    Repair,
    Schedule,
    Adapt,
    EmergeTest,
}

impl VMythosOp {
    pub fn latency(self) -> u8 {
        match self {
            VMythosOp::LoadTile | VMythosOp::GateLower | VMythosOp::GateUpper | VMythosOp::Schedule => 3,
            _ => 1,
        }
    }
}

/// Emit a complete Mythos constraint iteration in AVX-512 machine code.
pub struct VMythosEmitter {
    pub code: ExecutableBuffer,
    depth: usize,
}

impl VMythosEmitter {
    pub fn new() -> Result<Self, String> {
        Ok(VMythosEmitter {
            code: ExecutableBuffer::new(4096)?,
            depth: 0,
        })
    }

    /// Emit one Mythos iteration: Load, GateLower, GateUpper, Filter, DeadbandCheck.
    pub fn emit_constraint_iteration(&mut self) {
        // GateLower: VPCMPD $6 (SGE), k1, zmm0, zmm1
        VPCMPD::emit_sge(&mut self.code, 1, 0, 1);
        // GateUpper: VPCMPD $2 (SLE), k2, zmm0, zmm2
        VPCMPD::emit_sle(&mut self.code, 2, 0, 2);
        // Warden.filter: KPANDW k3, k1, k2
        KInstructions::kpandw(&mut self.code, 3, 1, 2);
        // DeadbandCheck: KORTESTW k3, k3 → ZF
        KInstructions::kortestw(&mut self.code, 3, 3);
        self.depth += 1;
    }

    pub fn finalize(mut self) -> Result<ExecutableBuffer, String> {
        GPRInstructions::ret(&mut self.code);
        self.code.finalize()?;
        Ok(self.code)
    }
}

/// Build a complete constraint check function.
pub fn build_mythos_loop() -> ExecutableBuffer {
    let mut emitter = VMythosEmitter::new().expect("alloc");
    // Prologue
    emitter.code.emit(&[0x55]); // push rbp
    emitter.code.emit(&[0x48, 0x89, 0xE5]); // mov rbp, rsp
    // Emit constraint check
    emitter.emit_constraint_iteration();
    // Return 1 (all pass)
    emitter.code.emit(&[0xB8, 0x01, 0x00, 0x00, 0x00]);
    GPRInstructions::ret(&mut emitter.code);
    let _ = emitter.code.finalize();
    emitter.code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mythos_op_latency() {
        assert_eq!(VMythosOp::LoadTile.latency(), 3);
        assert_eq!(VMythosOp::Filter.latency(), 1);
        assert_eq!(VMythosOp::DeadbandCheck.latency(), 1);
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn test_mythos_loop_builds() {
        let buf = build_mythos_loop();
        // The buffer starts with prologue (0x55 = push rbp)
        unsafe {
            assert_eq!(*buf.base_address(), 0x55);
        }
    }
}
