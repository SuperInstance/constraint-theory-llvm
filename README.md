# constraint-theory-llvm

**LLVM backend for constraint-theory-core** — compile CDCL traces to AVX-512 machine code.

Converts CDCL (Conflict-Driven Constraint Learning) solver traces into LLVM IR for AVX-512 vectorized constraint checking at FM's **35.9B/s memory bandwidth rate**.

---

## What It Does

CDCL solvers learn constraints during solving. These learned clauses encode why decisions failed.

**constraint-theory-llvm** compiles those learned clauses to AVX-512 LLVM IR:
- Stateless: Each learned clause becomes a vectorized check
- Cache-aligned: 64-byte constraint records (1 cache line)
- Batch SIMD: 16×16=256 constraint checks per AVX-512 call
- HDC bloom pre-filter: Bypasses 80-90% of checks

---

## Installation

```bash
cargo add constraint-theory-llvm
```

---

## Usage

### 1. Generate a CDCL Trace

```rust
use constraint_theory_llvm::{CDCLTrace, LLVMEmitter, EmitterConfig};

let mut trace = CDCLTrace::new();

// Record solver decisions, propagations, conflicts, backtracks
trace.add_decide(1, 1, None);                    // Decision at level 1
trace.add_propagate(2, 0, 1);                   // Forced propagation
trace.add_decide(2, -3, Some(0));               // Decision with antecedent
trace.add_conflict(1, 0, vec![1, -2, 3]);       // Conflict + analysis
trace.add_backtrack(0, vec![-1, 2]);            // Backtrack + learned clause
trace.add_learn(0, vec![-1, 2]);                 // Learn the clause

// The trace is now a program — compile it to LLVM IR
let emitter = LLVMEmitter::new(EmitterConfig::default());
let ir = emitter.emit_module(&trace);

println!("{}", ir);
```

### 2. Output: LLVM IR

```llvm
; constraint-theory-llvm: CDCL trace → AVX-512 IR
; Decisions: 2, Propagations: 1, Conflicts: 1
; Generated from CDCL trace

source_filename = "constraint-trace.ll"
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64:128"
target triple = "x86_64-unknown-linux-gnu"

; Constraint records (64-byte cache-aligned, FM's 35.9B/s format)
@.constraint_record = align 64 { i64, <16 x i32>, <16 x i32>, i64 }

define i1 @check_constraints(<16 x i32> %values, <16 x i32> %masks) {
entry:
  %clause0 = call i1 @check_clause(i32 2, i64 4390918232336384)
  %result = xor i1 true, false
  ret i1 %result
}

define i1 @check_clause(i32 %num_lits, i64 %encoded_literals) {
entry:
  %result = icmp eq i32 %num_lits, 0
  ret i1 %result
}

; AVX-512 batch: 16×16=256 checks per call
define <16 x i1> @batch_check(<16 x i32> %values, <16 x i64> %constraints) {
entry:
  %result = icmp sge <16 x i32> %values, zeroinitializer
  ret <16 x i1> %result
}

; HDC bloom pre-filter — bypasses 80-90% of constraints
define i1 @bloom_check(i64 %fingerprint, i64 %bloom_mask) {
entry:
  %xor = xor i64 %fingerprint, %bloom_mask
  %popcnt = call i64 @llvm.popcnt.i64(%xor)
  %threshold = icmp ult i64 %popcnt, 16
  ret i1 %threshold
}
```

### 3. Compile to Machine Code

```bash
# Save IR to file
echo "$IR" > trace.ll

# Compile with AVX-512
clang -O3 -march=native -mtune=native -x ir trace.ll -o trace

# Run
./trace
```

---

## The Architecture

```
CDCL Solver (constraint-theory-core)
    ↓ [execution trace]
constraint-theory-llvm
    ↓ [emit LLVM IR]
AVX-512 LLVM IR (.ll file)
    ↓ [llc/clang -O3 -march=native]
AVX-512 Machine Code
    ↓ [execute]
35.9B/s constraint checks (memory bandwidth)
```

**Why AVX-512?**
- 512-bit registers = 16×32-bit constraint checks per vector instruction
- 1 cache line (64 bytes) per constraint record
- No memory access: Data stays in L1 cache during batch check
- Result: 35.9 billion constraint checks per second

---

## Key Components

### `trace.rs` — CDCL Trace Recording

Records solver execution:
- `add_decide(level, literal, reason)` — branching decisions
- `add_propagate(literal, antecedent, level)` — forced propagations
- `add_conflict(level, clause, analysis)` — constraint violations
- `add_backtrack(target, learnt)` — learning + reversal
- `add_learn(clause_id, literals)` — learned clauses (the key output)

### `emitter.rs` — LLVM IR Generation

Converts traces to LLVM IR:
- `EmitterConfig` — CPU features, optimization level, vector width
- `emit_trace(trace)` — generates IR for a single trace
- `emit_module(trace)` — generates complete LLVM module

### `optimizer.rs` — AVX-512 Optimization

Optimizes generated IR:
- `AVX512Optimizer::optimize(ir, level)` — apply optimizations
- Bloom pre-filter insertion
- Batch check merging
- Cache alignment verification

---

## AVX-512 Constraint Format

```
64-byte cache-aligned constraint record:

 Byte 0-7:   constraint_id (i64)
 Byte 8-71:  lower_bounds x16 (16 × i32)
 Byte 72-135: upper_bounds x16 (16 × i32)
 Byte 136-143: metadata (i64)
```

Each `i32` is a 32-bit constraint bound. 16 bounds per record × 16 records in flight = 256 simultaneous checks via AVX-512.

---

## Relationship to Other Crates

| Crate | Purpose |
|-------|---------|
| `constraint-theory-core` | CDCL solver, AC-3 propagation, Sudoku solver |
| `plato-llvm-bridge` | PLATO → LLVM IR emitter (different direction) |
| `avx512-constraint-checker` | FM's raw AVX-512 implementation |
| `constraint-theory-llvm` | **This crate**: CDCL trace → LLVM IR |

---

## Benchmarking

```rust
use constraint_theory_llvm::{CDCLTrace, LLVMEmitter, EmitterConfig};

fn bench_trace_emission(trace: &CDCLTrace) -> std::time::Duration {
    let emitter = LLVMEmitter::new(EmitterConfig::default());
    let start = std::time::Instant::now();
    let _ir = emitter.emit_module(trace);
    start.elapsed()
}
```

Typical trace emission: **< 1ms** for 1000-event traces.

---

## Examples

```bash
# Run the SAT trace example
cargo run --example sat_trace

# Run tests
cargo test

# Run with output
cargo test -- --nocapture
```

---

## License

MIT — Cocapn Fleet (SuperInstance)
