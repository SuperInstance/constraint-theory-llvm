//! Mythos-Meshed TTL Constraints — tiles as KV, deadband as ACT, rooms as experts
//!
//! Embedding the three core Mythos patterns into the constraint pipeline:
//!
//! 1. **Tiles as KV** — Every constraint evaluation produces a (key, value, confidence)
//!    triplet. Multiple constraints in a chain produce a multi-key tile. The emitter
//!    compiles tiles, not booleans.
//!
//! 2. **Deadband as ACT** — TTL expiry levels map to Mythos priority tiers:
//!    - P0 (0.99): Trust, Tile — critical constraints, halt at 99% confidence
//!    - P1 (0.80): Agent, Bearing — standard constraints
//!    - P2 (0.50): Task — eager to halt, low priority
//!
//!    The return to continue compute — when a constraint expires at a given tier,
//!    the Mythos controller is signalled to route to a different expert room.
//!
//! 3. **Rooms as Experts** — Each constraint domain (math, agent, fleet, etc.) maps
//!    to a PLATO expert room. Confidence-weighted gating: high-confidence constraints
//!    batch through the fast path (AVX-512/NEON), low-confidence get the recurrent
//!    depth loop (CDCL re-check).

use super::ttl_constraint::{TtlConstraint, ConstraintState, TtlType, TtlTraceEvent, EmergenceEvent};

// ── Mythos Priority Tiers (from plato_mythos/deadband_act.py) ──────

/// Priority tiers matching PLATO deadband thresholds.
///
/// Repurposed from control-loop priority to adaptive-compute halting.
/// Higher priority = more compute budget before halting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MythosPriority {
    /// P0 = 0.99 — critical. Trust + Tile TTL types.
    /// Exhaust only when virtually certain. Most compute budget.
    Critical,
    /// P1 = 0.80 — standard. Agent + Bearing types.
    /// Normal compute budget.
    Standard,
    /// P2 = 0.50 — low. Task type.
    /// Eager to halt. Least compute budget.
    Low,
}

impl MythosPriority {
    /// Halting threshold (matches PLATO's DeadbandACT tiers).
    pub fn halt_threshold(self) -> f64 {
        match self {
            MythosPriority::Critical => 0.99,
            MythosPriority::Standard => 0.80,
            MythosPriority::Low => 0.50,
        }
    }

    /// Maximum steps before forced halt (recurrent loop depth budget).
    pub fn max_steps(self) -> usize {
        match self {
            MythosPriority::Critical => 12,
            MythosPriority::Standard => 8,
            MythosPriority::Low => 4,
        }
    }

    /// Map a TTL type to its Mythos priority.
    pub fn from_ttl(ttl: TtlType) -> Self {
        match ttl {
            TtlType::Trust => MythosPriority::Critical,
            TtlType::Tile => MythosPriority::Critical,
            TtlType::Agent => MythosPriority::Standard,
            TtlType::Bearing => MythosPriority::Standard,
            TtlType::Task => MythosPriority::Low,
        }
    }
}

// ── Tiles as KV — Constraint Result as Key-Value Pair ─────────────

/// A constraint evaluation result formatted as a Mythos tile.
///
/// In Mythos, a tile is a structured memory unit with:
/// - `key`: constraint_id (what was checked)
/// - `value`: boolean result + deadband level (did it pass? at what priority?)
/// - `confidence`: how sure we are (based on remaining lifespan / base_lifespan)
/// - `domain`: which expert room this routes to
#[derive(Debug, Clone)]
pub struct ConstraintTile {
    /// Constraint identifier (the key).
    pub key: i64,
    /// Evaluation result packed with priority tier.
    pub value: TileValue,
    /// Confidence in [0, 1]. Derived from remaining lifespan.
    pub confidence: f64,
    /// Domain tag for expert routing.
    pub domain: &'static str,
    /// Whether this tile is expired (dead) — deadband has halted.
    pub is_dead: bool,
}

#[derive(Debug, Clone)]
pub struct TileValue {
    /// Did the constraint pass?
    pub satisfied: bool,
    /// At what Mythos priority tier did we evaluate?
    pub priority: MythosPriority,
    /// How many recurrent steps did evaluation take?
    pub steps: usize,
}

