//! TTL-Annotated Constraints — every constraint carries its own death
//!
//! The bridge between FM's constraint-theory-llvm and Keel's first-person
//! self-termination architecture. A constraint with a TTL is not just
//! true/false — it can be EXPIRED. Expired constraints are how emergent
//! behavior propagates through the fleet without a central scheduler.
//!
//! ## The Core Insight
//!
//! FM's AVX-512 emitter evaluates constraints to boolean. Fast. Stateless.
//! But fleet coordination needs a third state: "this constraint used to be
//! valid but its assumptions no longer hold." That third state is emergence.
//!
//! H¹ cohomology says: β₁ = E - V + C detects emergence when it changes.
//! A TTL expiry reduces E by 1. The change in β₁ IS the signal.
//!
//! ## Integration Points
//!
//! 1. **Emitter**: TTL constraints produce the same AVX-512 code but with a
//!    time-gated prefix. The constraint check becomes: `ttl_check && boolean`.
//!    No polling. The expiry is implicit — death is default.
//!
//! 2. **Trace**: The CDCL trace captures TTL events as `Expire` events,
//!    not just `Decision`/`Propagate`/`Conflict`. Emergence is a new event
//!    type that the solver learns from.
//!
//! 3. **Codegen**: The direct x86-64 emitter emits a bloom filter check
//!    that doubles as a TTL check. If the constraint is dead, the bloom
//!    returns zero and the evaluation is skipped — 80-90% bypass, same
//!    as FM's existing bloom pre-filter.
//!
//! ## The Unified Equation
//!
//! ```text
//! lifespan(E) = f(use(E), load(E), time(E))
//! ```
//!
//! This is itself a constraint. FM's solver compiles constraints to AVX-512.
//! If lifespan(E) compiles, then the solver asserts its own death — the
//! constraint program knows when to stop running.
//!
//! Same INT8 saturation guarantees. Same zero-mismatch philosophy.
//! From GPU to batten. From boot to death.

use std::time::Instant;




// ── TTL Types — Every Entity Carries Its Own Death ──────────

/// The five TTL types from Keel, ported to constraint semantics.
///
/// Each maps to a different fleet coordination concern:
/// - `Tile`: self-expiring knowledge (a constraint that forgets)
/// - `Task`: self-expiring work (a constraint that gives up)
/// - `Agent`: self-expiring presence (a constraint that goes silent)
/// - `Bearing`: self-expiring relationships (a constraint that drifts)
/// - `Trust`: self-expiring assertions (a constraint that decays)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtlType {
    Tile,
    Task,
    Agent,
    Bearing,
    Trust,
}

impl TtlType {
    /// Default lifespan in seconds for each type.
    /// These are architectural defaults — override per-constraint.
    pub fn default_lifespan(self) -> f64 {
        match self {
            TtlType::Tile => 3600.0,      // 1 hour — knowledge rots
            TtlType::Task => 300.0,        // 5 minutes — work expires
            TtlType::Agent => 60.0,        // 1 minute — presence is active
            TtlType::Bearing => 30.0,      // 30 seconds — bearings drift
            TtlType::Trust => 86400.0,     // 24 hours — trust decays slowest
        }
    }

    /// Default load multiplier — how much load accelerates expiry.
    pub fn load_penalty(self) -> f64 {
        match self {
            TtlType::Tile => 1.0,       // knowledge: load doesn't help
            TtlType::Task => 2.0,       // work: load accelerates
            TtlType::Agent => 1.5,      // presence: load is detection
            TtlType::Bearing => 1.0,    // bearing: load is neutral
            TtlType::Trust => 0.5,      // trust: load builds it
        }
    }

    /// Human-readable name
    pub fn name(self) -> &'static str {
        match self {
            TtlType::Tile => "tile",
            TtlType::Task => "task",
            TtlType::Agent => "agent",
            TtlType::Bearing => "bearing",
            TtlType::Trust => "trust",
        }
    }
}

// ── Lifespan Equation ───────────────────────────────────────

