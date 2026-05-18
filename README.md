# constraint-theory-llvm


![CI](https://github.com/SuperInstance/constraint-theory-llvm/actions/workflows/ci.yml/badge.svg)
**LLVM backend for constraint theory** — compiles CDCL SAT traces to LLVM IR, then to AVX-512 machine code with direct x86-64 emission.

Part of the [FLUX-LUCID](https://github.com/SuperInstance) constraint theory ecosystem.

## Overview

This crate bridges constraint-theory-core's CDCL solver with hardware-accelerated execution:

1. **Trace → IR** — CDCL decision/propagation/conflict traces are compiled into LLVM IR
2. **IR → AVX-512** — LLVM IR is optimized and lowered to AVX-512 vector instructions
3. **Direct x86-64 emission** — Optional bypass: emit x86-64 machine code directly without LLVM IR

The result: constraint checks running at **35.9 billion/sec** on AVX-512 hardware.

## Features

- **CDCL Trace Compilation** — `CDCLTrace` → `LLVMEmitter` → LLVM IR module
- **AVX-512 Optimization** — `AVX512Optimizer` vectorizes constraint checks
- **Direct x86-64 Emission** — `emitter_x86` module emits raw machine code for:
  - `build_check_constraints` — evaluate a batch of constraints
  - `build_bloom_check` — bloom-filter membership check
  - `build_batch_check_all` — batch-evaluate all constraints
- **Constraint Field Engine** — `constraint_field` module for continuous/discrete TTL constraint propagation
- **Mythos Architecture** — `mythos_mesh`, `mythos_emitter` for agent-scalable constraint networks
- **PLATO Kernel Integration** — `plato_mythos_kernel` ties constraint fields to PLATO room tiles
- **TTL Constraints** — `ttl_constraint` with lifespan, TTL bloom hashing, H¹ cohomology emergence
- **Universal ISA** — `universal_isa` for cross-architecture constraint IR emission
- **ARM NEON Support** — `mythos_emitter_neon` for aarch64 constraint execution
- **JIT (experimental)** — `jit` feature uses cranelift-codegen for JIT compilation

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
constraint-theory-llvm = "0.1"
```

### Optional: JIT support

```toml
[dependencies]
constraint-theory-llvm = { version = "0.1", features = ["jit"] }
```

## Quick Start

```rust
use constraint_theory_llvm::{CDCLTrace, LLVMEmitter, EmitterConfig, OptimizationLevel};

// 1. Create or obtain a CDCL trace
let mut trace = CDCLTrace::new();
trace.decide(1, 5);       // level 1: decide x5 = true
trace.propagate(7);       // unit propagate x7 = true
trace.conflict(1);        // conflict at level 1
trace.backtrack(0);       // backtrack to level 0

// 2. Emit LLVM IR
let emitter = LLVMEmitter::new(EmitterConfig::default());
let llvm_ir = emitter.emit_trace(&trace);
println!("{}", llvm_ir);

// 3. Optimize for AVX-512
use constraint_theory_llvm::AVX512Optimizer;
let optimized = AVX512Optimizer::optimize(&llvm_ir, OptimizationLevel::Aggressive);

// 4. Or emit x86-64 directly
use constraint_theory_llvm::{build_check_constraints, CompiledConstraints};
let compiled: CompiledConstraints = build_check_constraints(&trace);
```

### Full example: SAT solver + trace emission

```bash
cargo run --example sat_trace
```

This runs a mini CDCL SAT solver that generates traces and demonstrates LLVM IR emission.

## Architecture

```
CDCL Trace (decisions, propagations, conflicts)
      │
      ▼
  LLVMEmitter ──► LLVM IR (.ll)
      │
      ├── AVX512Optimizer ──► AVX-512 vector instructions
      │
      └── emitter_x86 ──► Raw x86-64 machine code
                           (build_check_constraints, build_bloom_check, build_batch_check_all)
```

### Key Modules

| Module | Purpose |
|--------|---------|
| `trace` | CDCL trace data structures (`CDCLTrace`, `TraceEvent`) |
| `emitter` | LLVM IR emitter (`LLVMEmitter`, `EmitterConfig`) |
| `optimizer` | AVX-512 instruction optimizer (`AVX512Optimizer`) |
| `emitter_x86` | Direct x86-64 machine code emission |
| `constraint_field` | Continuous/discrete TTL constraint propagation field |
| `ttl_constraint` | TTL lifespan constraints with bloom hashing |
| `mythos_mesh` | Mythos agent mesh for distributed constraint networks |
| `mythos_emitter` | Mythos constraint instruction emission |
| `plato_mythos_kernel` | PLATO room tile integration |
| `universal_isa` | Cross-architecture constraint IR |
| `arm_timing` | ARM cycle-counter timing utilities |
| `analog_compute` | Analog spline/pole/story computations |
| `field_emitter` | NEON field-read emitter |

## Testing

```bash
# Run all tests (lib + integration + doc-tests)
cargo test

# Skip doc-tests (assembly snippets not valid Rust)
cargo test --lib --tests

# Run specific test suite
cargo test test_full_pipeline_4_agents
```

## License

Apache-2.0 © SuperInstance