impl ConstraintTile {
    /// Convert a TTL constraint state into a Mythos tile.
    pub fn from_constraint(c: &TtlConstraint) -> Self {
        let priority = MythosPriority::from_ttl(c.ttl_type);
        let (satisfied, is_dead) = match c.state {
            ConstraintState::Active { satisfied, .. } => (satisfied, false),
            ConstraintState::Expired { last_value, .. } => (last_value, true),
            ConstraintState::Emerged { .. } => (false, true),
        };
        let remaining = match c.state {
            ConstraintState::Active { remaining, .. } => remaining.max(0.0),
            _ => 0.0,
        };
        let confidence = if c.base_lifespan > 0.0 {
            (remaining / c.base_lifespan).min(1.0)
        } else {
            0.0
        };

        ConstraintTile {
            key: c.constraint_id,
            value: TileValue { satisfied, priority, steps: c.use_count as usize },
            confidence,
            domain: match c.ttl_type {
                TtlType::Tile => "plato",
                TtlType::Task => "agent",
                TtlType::Agent => "fleet",
                TtlType::Bearing => "communication",
                TtlType::Trust => "security",
            },
            is_dead,
        }
    }

    /// Mythos-style tile compression: pack into a 64-bit KV pair.
    /// Bits: [key:32] [satisfied:1] [priority:2] [dead:1] [confidence:28]
    pub fn pack(&self) -> u64 {
        let k = (self.key as u64) & 0xFFFF_FFFF;
        let s = if self.value.satisfied { 1u64 } else { 0u64 } << 32;
        let p = (self.value.priority as u64) << 33;
        let d = if self.is_dead { 1u64 } else { 0u64 } << 35;
        let c = ((self.confidence * (1u64 << 28) as f64) as u64) << 36;
        k | s | p | d | c
    }
}

// ── Deadband ACT Controller — Adaptive Halt from TTL ────────────

/// Recurrent-loop halting controller using TTL as the deadband signal.
///
/// Mythos pattern: the constraint's TTL expiry IS the halting signal.
/// No separate timing thread. No polling loop. When lifespan(E) < 0,
/// the deadband fires and the loop routes to a different expert.
#[derive(Debug, Clone)]
pub struct DeadbandActController {
    /// Current cumulative halt probability.
    cum_prob: f64,
    /// Current priority tier.
    priority: MythosPriority,
    /// Step counter.
    step: usize,
}

impl DeadbandActController {
    pub fn new() -> Self {
        DeadbandActController {
            cum_prob: 0.0,
            priority: MythosPriority::Standard,
            step: 0,
        }
    }

    /// Reset for a new constraint evaluation cycle.
    pub fn reset(&mut self, priority: MythosPriority) {
        self.cum_prob = 0.0;
        self.priority = priority;
        self.step = 0;
    }

    /// Compute halt probability from a constraint evaluation.
    ///
    /// Maps to DeadbandACT.halt_probability():
    /// - If the constraint expired, halt_prob = 1.0 (converged)
    /// - If lifespan < 50% remaining, halt_prob = 0.5 + delta
    /// - Otherwise, halt_prob = 0.0 (keep thinking)
    pub fn halt_probability(&self, tile: &ConstraintTile) -> f64 {
        if tile.is_dead {
            return 1.0; // deadband triggered — halting signal
        }
        if tile.confidence < 0.5 {
            return 0.8; // low confidence — likely to converge next step
        }
        if tile.confidence < 0.8 {
            return 0.3; // mid confidence — some thinking left
        }
        0.0 // high confidence — no reason to halt
    }

    /// Should the recurrent loop continue?
    ///
    /// Maps to DeadbandACT.should_continue():
    /// Returns true when cumulative prob is below the priority's halt threshold
    /// and step count is within budget.
    pub fn should_continue(&mut self, tile: &ConstraintTile) -> bool {
        if self.step >= self.priority.max_steps() - 1 {
            return false; // budget exhausted
        }
        self.step += 1;
        let prob = self.halt_probability(tile);
        self.cum_prob += prob;
        self.cum_prob < self.priority.halt_threshold()
    }
}

// ── Rooms as Experts — Domain-Routed Constraint Evaluation ──────

