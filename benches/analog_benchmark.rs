//! Phase 2: Benchmark analog_spline vs digital interpolation
//! Run with: cargo test --test analog_benchmark -- --nocapture

use std::time::Instant;
use crate::analog_compute::{analog_spline, analog_water_level, analog_story_pole, analog_sector, Material};

#[test]
fn test_storage_comparison() {
    println!("\n=== Storage Comparison ===");
    for n in [10, 50, 100] {
        let spline_bytes = 3 * 2 * 4 + 4; // 3 points × 2 f32 × 4 bytes + material
        let absolute_bytes = n * 2 * 8; // N tiles × 2 dims × 8 bytes
        let delta_bytes = n * 2 * 4; // N tiles × 2 dims × 4 bytes
        println!("{} tiles: absolute={} bytes, spline={}, deltas={}", 
                 n, absolute_bytes, spline_bytes, delta_bytes);
    }
}

#[test]
fn test_benchmark_latency() {
    let points = [(0.0_f32, 0.0), (1.0, 2.0), (2.0, 0.0)];
    let material = Material::Oak;
    
    // Spline
    let start = Instant::now();
    for _ in 0..10000 {
        let _ = analog_spline(&points, material, 0.5);
    }
    let ns = start.elapsed().as_nanos() as f64 / 10000.0;
    println!("\n=== Latency (10k iterations) ===");
    println!("analog_spline: {:.0} ns/call", ns);
    
    // Water level
    let pts: Vec<(f32,f32)> = (0..100).map(|i| (i as f32, (i as f32).sin())).collect();
    let start = Instant::now();
    for _ in 0..10000 {
        let _ = analog_water_level(&pts);
    }
    let ns = start.elapsed().as_nanos() as f64 / 10000.0;
    println!("analog_water_level (100pts): {:.0} ns/call", ns);
    
    // Story pole
    let deltas: Vec<f32> = (0..16).map(|i| i as f32).collect();
    let start = Instant::now();
    for _ in 0..10000 {
        let _ = analog_story_pole(10.0, &deltas);
    }
    let ns = start.elapsed().as_nanos() as f64 / 10000.0;
    println!("analog_story_pole (16 deltas): {:.0} ns/call", ns);
    
    // Sector
    let start = Instant::now();
    for _ in 0..10000 {
        let _ = analog_sector(100.0, 10);
    }
    let ns = start.elapsed().as_nanos() as f64 / 10000.0;
    println!("analog_sector: {:.0} ns/call", ns);
}

#[test]
fn test_smoothness() {
    let points = [(0.0_f32, 0.0), (1.0, 2.0), (2.0, 0.0)];
    let result = analog_spline(&points, Material::Oak, 0.5);
    let curve = &result.curve_points;
    let mid = curve.len() / 2;
    
    println!("\n=== Smoothness Check ===");
    println!("Curve points: {}", curve.len());
    println!("Curvature at mid-1: {:.6}", curve[mid-1].2);
    println!("Curvature at mid: {:.6}", curve[mid].2);
    println!("Curvature jump: {:.6}", (curve[mid].2 - curve[mid-1].2).abs());
}