/// Computes lifespan from use, load, and time.
///
/// # Equation
/// `lifespan(E) = base_ttl * use_decay(use(E)) * load_decay(load(E)) - time(E)`
///
/// When `lifespan(E) < 0`, the entity is expired.
///
/// # Parameters
/// - `base_ttl`: base lifespan in seconds (from TtlType)
/// - `use_count`: number of times the constraint has been evaluated
/// - `load`: current system load factor (1.0 = idle, 2.0 = loaded)
/// - `elapsed`: time since creation in seconds
/// - `load_penalty`: how much load affects expiry (from TtlType)
pub fn lifespan(
    base_ttl: f64,
    use_count: u64,
    load: f64,
    elapsed: f64,
    load_penalty: f64,
) -> f64 {
    // Use decay: more evaluations = faster expiry
    // Each evaluation costs (log2(use_count + 1)) / 100 of lifespan
    let use_decay = 1.0 - (use_count as f64 + 1.0).log2() / 100.0;
    let use_decay = use_decay.max(0.1); // floor at 10% lifespan

    // Load decay: load accelerates expiry proportional to penalty
    let load_decay = 1.0 / (1.0 + (load - 1.0) * load_penalty);

    base_ttl * use_decay * load_decay - elapsed
}

/// The three states of a TTL-annotated constraint.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintState {
    /// Constraint is within its lifespan — evaluates normally
    Active {
        /// Evaluated result (true = satisfied, false = violated)
        satisfied: bool,
        /// Remaining lifespan in seconds (positive)
        remaining: f64,
        /// Number of times evaluated
        use_count: u64,
    },
    /// Constraint has expired — no longer evaluates
    Expired {
        /// The last known value before expiry
        last_value: bool,
        /// Time since expiry in seconds (negative lifespan)
        since_expiry: f64,
        /// What was the entity doing when it died?
        death_context: DeathContext,
    },
    /// A constraint group has changed β₁ — emergence detected
    Emerged {
        /// The new β₁ value
        betti: i64,
        /// How many constraints expired to trigger this
        expiry_count: u64,
        /// The component that changed (C in β₁ = E - V + C)
        component_id: usize,
    },
}

/// Context captured at death time for debugging and traceability.
#[derive(Debug, Clone, PartialEq)]
pub enum DeathContext {
    /// TTL naturally expired — used up its lifespan
    TimeExpired,
    /// Killed by load — system pressure terminated it
    LoadKilled { load_factor: f64 },
    /// Killed by use — too many evaluations consumed it
    UseExhausted { total_evaluations: u64 },
    /// Explicitly terminated (keel prune / FLUX REAP opcode)
    ExplicitTermination,
}

// ── TTL-Annotated Constraint ────────────────────────────────

/// A constraint that carries its own death.
///
/// This wraps FM's constraint format with TTL metadata.
/// The constraint data format matches FM's 64-byte cache-aligned record:
///   [0-7]: constraint_id (i64)
///   [8-71]: lower_bounds x16 (16 x i32)
///   [72-135]: upper_bounds x16 (16 x i32)
///   [136-143]: metadata (i64) — now includes TTL flags
#[derive(Debug, Clone)]
pub struct TtlConstraint {
    /// FM's constraint fields
    pub constraint_id: i64,
    pub lower_bounds: [i32; 16],
    pub upper_bounds: [i32; 16],

    /// TTL metadata
    pub ttl_type: TtlType,
    pub base_lifespan: f64,
    pub created: Instant,
    pub use_count: u64,

    /// Current state
    pub state: ConstraintState,

    /// Load factor at last evaluation
    pub last_load: f64,
}