/// An expert room that handles a specific constraint domain.
///
/// In Mythos, each PLATO room is an expert group. Here, each domain
/// (math, agent, fleet, communication, security) is processed by its
/// own expert evaluator with domain-specific deadband thresholds.
#[derive(Debug, Clone)]
pub struct RoomExpert {
    /// Domain name (matches PLATO room fleet_{domain}).
    pub domain: &'static str,
    /// Priority tier for this room's constraints.
    pub default_priority: MythosPriority,
    /// Number of active constraints in this room.
    pub active_count: usize,
    /// Total constraints assigned to this room.
    pub total_count: usize,
}

impl RoomExpert {
    pub fn new(domain: &'static str, priority: MythosPriority) -> Self {
        RoomExpert { domain, default_priority: priority, active_count: 0, total_count: 0 }
    }

    /// Route a constraint tile to this room.
    /// The confidence determines the gating weight (0-1).
    pub fn route(&self, tile: &ConstraintTile) -> f64 {
        if self.domain == tile.domain {
            tile.confidence // confidence-weighted gating
        } else {
            0.0
        }
    }
}

/// A Mythos-style room router that distributes constraints to expert evaluators.
#[derive(Debug, Clone)]
pub struct MythosRouter {
    pub rooms: Vec<RoomExpert>,
}

impl MythosRouter {
    pub fn new() -> Self {
        MythosRouter {
            rooms: vec![
                RoomExpert::new("math", MythosPriority::Critical),
                RoomExpert::new("plato", MythosPriority::Critical),
                RoomExpert::new("fleet", MythosPriority::Standard),
                RoomExpert::new("agent", MythosPriority::Standard),
                RoomExpert::new("communication", MythosPriority::Standard),
                RoomExpert::new("security", MythosPriority::Critical),
            ],
        }
    }

    /// Assign constraints to rooms based on their domain tags.
    /// Returns (room_index, confidence_weight) pairs.
    pub fn assign(&mut self, constraints: &[TtlConstraint]) -> Vec<(usize, f64)> {
        let mut assignments = Vec::new();
        for c in constraints {
            let tile = ConstraintTile::from_constraint(c);
            for (i, room) in self.rooms.iter_mut().enumerate() {
                let weight = room.route(&tile);
                if weight > 0.0 {
                    room.total_count += 1;
                    if matches!(c.state, ConstraintState::Active { .. }) {
                        room.active_count += 1;
                    }
                    assignments.push((i, weight));
                    break;
                }
            }
        }
        assignments
    }
}

// ── Bard/Warden/Healer Archetypes ──────────────────────────────

/// The three Mythos archetypes as processing policies.
///
/// - Bard: generates output from constraint tiles (KV → human-readable)
/// - Warden: filters by quality gate (confidence threshold)
/// - Healer: repairs inconsistent tiles (expired → re-evaluate)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archetype {
    Bard,
    Warden,
    Healer,
}

impl Archetype {
    /// Process constraint tiles according to this archetype's policy.
    pub fn process_tiles(self, tiles: &[ConstraintTile]) -> Vec<ConstraintTile> {
        match self {
            Archetype::Bard => {
                // Bard: select tiles with highest confidence, one per domain
                let mut best: Vec<ConstraintTile> = Vec::new();
                for tile in tiles {
                    if tile.is_dead { continue; }
                    if let Some(existing) = best.iter_mut().find(|t: &&mut ConstraintTile| t.domain == tile.domain) {
                        if tile.confidence > existing.confidence {
                            *existing = tile.clone();
                        }
                    } else {
                        best.push(tile.clone());
                    }
                }
                best
            }
            Archetype::Warden => {
                // Warden: filter by confidence >= priority threshold
                tiles.iter().filter(|t| {
                    if t.is_dead { return false; }
                    let threshold = t.value.priority.halt_threshold();
                    t.confidence >= threshold
                }).cloned().collect()
            }
            Archetype::Healer => {
                // Healer: flag expired tiles for re-evaluation
                tiles.iter().map(|t| {
                    let mut healed = t.clone();
                    if healed.is_dead && healed.confidence < 0.3 {
                        // Mark for soft re-evaluation (restore with degraded confidence)
                        healed.is_dead = false;
                        healed.confidence = 0.3;
                    }
                    healed
                }).collect()
            }
        }
    }
}

// ── Full Mythos Pipeline ──────────────────────────────────────

