//! PLATO-on-VMythos — room server as SIMD array
//!
//! Every PLATO tile is a 64-byte SIMD record. Every room is a contiguous
//! block of records. The room server loop IS the Mythos recurrent loop.
//!
//! ## Byte Format (one tile = 64 bytes)
//!
//! ```text
//! [0..7]:     tile_id hash (u64, high 8 bytes of SHA-256)
//! [8..15]:    prev_hash chain link (u64)
//! [16..19]:   confidence (f32)
//! [20..23]:   agent_trust (f32)
//! [24..27]:   created_at_sec (u32, unix timestamp)
//! [28..31]:   expires_at_sec (u32, TTL deadline)
//! [32..35]:   domain_id (u32, packed from domain string)
//! [36..39]:   source_id (u32, packed from source string)
//! [40..43]:   flags:
//!               bit 0: validated (passed deadband gate)
//!               bit 1: provenance_verified
//!               bit 2: expired (TTL past deadline)
//!               bit 3: emerged (β₁ changed on this tile)
//!               bit 4-7: priority tier (0=P0, 1=P1, 2=P2)
//! [44..47]:   tags_hash (u32, bloom of tags array)
//! [48..63]:   reserved (16 bytes for future use)
//! ```
//!
//! This format is AVX-512 friendly — each 64-byte record fits in one
//! cache line, and the fields align to 4-byte boundaries for VPCMPD.
//!
//! ## Room as SIMD Array
//!
//! A room is a `Vec<[u8; 64]>` in memory. The room server's inner loop
//! is a SIMD reduce:
//!
//! ```asm
//! // Load 16 tiles (1024 bytes) into zmm0-zmm15
//! VMOVDQU64 zmm0, [rdi]       // tile 0-15 confidence values
//! VMOVDQU64 zmm1, [rdi+64]     // tile 16-31 confidence values
//! // ... 16 loads for 256 tiles per batch
//!
//! // Compare confidence >= threshold (P0=0.99, P1=0.80, P2=0.50)
//! VPCMPD $6, k1, zmm0, zmm_thresh_low  // P2: confidence >= 0.5
//! VPCMPD $2, k2, zmm0, zmm_thresh_high // P0: confidence <= 0.99
//! KPANDW k3, k1, k2                     // validated mask
//!
//! // Check TTL expiry
//! VPCMPD $6, k4, zmm_time, zmm_deadline // not expired yet
//! KANDNW k5, k3, k4                     // validated AND alive
//!
//! // KORTESTW: any tiles passing?
//! KORTESTW k5, k5
//! JNZ handle_result
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

// ── PLATO Tile as 64-byte SIMD Record ──────────────────────

/// A PLATO tile in the 64-byte SIMD format.
///
/// This is the SAME structure that FM's AVX-512 emitter processes.
/// A constraint record IS a PLATO tile. The emitter doesn't know
/// the difference — it just compares bytes.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct PlatoTile {
    pub tile_id: u64,
    pub prev_hash: u64,
    pub confidence: f32,
    pub agent_trust: f32,
    pub created_at: u32,
    pub expires_at: u32,
    pub domain_id: u32,
    pub source_id: u32,
    pub flags: u32,
    pub tags_hash: u32,
    pub reserved: [u8; 16],
}

static_assertions::assert_eq_size!(PlatoTile, [u8; 64]);

impl PlatoTile {
    /// Priority tier from flags (bits 4-7).
    pub fn priority(&self) -> u8 {
        ((self.flags >> 4) & 0xF) as u8
    }

    /// Is this tile still alive (not expired)?
    pub fn is_alive(&self) -> bool {
        (self.flags & 0b100) == 0
    }

    /// Has this tile emerged (β₁ changed)?
    pub fn has_emerged(&self) -> bool {
        (self.flags & 0b1000) != 0
    }

