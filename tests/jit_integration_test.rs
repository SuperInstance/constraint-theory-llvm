//! JIT integration test
//!
//! This test demonstrates the CDCL trace → LLVM IR → JIT compilation workflow.
//!
//! **Current Status**: Cranelift CANNOT directly compile our LLVM IR.
//! Our emitter produces LLVM IR (.ll format). Cranelift expects its own IR (.clif).
//!
//! This test documents the workflow and expected failures.

use constraint_theory_llvm::{CDCLTrace, LLVMEmitter, EmitterConfig};
#[cfg(feature = "jit")]
use constraint_theory_llvm::{transpile_llvm_to_cranelift, JITConfig};

fn make_trace_with_learned_clauses() -> CDCLTrace {
    let mut t = CDCLTrace::new();
    // Add a conflict with learned clause
    t.add_decide(1, 1, None);
    t.add_propagate(2, 0, 1);
    t.add_conflict(1, 0, vec![1, 2, 3]);
    t.add_learn(0, vec![1, -2, 3]);  // Learned clause
    t.add_backtrack(0, vec![-4]);
    t
}

#[test]
fn test_cdcl_trace_to_llvm_ir() {
    let trace = make_trace_with_learned_clauses();
    let emitter = LLVMEmitter::new(EmitterConfig::default());
    
    // Emit LLVM IR
    let llvm_ir = emitter.emit_trace(&trace);
    
    // Verify LLVM IR structure
    assert!(llvm_ir.contains("source_filename"));
    assert!(llvm_ir.contains("target triple"));
    assert!(llvm_ir.contains("define i1 @check_constraints"));
    assert!(llvm_ir.contains("@batch_check"));
    
    // The IR should compile with LLVM (llc) but NOT with Cranelift
    println!("Emitted LLVM IR:\n{}", llvm_ir);
}

// JIT-dependent tests moved behind feature gate below

#[cfg(feature = "jit")]
mod jit_tests {
    use super::*;
    
    #[test]
    fn test_cdcl_trace_to_jit_workflow() {
        // Complete workflow test: CDCL trace → LLVM IR → JIT attempt
        let trace = make_trace_with_learned_clauses();
        let emitter = LLVMEmitter::new(EmitterConfig::default());
        let llvm_ir = emitter.emit_trace(&trace);
        
        // Step 1: Emit works
        assert!(!llvm_ir.is_empty());
        
        // Step 2: Try to transpile to Cranelift format (fails as expected)
        let clif_result = transpile_llvm_to_cranelift(&llvm_ir);
        assert!(clif_result.is_err(), "Cranelift cannot compile LLVM IR");
        
        println!("Workflow completed:");
        println!("1. CDCL trace created with {} decisions", trace.decisions);
        println!("2. LLVM IR emitted ({} chars)", llvm_ir.len());
        println!("3. Cranelift transpilation FAILED as expected");
        println!("4. Workaround: Use Inkwell+LLVM or direct x86-64 codegen");
    }
}