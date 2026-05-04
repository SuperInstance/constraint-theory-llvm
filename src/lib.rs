//! constraint-theory-llvm: LLVM Backend for Constraint Theory Core
//!
//! # The Creative Gap (FM's Next Breakthrough)
//!
//! We built:
//! - constraint-theory-core: CDCL solver, AC-3, Sudoku, Rigidity
//! - plato-llvm-bridge: PLATO → LLVM IR emitter
//! - avx512-constraint-checker: AVX-512 constraint engine (35.9B/s)
//!
//! Missing piece: LLVM backend for constraint-theory-core
//! This compiles CDCL traces → LLVM IR → AVX-512 machine code.

mod trace;
mod emitter;
mod optimizer;

pub use trace::{CDCLTrace, TraceEvent, Decision, Propagation, Conflict, Backtrack};
pub use emitter::{LLVMEmitter, EmitterConfig, OptimizationLevel};
pub use optimizer::AVX512Optimizer;