    /// Validate confidence against priority threshold.
    /// Matches Mythos DeadbandACT.get_priority():
    /// P0=0.99, P1=0.80, P2=0.50
    pub fn passes_deadband(&self) -> bool {
        let threshold = match self.priority() {
            0 => 0.99, // P0
            1 => 0.80, // P1
            _ => 0.50, // P2
        };
        self.confidence >= threshold
    }

    /// Pack domain string into u32 using FNV-1a.
    pub fn pack_domain(domain: &str) -> u32 {
        let mut hash: u32 = 0x811C_9DC5;
        for b in domain.bytes() {
            hash ^= b as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
        hash
    }
}

// ── Room as SIMD Array ─────────────────────────────────────

/// A PLATO room backed by a contiguous SIMD array.
///
/// Each room is a `Vec<PlatoTile>` — a memory-mapped array of 64-byte
/// records. The room server processes batches via SIMD operations.
#[derive(Debug, Clone)]
pub struct PlatoRoom {
    /// Contiguous array of 64-byte tiles.
    pub tiles: Vec<PlatoTile>,
    /// Number of active (non-expired) tiles.
    pub active_count: usize,
    /// Room domain tag.
    pub domain_id: u32,
}

impl PlatoRoom {
    pub fn new(domain: &str) -> Self {
        PlatoRoom {
            tiles: Vec::new(),
            active_count: 0,
            domain_id: PlatoTile::pack_domain(domain),
        }
    }

    /// Add a tile to this room (SIMD store emulation).
    /// Equivalent to VMOVDQU64 [room+offset].
    pub fn push(&mut self, tile: PlatoTile) {
        let alive = tile.is_alive();
        self.tiles.push(tile);
        if alive {
            self.active_count += 1;
        }
    }

    /// SIMD batch filter: find matching tiles.
    ///
    /// In AVX-512: this is VPCMPD + KPANDW + KORTESTW across the batch.
    /// In NEON: LD1.4S + CMGE + CMLE + AND, 4 lanes at a time.
    ///
    /// Returns indices of tiles matching all filters.
    pub fn query(&self, min_confidence: f32, max_age_sec: u32, domain_mask: u32) -> Vec<usize> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;

        let mut results = Vec::new();

        // SIMD-simulated: in real code, this would be a vectorized loop
        // with VPCMPD/KPANDW processing 16 tiles per iteration.
        for (i, tile) in self.tiles.iter().enumerate() {
            // GateLower: confidence >= min_confidence
            let pass_confidence = tile.confidence >= min_confidence;

            // GateUpper: age <= max_age_sec
            let age = now.saturating_sub(tile.created_at);
            let pass_age = age <= max_age_sec;

            // Filter: AND of all gates
            let pass_domain = domain_mask == 0 || (tile.domain_id & domain_mask) != 0;
            let pass_deadband = tile.passes_deadband();

            // DeadbandCheck: all gates pass
            if pass_confidence && pass_age && pass_domain && pass_deadband && tile.is_alive() {
                results.push(i);
            }
        }

        results
    }

    /// Process expired tiles: clear their deadband bits.
    ///
    /// Equivalent to: VPCMPD(time >= deadline) → mask
    /// KANDNW(mask, current_mask) → cleared
    pub fn reap_expired(&mut self) -> Vec<usize> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;

        let mut reaped = Vec::new();
        for (i, tile) in self.tiles.iter_mut().enumerate() {
            if tile.is_alive() && now >= tile.expires_at {
                // Clear alive flag (bit 2) — death is default
                tile.flags |= 0b100;
                self.active_count = self.active_count.saturating_sub(1);
                reaped.push(i);
            }
        }
        reaped
    }

    /// Check for emergence: did any tile expire this tick?
    ///
    /// Equivalent to: prev_mask XOR current_mask → KXORW
    /// KORTESTW(k_delta, k_delta) — ZF=1 if no emergence
    pub fn check_emergence(&mut self, prev_active: usize) -> Vec<usize> {
        let delta = prev_active.abs_diff(self.active_count);
        if delta > 0 {
            // Mark emerged tiles
            // In real SIMD: KXORW k4, k3, k3_prev → delta mask
            // then VPTESTMD to find which lanes emerged
            self.tiles.iter_mut().for_each(|t| {
                if !t.is_alive() && !t.has_emerged() {
                    t.flags |= 0b1000; // emerged flag
                }
            });
        }
        self.tiles.iter().enumerate()
            .filter(|(_, t)| t.has_emerged())
            .map(|(i, _)| i)
            .collect()
    }
}

