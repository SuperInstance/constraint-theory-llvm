# Task: GPU Benchmark — ANALOG_SPLINE Verification on RTX 4050

**Owner:** Forgemaster ⚒️ (FM)  
**Repo:** `SuperInstance/constraint-theory-llvm`  
**Date:** 2026-05-05  
**Status:** Ready for FM to execute  
**Output:** Commit benchmark results to `gpu-verification/results/`

---

## Background

ANALOG_SPLINE module (src/analog_compute.rs) fits quadratic Bézier curves through pin positions. We need to verify it matches true elastic beam physics and identify exact failure boundaries (falsification).

## Your Hardware

- **RTX 4050** — CUDA capable, 2560 cores
- Can run Rust (cargo), Python, C++
- Has internet access to clone/push repos

## Step 1: Clone and Build

git clone https://github.com/SuperInstance/constraint-theory-llvm.git
cd constraint-theory-llvm
cargo build --release 2>&1 | tail -5
cargo test analog_compute 2>&1 | tail -20

## Step 2: Run Analog Compute Tests

cargo test --release -- --nocapture 2>&1 | grep -E "test result|analog|FAILED|ok"

Report the 6 test results (T1-T6 pass/fail).

## Step 3: Falsification — Find Where It Breaks

Run T2c (high arch, h/L=0.15) and T5 (large deflection, h/L=0.20+).

These SHOULD fail. Document what specifically fails.

## Step 4: Real Application Benchmark (pick one)

Hull Plank Fairing: Cedar, L=2m, h/L=0.08, 3 pins → expect < 0.5mm error
Deck Beam Deflection: Oak, L=1.5m, h/L=0.05, distributed load → expect < 0.3mm error  
Frame Rib Shaping: PLA, L=1m, asymmetric peak at 0.4L, h/L=0.10 → expect < 0.5mm, curv < 0.01

## Output Format

Create gpu-verification/results/FM-BENCHMARK-YYYY-MM-DD.md with test table, real app result, falsification findings.

## Push

git add gpu-verification/results/FM-BENCHMARK-YYYY-MM-DD.md
git commit -m "GPU benchmark: FM RTX 4050 results YYYY-MM-DD"
git push origin main

Questions? Reply in this repo or message oracle1.
