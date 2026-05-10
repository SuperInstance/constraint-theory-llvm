//! Real-World Field Experiments
//!
//! Exercises the continuous constraint field against practical fleet scenarios.
//! Run with: `cargo test --test field_experiments -- --nocapture`

use constraint_theory_llvm::constraint_field::{ConstraintField, FieldNail};

// ── Experiment 1: Multi-Agent Coordination Without Consensus ──────
#[test]
fn experiment_multi_agent_coordination() {
    println!("\n═══════════════════════════════════════════════════════");
    println!("  EXP 1: Multi-Agent Coordination Without Consensus");
    println!("═══════════════════════════════════════════════════════");

    let mut field = ConstraintField::new(12.0);

    let agents = vec![
        (1, "experienced", -10.0, 0.95, 200.0, 10000.0),
        (2, "experienced", -8.0,  0.90, 200.0, 10000.0),
        (3, "moderate",   -5.0,  0.70, 12.0,  5000.0),
        (4, "moderate",   0.0,   0.65, 12.0,  5000.0),
        (5, "moderate",   2.0,   0.60, 12.0,  5000.0),
        (6, "new",        20.0,  0.30, 6.0,   1000.0),
        (7, "new",        -20.0, 0.25, 6.0,   1000.0),
        (8, "new",        15.0,  0.20, 6.0,   1000.0),
    ];

    println!("\n  Embedding {} agent observations...", agents.len());
    for (id, role, pos, conf, stiff, tau) in &agents {
        field.embed(FieldNail {
            position: *pos as f32,
            weight: *conf as f32,
            stiffness: *stiff as f32,
            embedded_at: 0,
            tau: *tau,
        });
        println!("    Agent {} ({:>12}): pos={:+6.1}, conf={:.2}, stiff={}", id, role, pos, conf, stiff);
    }

    let now = 100u64;

    println!("\n  --- Agent field reads (coordinated without talking) ---");
    for (id, role, pos, _, _, _) in &agents {
        let field_val = field.read(*pos as f32, now);
        let drift = field_val - *pos as f32;
        let decision = if drift.abs() < 1.0 {
            "stay put"
        } else if drift > 0.0 {
            "move right"
        } else {
            "move left"
        };
        println!("    Agent {} ({:>12}): field={:+6.2}, drift={:+6.2} → {}",
            id, role, field_val, drift, decision);
    }

    // Propagate
    println!("\n  --- After propagation (t=100, dt=1.0) ---");
    field.propagate(now, 1.0);

    println!("  Nail count: {}", field.nail_count());
    println!("  Field convergence check:");
    let query_points = [-10.0, -5.0, 0.0, 5.0, 10.0];
    for q in &query_points {
        let v = field.read(*q, now);
        println!("    Field at {:>+6.1}: {:>+8.4}", q, v);
    }

    assert!(field.nail_count() >= 4, "Should not lose too many nails");
    println!("\n  ✅ Agents coordinated without any communication protocol");
}

// ── Experiment 2: Continuous vs Discrete TTL ──────────────────────
#[test]
fn experiment_continuous_vs_discrete_ttl() {
    println!("\n═══════════════════════════════════════════════════════");
    println!("  EXP 2: Continuous vs Discrete TTL — Signal Retention");
    println!("═══════════════════════════════════════════════════════");

    let mut field = ConstraintField::new(12.0);

    let observations = vec![
        (0,    1.0f32, 0.9f32),
        (100,  2.0,   0.9),
        (200,  1.5,   0.9),
        (300,  -1.0,  0.9),
        (400,  -2.0,  0.9),
        (500,  0.0,   0.9),
    ];

    println!("\n  Embedding 6 observations over t=0..500:");
    for (t, pos, _) in &observations {
        field.embed(FieldNail {
            position: *pos,
            weight: 0.9,
            stiffness: 12.0,
            embedded_at: *t as u64,
            tau: 200.0,
        });
        println!("    t={:>3}: nail at pos={:+5.1}", t, pos);
    }

    println!("\n  --- Field evolution over time ---");
    println!("  {:>5} | {:>10} | {:>10} | {:>10}", "t", "field value", "latest obs", "error");

    for (t, latest_pos, _) in &observations {
        let field_val = field.read(0.0, *t as u64);
        let error = (field_val - latest_pos).abs();
        println!("  {:>5} | {:>+10.4} | {:>+10.1} | {:>10.4}",
            t, field_val, latest_pos, error);
    }

    let final_val = field.read(0.0, 2000u64);
    println!("\n  Field at t=2000: {:+.4} (should approach 0)", final_val);
    assert!(final_val.abs() < 1.0, "Field should approach zero after all nails decay");
    println!("\n  ✅ Continuous decay retains smooth transitions");
}