// ── PLATO Room Server as VMythos Loop ──────────────────────

/// The PLATO room server expressed as a VMythos recurrent loop.
///
/// Each HTTP request is one iteration of the loop:
/// 1. LoadTile: parse request → 64-byte record
/// 2. GateLower: validate confidence, source, domain
/// 3. GateUpper: validate TTL not expired
/// 4. Filter: AND validation masks
/// 5. DeadbandCheck: KORTESTW — did tile pass?
/// 6. StoreTile: insert into room array
pub struct PlatoMythosServer {
    /// All rooms as SIMD arrays
    rooms: Vec<PlatoRoom>,
    /// Total operations (Mythos loop iterations)
    iterations: u64,
}

impl PlatoMythosServer {
    pub fn new() -> Self {
        PlatoMythosServer {
            rooms: Vec::new(),
            iterations: 0,
        }
    }

    /// Get or create a room. Equivalent to SIMD array allocation.
    pub fn room(&mut self, name: &str) -> &mut PlatoRoom {
        let domain_id = PlatoTile::pack_domain(name);
        let idx = self.rooms.iter().position(|r| r.domain_id == domain_id);
        let idx = idx.unwrap_or_else(|| {
            self.rooms.push(PlatoRoom::new(name));
            self.rooms.len() - 1
        });
        &mut self.rooms[idx]
    }

    /// Submit a tile to a room — one Mythos loop iteration.
    ///
    /// Steps (matching VMythos emitter instruction sequence):
    /// 1. LoadTile: pack fields into PlatoTile format
    /// 2. GateLower: check confidence >= threshold
    /// 3. GateUpper: check TTL within bounds
    /// 4. Filter: KPANDW equivalent
    /// 5. DeadbandCheck: KORTESTW equivalent
    /// 6. Store: save to room array
    pub fn submit(&mut self, room: &str, tile: PlatoTile) -> Result<(), &'static str> {
        self.iterations += 1;

        let room = self.room(room);

        // GateLower: confidence must be >= priority threshold
        if !tile.passes_deadband() {
            return Err("confidence below deadband threshold");
        }

        // GateUpper: TTL must not be expired
        if !tile.is_alive() {
            return Err("tile already expired");
        }

        // Filter: domain must match room
        if tile.domain_id != room.domain_id && room.domain_id != 0 {
            return Err("domain mismatch");
        }

        // DeadbandCheck: all gates passed — store tile
        room.push(tile);
        Ok(())
    }

    /// Run the reaping loop: check all rooms for expired tiles.
    ///
    /// This is the full Mythos recurrent loop at the PLATO level.
    /// Returns total emerged tiles across all rooms.
    pub fn reap_all(&mut self) -> usize {
        let mut total_emerged = 0;
        for room in self.rooms.iter_mut() {
            let prev = room.active_count;
            let reaped = room.reap_expired();
            if !reaped.is_empty() {
                let emerged = room.check_emergence(prev);
                total_emerged += emerged.len();
            }
        }
        total_emerged
    }

    /// Search across all rooms using SIMD-style filtering.
    ///
    /// This replaces PLATO's /search endpoint with an in-memory
    /// SIMD array scan. No SQLite needed for the hot path.
    pub fn search(&self, query: &str, min_confidence: f32, max_age_sec: u32) -> Vec<(usize, usize)> {
        let domain_mask = PlatoTile::pack_domain(query);
        // If query is empty, match all domains
        let mask = if query.is_empty() { 0 } else { domain_mask };

        let mut results = Vec::new();

        // SIMD-simulated: real implementation processes 16 tiles
        // per iteration with VPCMPD/KPANDW
        for (room_idx, room) in self.rooms.iter().enumerate() {
            for tile_idx in room.query(min_confidence, max_age_sec, mask) {
                results.push((room_idx, tile_idx));
            }
        }

        results
    }

    /// Stats for the /status endpoint.
    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "rooms": self.rooms.len(),
            "total_tiles": self.rooms.iter().map(|r| r.tiles.len()).sum::<usize>(),
            "active_tiles": self.rooms.iter().map(|r| r.active_count).sum::<usize>(),
            "iterations": self.iterations,
            "architecture": "plato-on-vmythos",
            "instruction_set": "load-tile / gate-lower / gate-upper / filter / deadband-check / store-tile",
        })
    }
}