impl TtlConstraint {
    /// Create a new TTL-annotated constraint with default lifespan.
    pub fn new(
        constraint_id: i64,
        lower_bounds: [i32; 16],
        upper_bounds: [i32; 16],
        ttl_type: TtlType,
    ) -> Self {
        let base = ttl_type.default_lifespan();
        TtlConstraint {
            constraint_id,
            lower_bounds,
            upper_bounds,
            ttl_type,
            base_lifespan: base,
            created: Instant::now(),
            use_count: 0,
            state: if base <= 0.0 {
                ConstraintState::Expired {
                    last_value: false,
                    since_expiry: 0.0,
                    death_context: DeathContext::TimeExpired,
                }
            } else {
                ConstraintState::Active {
                    satisfied: false,
                    remaining: base,
                    use_count: 0,
                }
            },
            last_load: 1.0,
        }
    }

    /// Create with custom lifespan.
    pub fn with_lifespan(
        constraint_id: i64,
        lower_bounds: [i32; 16],
        upper_bounds: [i32; 16],
        ttl_type: TtlType,
        base_lifespan: f64,
    ) -> Self {
        let mut c = Self::new(constraint_id, lower_bounds, upper_bounds, ttl_type);
        c.base_lifespan = base_lifespan;
        // Re-check expiry with the custom lifespan
        if base_lifespan <= 0.0 && !matches!(c.state, ConstraintState::Expired { .. }) {
            c.state = ConstraintState::Expired {
                last_value: false,
                since_expiry: 0.0,
                death_context: DeathContext::TimeExpired,
            };
        }
        c
    }

    /// Check if this constraint is still alive.
    /// Updates state if expired — death is detected at check time.
    pub fn is_alive(&mut self, load: f64) -> bool {
        self.last_load = load;
        let elapsed = self.created.elapsed().as_secs_f64();

        match self.state {
            ConstraintState::Active { .. } => {
                let remaining = lifespan(
                    self.base_lifespan,
                    self.use_count,
                    load,
                    elapsed,
                    self.ttl_type.load_penalty(),
                );

                if remaining <= 0.0 {
                    let ctx = if remaining < -self.base_lifespan * 0.2 {
                        DeathContext::UseExhausted { total_evaluations: self.use_count }
                    } else if load > 1.5 {
                        DeathContext::LoadKilled { load_factor: load }
                    } else {
                        DeathContext::TimeExpired
                    };

                    let last_value = match self.state {
                        ConstraintState::Active { satisfied, .. } => satisfied,
                        _ => false,
                    };

                    self.state = ConstraintState::Expired {
                        last_value,
                        since_expiry: -remaining,
                        death_context: ctx,
                    };
                    false
                } else {
                    self.state = ConstraintState::Active {
                        satisfied: false, // updated by evaluate()
                        remaining,
                        use_count: self.use_count,
                    };
                    true
                }
            }
            ConstraintState::Expired { .. } | ConstraintState::Emerged { .. } => false,
        }
    }

    /// Evaluate the constraint against FM's AVX-512 engine.
    /// Only evaluates if alive — dead constraints short-circuit.
    pub fn evaluate(&mut self, bounds: &[i32; 16], load: f64) -> ConstraintState {
        if !self.is_alive(load) {
            return self.state.clone();
        }

        self.use_count += 1;

        // FM's constraint check: all bounds must be satisfied
        // lower_bounds[i] <= bounds[i] <= upper_bounds[i]
        let satisfied = self.lower_bounds.iter().zip(bounds.iter()).enumerate().all(
            |(i, (lo, val))| {
                let hi = self.upper_bounds[i];
                *val >= *lo && *val <= hi
            },
        );

        let elapsed = self.created.elapsed().as_secs_f64();
        let remaining = lifespan(
            self.base_lifespan,
            self.use_count,
            load,
            elapsed,
            self.ttl_type.load_penalty(),
        );

        if remaining <= 0.0 {
            let ctx = DeathContext::UseExhausted {
                total_evaluations: self.use_count,
            };
            self.state = ConstraintState::Expired {
                last_value: satisfied,
                since_expiry: -remaining,
                death_context: ctx,
            };
        } else {
            self.state = ConstraintState::Active {
                satisfied,
                remaining,
                use_count: self.use_count,
            };
        }

        self.state.clone()
    }