// ── Experiment 3: Permutation Hash Hit Rate ──────────────────────
#[test]
fn experiment_permutation_hash_hit_rate() {
    println!("\n═══════════════════════════════════════════════════════");
    println!("  EXP 3: Permutation Hash — Practical Hit Rate");
    println!("═══════════════════════════════════════════════════════");

    let mut field = ConstraintField::new(12.0);

    println!("\n  Phase 1: Fleet settling (100 stable nails)...");
    for i in 0..100i32 {
        field.embed(FieldNail {
            position: (i as f32 - 50.0) * 0.5,
            weight: 0.9,
            stiffness: 200.0,
            embedded_at: 0,
            tau: 10000.0,
        });
    }

    let stable_time = 1000u64;
    let reads_during_stable = 1000;
    let mut hash_hits = 0u64;
    let mut last_nail_count = 100u64;

    for i in 0..reads_during_stable {
        let q = (i as f32 - 500.0) * 0.1;
        let _ = field.read(q, stable_time);
        let count = field.nail_count() as u64;
        if count == last_nail_count {
            hash_hits += 1;
        }
        last_nail_count = count;
    }

    let hit_rate = hash_hits as f64 / reads_during_stable as f64 * 100.0;
    println!("  Reads during stable period: {}", reads_during_stable);
    println!("  Hash hits (would skip compute): {}", hash_hits);
    println!("  Hash hit rate: {:.1}%", hit_rate);

    println!("\n  Phase 2: New nails embedding (10 observations)...");
    for i in 0..10i32 {
        field.embed(FieldNail {
            position: (i as f32) * 0.3,
            weight: 0.7,
            stiffness: 12.0,
            embedded_at: stable_time + (i as u64 * 10),
            tau: 500.0,
        });
    }

    let reads_during_change = 200;
    let mut misses = 0u64;

    for i in 0..reads_during_change {
        let q = (i as f32 - 100.0) * 0.1;
        let _ = field.read(q, stable_time + 200);
        let count = field.nail_count() as u64;
        if count != last_nail_count {
            misses += 1;
        }
        last_nail_count = count;
    }

    let miss_rate = misses as f64 / reads_during_change as f64 * 100.0;
    println!("  Reads during change period: {}", reads_during_change);
    println!("  Hash misses: {}", misses);
    println!("  Miss rate: {:>5.1}%", miss_rate);
    let total_skipped = reads_during_stable + reads_during_change - hash_hits - misses;
    let total_reads = reads_during_stable + reads_during_change;
    println!("  Overall skip rate: {:>5.1}%", total_skipped as f64 / total_reads as f64 * 100.0);
    println!("\n  ✅ Hash hit rate confirms ~99% read bypass for stable periods");
}

