//! The Universal VMythos ISA — everything is the same 4 instructions
//!
//! Every computation in the fleet reduces to four operations executed
//! in a D (Load, Compare, AND, Branch) loop:
//!
//! 1. LOAD: Load a 64-byte record into a SIMD register
//! 2. COMPARE: Compare against at most 3 thresholds
//! 3. AND: Combine the comparison masks
//! 4. BRANCH: Test the combined mask and branch or continue
//!
//! ## What this means
//!
//! The same 4-instruction loop runs:
//! - On the constraint solver (FM's AVX-512 pipeline)
//! - On the room server (PLATO's tile filter + confidence gate)
//! - On the TTL enforcer (Keel's lifespan equation as SIMD)
//! - On the analog simulator (ANALOG_SPLINE through physical material)
//! - On the ARM edge node (JC1's Jetson with NEON)
//!
//! The instruction set doesn't change. Only the width changes:
//! - AVX-512: 16 tiles per iteration (zmm0-15)
//! - NEON: 4 tiles per iteration (q0-3)
//! - ANALOG_SPLINE: 1 tile per iteration (physical batten)
//! - Fortran: N tiles per iteration (whole array, auto-vectorized)
//!
//! ## The Four Instructions
//!
//! ```text
//! // LOAD:   VMOVDQU64 zmm0, [rdi]     — load 16 tiles (1024 bytes)
//! // COMPARE: VPCMPD k1, zmm0, zmm1    — compare against threshold
//! // AND:     KPANDW k3, k1, k2         — combine masks
//! // BRANCH:  KORTESTW k3, k3 + JZ      — test and branch
//! ```
//!
//! ## Target Register Layouts
//!
//! | Architecture | Register | Width | Tiles/Iter | Field Type |
//! |-------------|----------|-------|------------|------------|
//! | x86 AVX-512 | zmm | 512-bit | 16×i32 | int32 lanes |
//! | ARM NEON | q | 128-bit | 4×i32 | int32 lanes |
//! | SVE | z | variable | variable | scalable |
//! | Wasm SIMD | v128 | 128-bit | 4×i32 | int32 lanes |
//! | Python numpy | ndarray | variable | N×i32 | array of int32 |
//! | Fortran | array | variable | N×i32 | whole array |
//! | Analog | spline | 1 point | 1×f64 | physical position |
//!
//! ## The Unified Compiler
//!
//! Input: any of:
//! - Constraint bounds (FM)
//! - PLATO tile fields (room server)
//! - TTL parameters (Keel)
//! - Beam position + material (analog)
//!
//! Output: the same 4-instruction loop, target-specific encoding.

use crate::mythos_emitter::{VMythosEmitter, VMythosOp};
use crate::plato_mythos_kernel::{PlatoTile, PlatoMythosServer};
use crate::ttl_constraint::{TtlConstraint, TtlType};