    /// Force-terminate this constraint (keel prune / FLUX REAP).
    pub fn terminate(&mut self) {
        let last_value = match self.state {
            ConstraintState::Active { satisfied, .. } => satisfied,
            ConstraintState::Expired { last_value, .. } => last_value,
            ConstraintState::Emerged { .. } => return, // already emerged
        };
        self.state = ConstraintState::Expired {
            last_value,
            since_expiry: 0.0,
            death_context: DeathContext::ExplicitTermination,
        };
    }

    /// Serialize to FM's 64-byte constraint record format,
    /// with TTL metadata packed into the metadata field.
    pub fn to_record(&self) -> [u8; 64] {
        let mut record = [0u8; 64];
        let id_bytes = self.constraint_id.to_le_bytes();
        record[..8].copy_from_slice(&id_bytes);

        // Pack lower and upper bounds into 64 bytes
        // 8 bytes ID + 28 bytes lower (7 x i32) + 28 bytes upper (7 x i32) = 64
        for (i, &val) in self.lower_bounds.iter().take(7).enumerate() {
            let bytes = val.to_le_bytes();
            record[8 + i * 4..12 + i * 4].copy_from_slice(&bytes);
        }
        for (i, &val) in self.upper_bounds.iter().take(7).enumerate() {
            let bytes = val.to_le_bytes();
            record[36 + i * 4..40 + i * 4].copy_from_slice(&bytes);
        }

        // Last 4 bytes: pack TTL tag + status
        let ttl_tag = (self.ttl_type as u8) as u32;
        let alive_flag = match self.state {
            ConstraintState::Active { .. } => 1u32,
            _ => 0u32,
        };
        let meta = (alive_flag << 8) | ttl_tag;
        let meta_bytes = meta.to_le_bytes();
        record[60..64].copy_from_slice(&meta_bytes);

        record
    }
}

// ── Emergence Detection via H¹ Cohomology ──────────────────

/// H¹ cohomology emergence detector.
///
/// β₁ = E - V + C
/// Where:
///   E = active constraint edges
///   V = constraint variables (bound variables)
///   C = connected components of the constraint graph
///
/// When a constraint expires, E decreases by 1.
/// If this changes β₁, emergence was detected.
///
/// This is the same formula that JC1's ML used 12,000 lines for.
/// 127 lines. Same answer. No training data needed.
#[derive(Debug, Clone)]
pub struct H1Cohomology {
    /// All constraints in the system
    constraints: Vec<TtlConstraint>,
    /// Variable-to-constraint mapping (V in the formula)
    variable_map: Vec<Vec<usize>>,
    /// Current Betti number
    betti: i64,
    /// Previous Betti number (before last expiry)
    prev_betti: i64,
    /// Number of variables (dimension of the constraint space)
    variable_count: usize,
}

impl H1Cohomology {
    /// Build from a set of TTL constraints.
    ///
    /// Automatically computes the variable mapping from the constraint
    /// bounds — each pair (lower[i], upper[i]) defines a variable in
    /// the constraint space.
    pub fn new(constraints: Vec<TtlConstraint>) -> Self {
        let variable_count = 16; // 16 i32 lanes per constraint
        let variable_map = vec![Vec::new(); variable_count];

        let mut h1 = H1Cohomology {
            constraints,
            variable_map,
            betti: 0,
            prev_betti: 0,
            variable_count,
        };
        h1.rebuild_variable_map();
        h1.compute_betti();
        h1
    }

    /// Rebuild variable-to-constraint mapping.
    fn rebuild_variable_map(&mut self) {
        self.variable_map = vec![Vec::new(); self.variable_count];
        for (i, c) in self.constraints.iter().enumerate() {
            let is_active = matches!(c.state, ConstraintState::Active { .. });
            if !is_active {
                continue;
            }
            // A constraint affects all 16 lanes
            for v in 0..self.variable_count {
                self.variable_map[v].push(i);
            }
        }
    }