/// A constraint evaluation pipeline meshed with Mythos patterns.
///
/// Three layers in one:
/// 1. Constraints → Tiles (KV pair output)
/// 2. Tiles → Rooms (expert routing via domain tags)
/// 3. Tiles → Archetypes (bard/warden/healer processing policy)
pub struct MythosPipeline {
    pub router: MythosRouter,
    pub deadband: DeadbandActController,
    pub archetype: Archetype,
}

impl MythosPipeline {
    pub fn new(archetype: Archetype) -> Self {
        MythosPipeline {
            router: MythosRouter::new(),
            deadband: DeadbandActController::new(),
            archetype,
        }
    }

    /// Process constraints through the full Mythos pipeline.
    pub fn evaluate(&mut self, constraints: &[TtlConstraint]) -> Vec<EmergedTile> {
        // 1. Convert constraints to tiles (KV)
        let tiles: Vec<ConstraintTile> = constraints.iter()
            .map(|c| ConstraintTile::from_constraint(c))
            .collect();

        // 2. Assign tiles to expert rooms
        let _assignments = self.router.assign(constraints);

        // 3. Apply archetype processing
        let processed = self.archetype.process_tiles(&tiles);

        // 4. For each tile, check deadband
        let mut emerged = Vec::new();
        for tile in &processed {
            self.deadband.reset(tile.value.priority);
            if !self.deadband.should_continue(tile) {
                emerged.push(EmergedTile {
                    tile: tile.clone(),
                    reason: "deadband halted — constraint converged to expert room",
                    total_steps: self.deadband.step,
                });
            }
        }

        emerged
    }
}

/// An event emitted when a constraint tile crosses the deadband threshold
/// and emerges into consciousness (the result is ready to be shared).
#[derive(Debug, Clone)]
pub struct EmergedTile {
    pub tile: ConstraintTile,
    pub reason: &'static str,
    pub total_steps: usize,
}

// ── Tests ─────────────────────────────────────────────────────

#[test]
fn test_mythos_priority_maps_ttl() {
    assert_eq!(MythosPriority::from_ttl(TtlType::Trust), MythosPriority::Critical);
    assert_eq!(MythosPriority::from_ttl(TtlType::Task), MythosPriority::Low);
}

#[test]
fn test_deadband_halt_on_expiry() {
    let mut act = DeadbandActController::new();
    act.reset(MythosPriority::Standard);

    // Expired tile should trigger halt
    let expired_tile = ConstraintTile {
        key: 1, value: TileValue { satisfied: false, priority: MythosPriority::Standard, steps: 5 },
        confidence: 0.0, domain: "math", is_dead: true,
    };
    assert_eq!(act.halt_probability(&expired_tile), 1.0);
}

#[test]
fn test_tile_packing() {
    let tile = ConstraintTile {
        key: 42, value: TileValue { satisfied: true, priority: MythosPriority::Standard, steps: 3 },
        confidence: 0.75, domain: "fleet", is_dead: false,
    };
    let packed = tile.pack();
    assert_ne!(packed, 0);
    // Verify key survived packing
    assert_eq!(packed & 0xFFFF_FFFF, 42);
}

#[test]
fn test_archetype_bard_selects_best() {
    let tiles = vec![
        ConstraintTile { key: 1, value: TileValue { satisfied: true, priority: MythosPriority::Standard, steps: 2 }, confidence: 0.9, domain: "math", is_dead: false },
        ConstraintTile { key: 2, value: TileValue { satisfied: false, priority: MythosPriority::Standard, steps: 4 }, confidence: 0.3, domain: "math", is_dead: false },
    ];
    let bard = Archetype::Bard;
    let result = bard.process_tiles(&tiles);
    assert_eq!(result.len(), 1); // only 1 domain
    assert_eq!(result[0].key, 1); // highest confidence
}

#[test]
fn test_warden_filters_low_confidence() {
    let tiles = vec![
        ConstraintTile { key: 1, value: TileValue { satisfied: true, priority: MythosPriority::Critical, steps: 2 }, confidence: 0.99, domain: "math", is_dead: false },
        ConstraintTile { key: 2, value: TileValue { satisfied: true, priority: MythosPriority::Low, steps: 1 }, confidence: 0.3, domain: "task", is_dead: false },
    ];
    let warden = Archetype::Warden;
    let result = warden.process_tiles(&tiles);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].key, 1); // only the critical one passed
}