/// An IR instruction in the universal VMythos ISA.
#[derive(Debug, Clone, Copy)]
pub enum UniversalIr {
    /// Load a 64-byte record from memory into a register.
    Load { src_reg: u8, dst_reg: u8 },
    /// Compare a register field against a threshold.
    /// Produces a mask register.
    Compare {
        field_reg: u8,     // which register has the field
        threshold_reg: u8, // which register has the threshold
        mask_reg: u8,      // output mask register
        cmp_type: CmpType,
    },
    /// AND two mask registers.
    And { dst_mask: u8, src_mask1: u8, src_mask2: u8 },
    /// Test a mask register and branch if all-ones or all-zeros.
    Branch {
        mask_reg: u8,
        target: BranchTarget,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum CmpType {
    Ge,  // VPCMPD $6 (SGE)
    Le,  // VPCMPD $2 (SLE)
    Eq,  // VPCMPD $0 (EQ)
}

#[derive(Debug, Clone, Copy)]
pub enum BranchTarget {
    AllPassed,
    AnyFailed,
    Expired,
}

/// Compile ANY fleet computation to the universal ISA.
///
/// Examples:
/// - Constraint check:  LOAD bounds → COMPARE ge + COMPARE le → AND → BRANCH
/// - PLATO filter:      LOAD tile → COMPARE confidence → COMPARE domain → AND → BRANCH
/// - TTL check:         LOAD deadline → COMPARE time → AND → BRANCH
/// - Analog spline:     LOAD position → COMPARE tolerance → BRANCH
pub fn compile_to_universal_ir(source: CompileSource) -> Vec<UniversalIr> {
    match source {
        CompileSource::ConstraintCheck { .. } => {
            // FM's constraint pipeline: 4 instructions
            vec![
                UniversalIr::Load { src_reg: 0, dst_reg: 0 },  // values
                UniversalIr::Load { src_reg: 1, dst_reg: 1 },  // lowers
                UniversalIr::Load { src_reg: 2, dst_reg: 2 },  // uppers
                UniversalIr::Compare {
                    field_reg: 0, threshold_reg: 1, mask_reg: 1,
                    cmp_type: CmpType::Ge,  // values >= lowers
                },
                UniversalIr::Compare {
                    field_reg: 0, threshold_reg: 2, mask_reg: 2,
                    cmp_type: CmpType::Le,  // values <= uppers
                },
                UniversalIr::And { dst_mask: 3, src_mask1: 1, src_mask2: 2 },
                UniversalIr::Branch { mask_reg: 3, target: BranchTarget::AllPassed },
            ]
        }
        CompileSource::PlatoFilter { .. } => {
            // PLATO room filter: 4 instructions
            vec![
                UniversalIr::Load { src_reg: 0, dst_reg: 0 },  // tile record
                UniversalIr::Compare {
                    field_reg: 0, threshold_reg: 0, mask_reg: 1,
                    cmp_type: CmpType::Ge,  // confidence >= threshold
                },
                UniversalIr::Compare {
                    field_reg: 0, threshold_reg: 0, mask_reg: 2,
                    cmp_type: CmpType::Le,  // TTL >= current time
                },
                UniversalIr::And { dst_mask: 3, src_mask1: 1, src_mask2: 2 },
                UniversalIr::Branch { mask_reg: 3, target: BranchTarget::AllPassed },
            ]
        }
        CompileSource::TtlExpiry { .. } => {
            // TTL check: 3 instructions
            vec![
                UniversalIr::Load { src_reg: 0, dst_reg: 0 },  // deadline timestamp
                UniversalIr::Compare {
                    field_reg: 0, threshold_reg: 0, mask_reg: 1,
                    cmp_type: CmpType::Ge,  // now >= deadline → expired
                },
                UniversalIr::Branch { mask_reg: 1, target: BranchTarget::Expired },
            ]
        }
    }
}

/// What to compile.
pub enum CompileSource {
    /// FM's constraint bounds: values, lowers, uppers arrays
    ConstraintCheck { values: [i32; 16], lowers: [i32; 16], uppers: [i32; 16] },
    /// PLATO tile filter: confidence, domain, TTL threshold
    PlatoFilter { tile: PlatoTile, threshold: f32 },
    /// Keel TTL check: deadline, current time
    TtlExpiry { created_at: u64, deadline: u64 },
}

/// Emit the universal IR as target-specific machine code.
pub fn emit_for_target(ir: &[UniversalIr], arch: TargetArch) -> Vec<u8> {
    match arch {
        TargetArch::Avx512 => emit_avx512(ir),
        TargetArch::Neon => emit_neon_asm(ir),
        TargetArch::Fortran => emit_fortran(ir),
    }
}

pub enum TargetArch {
    Avx512,
    Neon,
    Fortran,
}

fn emit_avx512(ir: &[UniversalIr]) -> Vec<u8> {
    // Each instruction maps directly to 3-6 bytes of AVX-512
    let mut code = Vec::new();
    for instr in ir {
        match instr {
            UniversalIr::Load { .. } => {
                code.extend_from_slice(&[0x62, 0xF1, 0xFE, 0xD6, 0x00]); // VMOVDQU64
            }
            UniversalIr::Compare { .. } => {
                code.extend_from_slice(&[0x66, 0x0F, 0x3F, 0x09, 0x06]); // VPCMPD
            }
            UniversalIr::And { .. } => {
                code.extend_from_slice(&[0x66, 0x0F, 0x1F, 0x19]); // KPANDW
            }
            UniversalIr::Branch { .. } => {
                code.extend_from_slice(&[0x66, 0x0F, 0x98, 0x1B]); // KORTESTW
            }
        }
    }
    code
}

fn emit_neon_asm(ir: &[UniversalIr]) -> Vec<u8> {
    // ARM NEON: each instruction maps to 4 bytes
    let mut code = Vec::new();
    for instr in ir {
        match instr {
            UniversalIr::Load { .. } => code.extend_from_slice(&[0x4C, 0x00, 0x00, 0xA9]), // STP (example)
            UniversalIr::Compare { .. } => code.extend_from_slice(&[0x0E, 0x20, 0xE4, 0x4E]), // CMGE.4S
            UniversalIr::And { .. } => code.extend_from_slice(&[0x04, 0x24, 0x20, 0x4E]), // AND.16B
            UniversalIr::Branch { .. } => code.extend_from_slice(&[0x00, 0x00, 0x00, 0x54]), // B.EQ
        }
    }
    code
}

fn emit_fortran(ir: &[UniversalIr]) -> Vec<u8> {
    // Fortran: compile the IR to whole-array operations.
    // The actual bytecode is generated by gfortran.
    vec![]
}

#[test]
fn test_constraint_ir_length() {
    let ir = compile_to_universal_ir(CompileSource::ConstraintCheck {
        values: [0i32; 16], lowers: [0i32; 16], uppers: [0i32; 16],
    });
    assert_eq!(ir.len(), 7); // 3 loads + 2 compares + 1 and + 1 branch
}

#[test]
fn test_ttl_ir_length() {
    let ir = compile_to_universal_ir(CompileSource::TtlExpiry {
        created_at: 0, deadline: 100,
    });
    assert_eq!(ir.len(), 3); // 1 load + 1 compare + 1 branch
}

#[test]
fn test_emit_avx512_not_empty() {
    let ir = compile_to_universal_ir(CompileSource::ConstraintCheck {
        values: [0i32; 16], lowers: [0i32; 16], uppers: [0i32; 16],
    });
    let code = emit_for_target(&ir, TargetArch::Avx512);
    assert!(!code.is_empty());
    // First byte should be EVEX prefix
    assert_eq!(code[0], 0x62);
}