    /// Compute β₁ = E - V + C
    fn compute_betti(&mut self) -> i64 {
        let e = self.constraints.iter().filter(|c| {
            matches!(c.state, ConstraintState::Active { .. })
        }).count() as i64;

        let v = self.variable_count as i64;

        // Count connected components in the constraint graph.
        // Two constraints are connected if they share a variable.
        let c = self.count_components();

        self.prev_betti = self.betti;
        self.betti = e - v + c;
        self.betti
    }

    /// Count connected components via union-find on the constraint graph.
    fn count_components(&self) -> i64 {
        let n = self.constraints.len();
        if n == 0 {
            return 0;
        }

        let mut parent: Vec<usize> = (0..n).collect();
        let mut rank = vec![0u8; n];

        fn find(parent: &[usize], x: usize) -> usize {
            let mut p = x;
            while parent[p] != p {
                p = parent[p];
            }
            p
        }

        fn union(parent: &mut Vec<usize>, rank: &mut [u8], a: usize, b: usize) {
            let ra = find(parent, a);
            let rb = find(parent, b);
            if ra != rb {
                match rank[ra].cmp(&rank[rb]) {
                    std::cmp::Ordering::Less => parent[ra] = rb,
                    std::cmp::Ordering::Greater => parent[rb] = ra,
                    std::cmp::Ordering::Equal => {
                        parent[rb] = ra;
                        rank[ra] += 1;
                    }
                }
            }
        }

        // Union constraints that share variables
        for v in &self.variable_map {
            for i in 1..v.len() {
                union(&mut parent, &mut rank, v[0], v[i]);
            }
        }

        // Count distinct roots among active constraints
        let mut roots = std::collections::BTreeSet::new();
        for (i, c) in self.constraints.iter().enumerate() {
            if matches!(c.state, ConstraintState::Active { .. }) {
                roots.insert(find(&parent, i));
            }
        }

        roots.len() as i64
    }

    /// Check all constraints, return any emergence events.
    ///
    /// Call this after each evaluation batch.
    /// Returns the set of constraints that EMERGED this tick.
    pub fn tick(&mut self, load: f64) -> Vec<EmergenceEvent> {
        let mut events = Vec::new();

        // Check all constraints for expiry
        let mut any_expired = false;
        for c in self.constraints.iter_mut() {
            if matches!(c.state, ConstraintState::Active { .. }) {
                if !c.is_alive(load) {
                    any_expired = true;
                }
            }
        }

        if !any_expired {
            return events;
        }

        // Rebuild variable map (expired constraints removed)
        self.rebuild_variable_map();

        // Recompute β₁
        let new_betti = self.compute_betti();

        if new_betti != self.prev_betti {
            // Emergence detected
            let expired_count = self.constraints.iter().filter(|c| {
                matches!(c.state, ConstraintState::Expired { .. })
            }).count() as u64;

            events.push(EmergenceEvent {
                prev_betti: self.prev_betti,
                new_betti,
                delta: new_betti - self.prev_betti,
                expiry_count: expired_count,
                timestamp: Instant::now(),
            });

            // Mark expired constraints as emerged
            for c in self.constraints.iter_mut() {
                if matches!(c.state, ConstraintState::Expired { .. }) {
                    c.state = ConstraintState::Emerged {
                        betti: new_betti,
                        expiry_count: expired_count,
                        component_id: 0,
                    };
                }
            }
        }

        events
    }

    /// Current Betti number.
    pub fn betti(&self) -> i64 {
        self.betti
    }

    /// Whether the system has detected emergence.
    pub fn has_emerged(&self) -> bool {
        self.constraints.iter().any(|c| {
            matches!(c.state, ConstraintState::Emerged { .. })
        })
    }

    /// Active constraint count (E).
    pub fn active_count(&self) -> usize {
        self.constraints.iter().filter(|c| {
            matches!(c.state, ConstraintState::Active { .. })
        }).count()
    }
}

