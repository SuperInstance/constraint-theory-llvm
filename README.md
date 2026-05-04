# constraint-theory-llvm

**LLVM backend for constraint-theory-core — compile CDCL traces to AVX-512.**

> "LLVM backend for constraint-theory-core" — the creative gap identified between FM's constraint-theory-core and his AVX-512 constraint engine.

## The Gap

We built:
- `SuperInstance/constraint-theory-core` — CDCL solver, AC-3, rigidity, holonomy
- `SuperInstance/plato-llvm-bridge` — PLATO tiles → LLVM IR emitter
- `SuperInstance/avx512-constraint-checker` — FM's 35.9B/s AVX-512 engine

**Missing:** LLVM backend that connects constraint-theory-core CDCL traces to AVX-512 execution.

## What This Does

```text
PLATO Tiles → constraint-theory-core (CDCL) → Trace → LLVM IR → AVX-512
```

1. **CDCL Trace**: Records all decisions, propagations, conflicts, backtracks
2. **LLVM Emitter**: Converts trace to AVX-512 LLVM IR
3. **AVX-512 Optimizer**: Applies FM's constraint engine optimizations

## Key Components

### trace.rs — CDCL Execution Trace
Records the full execution of the CDCL solver:
- `Decide` events (branching choices)
- `Propagate` events (unit propagation)
- `Conflict` events (constraint violations)
- `Backtrack` events (learning)
- `Learn` events (new clauses added)

### emitter.rs — LLVM IR Generation
Converts traces to AVX-512 LLVM IR:
- 64-byte cache-aligned constraint records (FM's format)
- 16×16 = 256 checks per AVX-512 call
- HDC bloom pre-filter insertion

### optimizer.rs — AVX-512 Optimizations
- Bloom pre-filter: bypass 80-90% of constraints
- Batch SIMD: 16 constraints per vector
- Cache alignment: zero-latency constraint access

## Architecture

```text
┌─────────────┐     ┌─────────────────────┐     ┌─────────────┐
│ PLATO Tiles │────▶│ constraint-theory- │────▶│   LLVM IR   │
│  (inputs)   │     │     core CDCL       │     │   (trace)   │
└─────────────┘     └─────────────────────┘     └──────┬──────┘
                                                       │
                                                       ▼
                    ┌─────────────────────────────────────────┐
                    │     avx512-constraint-checker (FM's)     │
                    │  35.9B/s: 256 checks/call, HDC bloom   │
                    └─────────────────────────────────────────┘
```

## Why This Matters

- NO GPU has ASIL D / DAL A certification
- AVX-512 (Ryzen AI 9) is certifiable path to DO-254 DAL A
- CDCL traces capture learned constraints = execute at memory bandwidth
- LLVM backend bridges FM's two separate breakthroughs

## Integration

```rust
use constraint_theory_llvm::{CDCLTrace, LLVMEmitter, EmitterConfig};

let trace = CDCLTrace::new();
// ... record decisions, propagations, conflicts ...

let emitter = LLVMEmitter::new(EmitterConfig::default());
let ir = emitter.emit_trace(&trace);
let optimized = AVX512Optimizer::optimize(&ir, OptimizationLevel::Aggressive);
// Compile and link with FM's avx512-constraint-checker
```

## Related Repos

- `SuperInstance/constraint-theory-core` — CDCL solver, SAT, CSP
- `SuperInstance/plato-llvm-bridge` — PLATO → LLVM IR
- `SuperInstance/avx512-constraint-checker` — FM's constraint engine
- `SuperInstance/holonomy-consensus` — zero-holonomy fleet consensus

**License:** MIT — SuperInstance
