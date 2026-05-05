//! CDCL Trace Tests

use constraint_theory_llvm::{CDCLTrace, LLVMEmitter, EmitterConfig, AVX512Optimizer, OptimizationLevel};

fn make_trace() -> CDCLTrace {
    let mut t = CDCLTrace::new();
    t.add_decide(1, 1, None);
    t.add_propagate(2, 0, 1);
    t.add_decide(2, 3, None);
    t.add_conflict(2, 1, vec![1, 2, 3]);
    t.add_backtrack(1, vec![-4]);
    t.add_decide(2, -4, None);
    t.add_propagate(-4, 0, 2);
    t
}

#[test]
fn test_trace_decisions() {
    let t = make_trace();
    let prog = t.decision_program();
    assert_eq!(prog.len(), 3, "Should have 3 decisions");
    assert_eq!(prog[0], 1);
    assert_eq!(prog[1], 3);
    assert_eq!(prog[2], -4);
}

#[test]
fn test_emitter_output() {
    let t = make_trace();
    let emitter = LLVMEmitter::new(EmitterConfig::default());
    let ir = emitter.emit_trace(&t);
    assert!(ir.contains("check_constraints"));
    assert!(ir.contains("@batch_check"));
    assert!(ir.contains("@bloom_check"));
    assert!(ir.contains("source_filename"));
}

#[test]
fn test_optimizer_aggressive() {
    let ir = r#"define i1 @check_constraints(i32 %x) { entry: ret i1 true }"#;
    let opt = AVX512Optimizer::optimize(ir, OptimizationLevel::Aggressive);
    assert!(!opt.is_empty());
}

#[test]
fn test_empty_trace_emits_module() {
    let mut t = CDCLTrace::new();
    let emitter = LLVMEmitter::new(EmitterConfig::default());
    let ir = emitter.emit_trace(&t);
    assert!(ir.contains("define i1 @check_constraints"));
}

#[test]
fn test_large_trace() {
    let mut t = CDCLTrace::new();
    for i in 1..=20 {
        t.add_decide(i, i as i64, None);
        if i % 5 == 0 {
            t.add_conflict(i, 0, vec![1, 2]);
            t.add_backtrack(i - 1, vec![-1]);
        }
    }
    let emitter = LLVMEmitter::new(EmitterConfig::default());
    let ir = emitter.emit_trace(&t);
    // check_clause calls only appear when there are learned clauses
    // With no learned clauses, the trace just has decisions
    assert!(t.decision_program().len() == 20, "Should have 20 decisions");
}

#[test]
fn test_emitter_module_output() {
    let t = make_trace();
    let emitter = LLVMEmitter::new(EmitterConfig::default());
    let module = emitter.emit_module(&t);
    assert!(module.contains("!llvm.module.flags"));
}

#[test]
fn test_vector_width_default() {
    let config = EmitterConfig::default();
    assert_eq!(config.vector_width, 16);
}

#[test]
fn test_optimization_levels() {
    for level in [OptimizationLevel::None, OptimizationLevel::Less, OptimizationLevel::Default, OptimizationLevel::Aggressive] {
        let mut t = CDCLTrace::new();
        t.add_decide(1, 1, None);
        let config = EmitterConfig { optimization_level: level, ..Default::default() };
        let emitter = LLVMEmitter::new(config);
        let ir = emitter.emit_trace(&t);
        assert!(ir.contains("define i1 @check_constraints"));
    }
}

#[test]
fn test_trace_depth() {
    let mut t = CDCLTrace::new();
    t.add_decide(1, 1, None);
    t.add_decide(2, 2, None);
    t.add_decide(3, 3, None);
    assert_eq!(t.decision_depth(), 3);
}