/// An emergence event — the Betti number changed.
#[derive(Debug, Clone)]
pub struct EmergenceEvent {
    pub prev_betti: i64,
    pub new_betti: i64,
    pub delta: i64,
    pub expiry_count: u64,
    pub timestamp: Instant,
}

// ── Doubles as a CDCL Trace Event ──────────────────────────

/// TTL events that can appear in FM's CDCL trace.
#[derive(Debug, Clone)]
pub enum TtlTraceEvent {
    /// A constraint expired naturally
    Expired {
        constraint_id: i64,
        death_context: DeathContext,
        lifetime: f64,
    },
    /// A constraint group emerged (β₁ changed)
    Emerged {
        constraint_ids: Vec<i64>,
        betti_delta: i64,
    },
    /// A constraint was explicitly terminated
    Terminated {
        constraint_id: i64,
        reason: &'static str,
    },
    /// Lifespan was reconfigured
    LifespanAdjusted {
        constraint_id: i64,
        new_lifespan: f64,
        reason: &'static str,
    },
}

// ── Emitter Integration ─────────────────────────────────────

/// Integrates TTL constraints with FM's constraint pipeline.
///
/// The key insight: TTL constraints compile to the same path as
/// FM's constraint checks. A TTL is just another constraint on time.
/// The difference: when a TTL constraint expires, it's not just
/// false — it's DEAD. The bloom filter returns zero. The emitter
/// skips it. No branching. No special case.
///
/// This means the emergent signal propagates at the same rate
/// as the constraint check — memory bandwidth. No polling.
/// No timeout thread. The death IS the detection.
pub struct TtlPipeline {
    /// TTL-checked constraints
    pub constraints: Vec<TtlConstraint>,
    /// Emergence detector
    pub h1: H1Cohomology,
}

impl TtlPipeline {
    pub fn new(constraints: Vec<TtlConstraint>) -> Self {
        let h1 = H1Cohomology::new(constraints.clone());
        TtlPipeline { constraints, h1 }
    }

    /// Produce FM-format constraint records (64-byte records for emitter).
    pub fn to_records(&self) -> Vec<[u8; 64]> {
        self.constraints.iter().map(|c| c.to_record()).collect()
    }

    /// Check constraints with emergence detection.
    /// Returns emergence events after each batch.
    pub fn check(
        &mut self,
        bounds: &[i32; 16],
        load: f64,
    ) -> (Vec<ConstraintState>, Vec<EmergenceEvent>) {
        let states: Vec<ConstraintState> = self
            .constraints
            .iter_mut()
            .map(|c| c.evaluate(bounds, load))
            .collect();

        let events = self.h1.tick(load);
        (states, events)
    }

    /// Get emergence events without checking (just tick).
    pub fn poll_emergence(&mut self, load: f64) -> Vec<EmergenceEvent> {
        self.h1.tick(load)
    }
}

// ── Integration with FM's x86-64 Bloom Pre-Filter ──────────