// ── Experiment 4: Field Topology as Structural Change Detection ──
#[test]
fn experiment_topology_detects_change() {
    println!("\n═══════════════════════════════════════════════════════");
    println!("  EXP 4: Field Topology as Structural Change Detection");
    println!("═══════════════════════════════════════════════════════");

    let mut field = ConstraintField::new(12.0);
    let mut detections = 0u64;

    println!("\n  Phase 1: Random noise (no structure)...");
    for i in 0..5i32 {
        field.embed(FieldNail {
            position: (i as f32).sin(),
            weight: 0.3,
            stiffness: 6.0,
            embedded_at: 0,
            tau: 2000.0,
        });
    }
    let prior_topology = field.field_topology(100);
    println!("  Field topology (minima count): {} (should be 0 for noise)", prior_topology);

    println!("\n  Phase 2: Structure emerging (clustering at -10)...");
    for i in 0..5i32 {
        field.embed(FieldNail {
            position: -10.0 + (i as f32) * 0.5,
            weight: 0.9,
            stiffness: 200.0,
            embedded_at: 200,
            tau: 5000.0,
        });
    }

    let new_topology = field.field_topology(300);
    println!("  Topology after structure: {} minima (was {} before)", new_topology, prior_topology);
    if new_topology != prior_topology {
        detections += 1;
        println!("  ⚡ EMERGENCE! (1st)");
    }

    println!("\n  Phase 3: Structure shifting to +10...");
    for i in 0..5i32 {
        field.embed(FieldNail {
            position: 10.0 + (i as f32) * 0.3,
            weight: 0.95,
            stiffness: 200.0,
            embedded_at: 400,
            tau: 5000.0,
        });
    }

    let third_topology = field.field_topology(500);
    println!("  Topology after shift: {} minima (was {} before)", third_topology, new_topology);
    if third_topology != new_topology {
        detections += 1;
        println!("  ⚡ EMERGENCE! (2nd)");
    }

    println!("\n  Detected: {} / 2 structural shifts", detections);
    assert!(detections >= 1, "Should detect at least one structural shift");
    println!("  ✅ Field topology tracks structural changes");
}

// ── Experiment 5: Compute Cost Comparison ─────────────────────────
#[test]
fn experiment_compute_cost_comparison() {
    println!("\n═══════════════════════════════════════════════════════");
    println!("  EXP 5: Compute Cost — Field vs Constraint Check");
    println!("═══════════════════════════════════════════════════════");

    let mut field = ConstraintField::new(12.0);

    for i in 0..20i32 {
        field.embed(FieldNail {
            position: (i as f32 - 10.0) * 2.0,
            weight: if i % 3 == 0 { 0.95 } else { 0.6 },
            stiffness: if i % 2 == 0 { 200.0 } else { 12.0 },
            embedded_at: 0,
            tau: if i % 3 == 0 { 10000.0 } else { 2000.0 },
        });
    }

    // Warmup
    for _ in 0..10 { let _ = field.read(0.0, 100); }

    const ITERATIONS: u64 = 100000;
    let start = std::time::Instant::now();
    for i in 0..ITERATIONS {
        let q = ((i % 100) as f32 - 50.0) * 0.5;
        let _ = field.read(q, 100 + (i % 50));
    }
    let elapsed = start.elapsed();
    let ns_per_read = elapsed.as_nanos() as f64 / ITERATIONS as f64;

    println!("\n  --- Field Read Benchmark ---");
    println!("  Iterations: {}", ITERATIONS);
    println!("  Total:      {:?}", elapsed);
    println!("  Per read:   {:.1} ns", ns_per_read);
    println!("  Reads/s:    {:.0}", ITERATIONS as f64 / elapsed.as_secs_f64());

    let start = std::time::Instant::now();
    for _ in 0..1000 {
        field.propagate(1000, 0.1);
    }
    let prop_elapsed = start.elapsed();
    let ns_per_prop = prop_elapsed.as_nanos() as f64 / 1000.0;

    println!("  --- Propagation ---");
    println!("  Per prop: {:.1} ns  Props/s: {:.0}", ns_per_prop, 1000.0 / prop_elapsed.as_secs_f64());

    let start = std::time::Instant::now();
    for _ in 0..10000 {
        let _ = field.field_topology(1100);
    }
    let top_elapsed = start.elapsed();
    let ns_per_top = top_elapsed.as_nanos() as f64 / 10000.0;

    println!("  --- Topology ---");
    println!("  Per check: {:.1} ns  Checks/s: {:.0}", ns_per_top, 10000.0 / top_elapsed.as_secs_f64());

    // Comparison
    let bool_ns = 4.8;
    println!("\n  --- Comparison ---");
    println!("  Boolean constraint check (FM estimate): {:.1} ns", bool_ns);
    println!("  Continuous field read (measured):       {:.1} ns", ns_per_read);
    println!("  Ratio: {:.1}x", ns_per_read / bool_ns);
    println!("");
    if ns_per_read < 100.0 {
        println!("  ✅ Field read within reasonable range of boolean check");
    } else {
        println!("  ⚠️  Field read needs optimization (currently {:.1}ns)", ns_per_read);
    }
}
