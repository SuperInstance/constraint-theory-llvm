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
//!
//! # JIT Exploration
//!
//! When the `jit` feature is enabled, this module explores using
//! cranelift-codegen for JIT compilation. See `jit.rs` for details.
//! 
//! ## Key Finding
//! 
//! Cranelift CANNOT compile LLVM IR (.ll) format. It uses its own IR (.clif).
//! This means the emitter's output cannot be directly compiled by cranelift.
//! Options: Inkwell (LLVM bindings), static LLC, or rewrite emitter for .clif.

#[cfg(feature = "jit")]
mod jit;

#[cfg(feature = "jit")]
pub use jit::{JITConfig, OptLevel, transpile_llvm_to_cranelift};

mod trace;
mod emitter;
mod optimizer;

pub use trace::{CDCLTrace, TraceEvent, Decision, Propagation, Conflict, Backtrack};
pub use emitter::{LLVMEmitter, EmitterConfig, OptimizationLevel};
pub use optimizer::AVX512Optimizer;
pub mod analog_compute;