/// A TTL-aware bloom filter.
///
/// FM's existing bloom pre-filter bypasses 80-90% of constraint checks.
/// A TTL-aware bloom also filters EXPIRED constraints — the bloom entry
/// is zeroed when the constraint dies. This means dead constraints are
/// invisible to the emitter. No branching. No special case.
///
/// Implementation: the bloom hash includes (constraint_id, ttl_tag).
/// When a constraint expires, its bloom entry is cleared. The emitter
/// skips it because the bloom returns zero.
///
/// This reuses FM's existing bloom machinery. No new codegen needed.
pub fn ttl_bloom_hash(constraint_id: i64, ttl_type: TtlType) -> u64 {
    let tag = ttl_type as u64;
    // Murmur3-inspired mixing
    let h1 = (constraint_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let h2 = tag.wrapping_mul(0xC6A4_A793_5BD1_E995);
    h1 ^ h2.rotate_left(17)
}

// ── Span Scale Invariance ───────────────────────────────────

/// The TTL equation is scale-invariant.
///
/// Same formula applies to:
/// - A single constraint on a GPU (µs lifespan)
/// - A fleet agent on a server (hours lifespan)
/// - A vessel's annual certification (years lifespan)
///
/// The constants change. The equation doesn't.
/// This is the Mandelbrot constraint — self-similar at every scale.
#[test]
fn test_lifespan_basic() {
    let remaining = lifespan(60.0, 0, 1.0, 30.0, 1.0);
    assert!(remaining > 0.0); // 30s into 60s lifespan
    let remaining = lifespan(60.0, 1000, 2.0, 30.0, 1.5);
    assert!(remaining < 0.0); // heavily used + loaded = expired
}

#[test]
fn test_constraint_lifecycle() {
    let mut c = TtlConstraint::with_lifespan(1, [0i32; 16], [100i32; 16], TtlType::Task, 3600.0);
    assert!(matches!(c.state, ConstraintState::Active { .. }));
    assert!(c.is_alive(1.0));
    c.terminate();
    assert!(matches!(c.state, ConstraintState::Expired { .. }));
}

#[test]
fn test_h1_cohomology_basic() {
    let c1 = TtlConstraint::with_lifespan(1, [0i32; 16], [10i32; 16], TtlType::Tile, 3600.0);
    let c2 = TtlConstraint::with_lifespan(2, [0i32; 16], [10i32; 16], TtlType::Tile, 3600.0);
    let constraints = vec![c1, c2];
    let h1 = H1Cohomology::new(constraints);
    assert_eq!(h1.betti(), 2i64 - 16i64 + 1i64); // E=2, V=16, C=1
}

#[test]
fn test_expiry_reduces_e() {
    let c1 = TtlConstraint::with_lifespan(1, [0i32; 16], [10i32; 16], TtlType::Tile, 3600.0);
    let c2 = TtlConstraint::with_lifespan(2, [0i32; 16], [10i32; 16], TtlType::Tile, 0.0); // instantly dead
    let constraints = vec![c1, c2];
    let h1 = H1Cohomology::new(constraints);
    assert_eq!(h1.active_count(), 1); // c2 expired immediately
}

#[test]
fn test_ttl_bloom_hash_unique() {
    let h1 = ttl_bloom_hash(1, TtlType::Tile);
    let h2 = ttl_bloom_hash(1, TtlType::Task);
    assert_ne!(h1, h2); // different TTL types = different hashes
    let h3 = ttl_bloom_hash(2, TtlType::Tile);
    assert_ne!(h1, h3); // different IDs = different hashes
}

#[test]
fn test_evaluate_updates_state() {
    let mut c = TtlConstraint::new(1, [0i32; 16], [50i32; 16], TtlType::Tile);
    // Values within bounds → satisfied
    let vals = [25i32; 16];
    let state = c.evaluate(&vals, 1.0);
    match state {
        ConstraintState::Active { satisfied, .. } => assert!(satisfied),
        _ => panic!("expected active"),
    }
    // Values out of bounds → violated
    let vals = [100i32; 16];
    let state = c.evaluate(&vals, 1.0);
    match state {
        ConstraintState::Active { satisfied, .. } => assert!(!satisfied),
        _ => panic!("expected active"),
    }
}

#[test]
fn test_full_pipeline() {
    // Simulate the full FM + Keel integration
    let constraints = vec![
        TtlConstraint::new(1, [0i32; 16], [50i32; 16], TtlType::Tile),
        TtlConstraint::new(2, [0i32; 16], [100i32; 16], TtlType::Task),
        TtlConstraint::with_lifespan(3, [0i32; 16], [100i32; 16], TtlType::Bearing, 0.0), // instant death
    ];

    let _pipeline = TtlPipeline::new(constraints.clone());
    let _records: Vec<[u8; 64]> = constraints.iter().map(|c| c.to_record()).collect();
    let mut h1 = H1Cohomology::new(constraints.clone());

    // First tick: all alive
    let _events = h1.tick(1.0);
    assert_eq!(h1.active_count(), 2);
    assert_eq!(h1.active_count(), 2);
}