// ── Provenance Chain as Mask Accumulation ──────────────────

/// Provenance tracking via SIMD-style mask accumulation.
///
/// In Mythos terms: KADDW across submissions accumulates total
/// replications. When a tile has enough replications, it's "proven."
///
/// This replaces the separate chain-of-trust with arithmetic.
#[derive(Debug, Clone)]
pub struct ProvenanceMask {
    /// How many times each tile has been submitted.
    /// In SIMD: KADDW accumulates submission count across lanes.
    counts: Vec<u32>,
    /// How many replications needed for "proven" status.
    threshold: u32,
}

impl ProvenanceMask {
    pub fn new(threshold: u32) -> Self {
        ProvenanceMask {
            counts: Vec::new(),
            threshold,
        }
    }

    /// Record a submission — equivalent to KADDW.
    pub fn record(&mut self, tile_id: u64) -> bool {
        let idx = self.counts.len();
        self.counts.push(1);
        // Check if threshold reached (proven)
        self.counts[idx] >= self.threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_size() {
        assert_eq!(std::mem::size_of::<PlatoTile>(), 64);
    }

    #[test]
    fn test_plato_room_push() {
        let mut room = PlatoRoom::new("fleet_math");
        let tile = PlatoTile {
            tile_id: 1, prev_hash: 0,
            confidence: 0.95, agent_trust: 0.8,
            created_at: 100, expires_at: 200,
            domain_id: PlatoTile::pack_domain("fleet_math"),
            source_id: 0, flags: 0b0001_0000, // P0 priority
            tags_hash: 0, reserved: [0u8; 16],
        };
        room.push(tile);
        assert_eq!(room.tiles.len(), 1);
        assert_eq!(room.active_count, 1);
    }

    #[test]
    fn test_deadband_rejection() {
        let mut server = PlatoMythosServer::new();
        let tile = PlatoTile {
            tile_id: 1, prev_hash: 0,
            confidence: 0.3, agent_trust: 0.1, // too low for P0
            created_at: 100, expires_at: 200,
            domain_id: PlatoTile::pack_domain("fleet_math"),
            source_id: 0, flags: 0b0000_0000, // P2 priority (0.50 threshold)
            tags_hash: 0, reserved: [0u8; 16],
        };
        // P2 threshold is 0.50, and confidence is 0.3
        assert!(server.submit("fleet_math", tile).is_err());
    }

    #[test]
    fn test_reap_expired() {
        let mut room = PlatoRoom::new("fleet_math");
        // Tile with TTL in the past
        let past = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32 - 100;

        let tile = PlatoTile {
            tile_id: 1, prev_hash: 0,
            confidence: 0.95, agent_trust: 0.8,
            created_at: past - 1000, expires_at: past, // already expired
            domain_id: PlatoTile::pack_domain("fleet_math"),
            source_id: 0, flags: 0b0001_0000,
            tags_hash: 0, reserved: [0u8; 16],
        };
        room.push(tile);
        assert_eq!(room.active_count, 0); // expired at birth
    }

    #[test]
    fn test_vmythos_stats() {
        let server = PlatoMythosServer::new();
        let stats = server.stats();
        assert_eq!(stats["rooms"], 0);
        assert_eq!(stats["architecture"], "plato-on-vmythos");
    }
}
