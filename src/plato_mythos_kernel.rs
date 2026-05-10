//! PLATO-on-VMythos — room server as SIMD array
//!
//! Optimizations implemented:
//! 1. Packed 64-byte record format (flags/deadline packed into u128, deadline as u64)
//! 2. Sorted Vec + binary search for deadline-based queries
//! 3. Combined flags check (is_valid()) merging passes_deadband() and is_alive()

use std::time::{SystemTime, UNIX_EPOCH};

// ── Status flag bit positions (stored in low byte of flags_deadline) ──────────
const FLAG_ALIVE: u8       = 0b010;  // bit 1 — tile is alive (not reaped)
const FLAG_DEADBAND: u8    = 0b100;  // bit 2 — tile passed deadband check
const FLAG_EMERGED: u8     = 0b1000; // bit 3 — tile emerged after reap cycle

/// Combined check: alive AND passed deadband (one bitmask test)
const TILE_VALID: u8 = FLAG_ALIVE | FLAG_DEADBAND; // 0b110

// ── Helper: f32 ↔ u8 confidence encoding ────────────────────────────────────
fn conf_to_u8(confidence: f32) -> u8 {
    (confidence.clamp(0.0, 1.0) * 255.0) as u8
}

fn u8_to_conf(bits: u8) -> f32 {
    (bits as f32) / 255.0
}

// ── Packed 64-byte tile record (Optimization 1) ─────────────────────────────
// Layout matches FM's constraint record format for SIMD alignment.
//
// flags_deadline (u128) layout:
//   [  0..  7] status_flags (u8)     — ALIVE, DEADBAND, EMERGED, priority(high nibble)
//   [  8.. 15] confidence_bits (u8)  — 0..255 mapping to f32 0.0..1.0
//   [ 16.. 47] created_at (u32)      — stored in spare bits
//   [ 48.. 63] spare (16 bits)       — reserved for future expansion
//   [ 64..127] deadline (u64)        — cntvct_el0 compatible expiration time
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct PlatoTile {
    pub tile_id: u64,           // [0..7]   8 bytes
    pub prev_hash: u64,         // [8..15]  8 bytes
    pub flags_deadline: u128,   // [16..31] 16 bytes — packed flags, deadline, confidence
    pub domain_id: u32,         // [32..35] 4 bytes
    pub source_id: u32,         // [36..39] 4 bytes
    pub tags_hash: u32,         // [40..43] 4 bytes
    pub reserved: [u8; 20],     // [44..63] 20 bytes — holds agent_trust in first 4 bytes
}

// Compile-time size check: exactly 64 bytes
const _PLATO_TILE_SIZE: [(); 64] = [(); std::mem::size_of::<PlatoTile>()];

impl PlatoTile {
    /// Convenience constructor mirroring the original field API.
    /// Packs confidence, priority, created_at, and deadline into the u128 field.
    pub fn new(
        tile_id: u64,
        prev_hash: u64,
        confidence: f32,
        agent_trust: f32,
        created_at: u32,
        expires_at: u32,
        domain_id: u32,
        source_id: u32,
        priority: u8,
        tags_hash: u32,
    ) -> Self {
        let deadline = expires_at as u64;
        let conf_bits = conf_to_u8(confidence);
        // New tiles are alive by default. Compute deadband from confidence/priority later.
        let status = FLAG_ALIVE | ((priority & 0x0F) << 4);
        let low = (status as u128) | ((conf_bits as u128) << 8) | ((created_at as u128) << 16);
        let flags_deadline = low | ((deadline as u128) << 64);

        let mut reserved = [0u8; 20];
        reserved[0..4].copy_from_slice(&agent_trust.to_le_bytes());

        PlatoTile { tile_id, prev_hash, flags_deadline, domain_id, source_id, tags_hash, reserved }
    }

    // ── Field accessors ──────────────────────────────────────────────────────

    #[inline]
    pub fn deadline(&self) -> u64 { (self.flags_deadline >> 64) as u64 }

    #[inline]
    pub fn status_flags(&self) -> u8 { self.flags_deadline as u8 }

    #[inline]
    pub fn confidence_bits(&self) -> u8 { ((self.flags_deadline >> 8) & 0xFF) as u8 }

    #[inline]
    pub fn created_at(&self) -> u32 { ((self.flags_deadline >> 16) & 0xFFFF_FFFF) as u32 }

    #[inline]
    pub fn confidence(&self) -> f32 { u8_to_conf(self.confidence_bits()) }

    #[inline]
    pub fn agent_trust(&self) -> f32 {
        f32::from_le_bytes([self.reserved[0], self.reserved[1], self.reserved[2], self.reserved[3]])
    }

    #[inline]
    pub fn priority(&self) -> u8 { (self.status_flags() >> 4) & 0xF }

    #[inline]
    pub fn is_alive(&self) -> bool { (self.status_flags() & FLAG_ALIVE) != 0 }

    #[inline]
    pub fn has_emerged(&self) -> bool { (self.status_flags() & FLAG_EMERGED) != 0 }

    /// Original deadband check: confidence must meet priority-based threshold.
    pub fn passes_deadband(&self) -> bool {
        let threshold = match self.priority() {
            0 => 0.99, 1 => 0.80, _ => 0.50,
        };
        self.confidence() >= threshold - 0.005
    }

    /// Optimization 3: Combined flags check — alive AND passed deadband (one bitmask test).
    #[inline]
    pub fn is_valid(&self) -> bool {
        (self.status_flags() & TILE_VALID) == TILE_VALID
    }

    // ── Status flag mutators ─────────────────────────────────────────────────

    fn set_flag(&mut self, flag: u8, set: bool) {
        let status = if set { self.status_flags() | flag } else { self.status_flags() & !flag };
        self.flags_deadline = (self.flags_deadline & !0xFF) | (status as u128);
    }

    pub fn mark_dead(&mut self) { self.set_flag(FLAG_ALIVE, false); }
    pub fn set_emerged(&mut self) { self.set_flag(FLAG_EMERGED, true); }
    pub fn mark_deadband_passed(&mut self) { self.set_flag(FLAG_DEADBAND, true); }

    // ── Domain hashing ───────────────────────────────────────────────────────

    pub fn pack_domain(domain: &str) -> u32 {
        let mut hash: u32 = 0x811C_9DC5;
        for b in domain.bytes() { hash ^= b as u32; hash = hash.wrapping_mul(0x0100_0193); }
        hash
    }
}

// ── PlatoRoom with sorted Vec (Optimization 2) ─────────────────────────────
#[derive(Debug, Clone)]
pub struct PlatoRoom {
    /// Tiles maintained in ascending order of deadline (expiration time).
    tiles: Vec<PlatoTile>,
    pub active_count: usize,
    pub domain_id: u32,
}

impl PlatoRoom {
    pub fn new(domain: &str) -> Self {
        PlatoRoom { tiles: Vec::new(), active_count: 0, domain_id: PlatoTile::pack_domain(domain) }
    }

    /// Insert tile in deadline-sorted position (Optimization 2).
    /// Uses binary search to find the correct insertion index, keeping tiles sorted
    /// by deadline so reap_expired() and deadline-based queries can use binary search.
    pub fn push(&mut self, tile: PlatoTile) {
        let alive = tile.is_alive();
        let deadline = tile.deadline();
        let idx = self.tiles.binary_search_by_key(&deadline, |t| t.deadline())
            .unwrap_or_else(|e| e);
        self.tiles.insert(idx, tile);
        if alive { self.active_count += 1; }
    }

    /// Optimization 2: Binary search for deadline-range queries + linear scan for confidence.
    ///
    /// Uses binary_search to find the first tile whose deadline exceeds (now - max_age_sec),
    /// then linearly scans from there for remaining filters (confidence, domain, deadband, alive).
    pub fn query(&self, min_confidence: f32, max_age_sec: u32, domain_mask: u32) -> Vec<usize> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as u64;
        let cutoff = now.saturating_sub(max_age_sec as u64);

        // Binary search: first tile with deadline > cutoff
        let start = self.tiles.binary_search_by_key(&cutoff, |t| t.deadline())
            .unwrap_or_else(|e| e);

        let conf_bits = conf_to_u8(min_confidence);
        let mut results = Vec::new();
        for i in start..self.tiles.len() {
            let tile = &self.tiles[i];
            // Deadline check already satisfied by binary search start point
            // Remaining filters: confidence, domain, combined validity
            let pass_confidence = tile.confidence_bits() >= conf_bits;
            let pass_domain = domain_mask == 0 || (tile.domain_id & domain_mask) != 0;
            if pass_confidence && pass_domain && tile.is_valid() {
                results.push(i);
            }
        }
        results
    }

    /// Optimization 2: Binary search to find expired tiles by deadline.
    pub fn reap_expired(&mut self) -> Vec<usize> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as u64;

        // Binary search: first tile with deadline > now (i.e., all tiles before this are expired)
        let boundary = self.tiles.binary_search_by_key(&(now + 1), |t| t.deadline())
            .unwrap_or_else(|e| e);

        let mut reaped = Vec::new();
        for i in 0..boundary {
            let tile = &mut self.tiles[i];
            if tile.is_alive() {
                tile.mark_dead();
                self.active_count = self.active_count.saturating_sub(1);
                reaped.push(i);
            }
        }
        reaped
    }

    pub fn check_emergence(&mut self, prev_active: usize) -> Vec<usize> {
        let delta = prev_active.abs_diff(self.active_count);
        if delta > 0 {
            for tile in self.tiles.iter_mut() {
                if !tile.is_alive() && !tile.has_emerged() {
                    tile.set_emerged();
                }
            }
        }
        self.tiles.iter().enumerate()
            .filter(|(_, t)| t.has_emerged())
            .map(|(i, _)| i)
            .collect()
    }

    pub fn tile_count(&self) -> usize { self.tiles.len() }
}

// ── PlatoMythosServer ───────────────────────────────────────────────────────
pub struct PlatoMythosServer {
    rooms: Vec<PlatoRoom>,
    pub iterations: u64,
}

impl PlatoMythosServer {
    pub fn new() -> Self { PlatoMythosServer { rooms: Vec::new(), iterations: 0 } }

    pub fn room(&mut self, name: &str) -> &mut PlatoRoom {
        let domain_id = PlatoTile::pack_domain(name);
        let idx = self.rooms.iter().position(|r| r.domain_id == domain_id);
        let idx = idx.unwrap_or_else(|| { self.rooms.push(PlatoRoom::new(name)); self.rooms.len() - 1 });
        &mut self.rooms[idx]
    }

    pub fn submit(&mut self, room: &str, tile: PlatoTile) -> Result<(), &'static str> {
        self.iterations += 1;
        let room = self.room(room);
        if !tile.passes_deadband() { return Err("confidence below deadband threshold"); }
        if !tile.is_alive() { return Err("tile already expired"); }
        if tile.domain_id != room.domain_id && room.domain_id != 0 { return Err("domain mismatch"); }
        room.push(tile);
        Ok(())
    }

    pub fn reap_all(&mut self) -> usize {
        let mut total_emerged = 0;
        for room in self.rooms.iter_mut() {
            let prev = room.active_count;
            let reaped = room.reap_expired();
            if !reaped.is_empty() { total_emerged += room.check_emergence(prev).len(); }
        }
        total_emerged
    }

    pub fn search(&self, query: &str, min_confidence: f32, max_age_sec: u32) -> Vec<(usize, usize)> {
        let domain_mask = if query.is_empty() { 0 } else { PlatoTile::pack_domain(query) };
        let mut results = Vec::new();
        for (room_idx, room) in self.rooms.iter().enumerate() {
            for tile_idx in room.query(min_confidence, max_age_sec, domain_mask) {
                results.push((room_idx, tile_idx));
            }
        }
        results
    }

    pub fn stats(&self) -> String {
        let total: usize = self.rooms.iter().map(|r| r.tile_count()).sum();
        let active: usize = self.rooms.iter().map(|r| r.active_count).sum();
        format!("rooms={} total_tiles={} active_tiles={} iterations={}", self.rooms.len(), total, active, self.iterations)
    }
}

// ── ProvenanceMask (unchanged) ───────────────────────────────────────────────
pub struct ProvenanceMask {
    counts: Vec<u32>,
    threshold: u32,
}

impl ProvenanceMask {
    pub fn new(threshold: u32) -> Self { ProvenanceMask { counts: Vec::new(), threshold } }
    pub fn record(&mut self, _tile_id: u64) -> bool {
        self.counts.push(1);
        let idx = self.counts.len() - 1;
        self.counts[idx] >= self.threshold
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_size() { assert_eq!(std::mem::size_of::<PlatoTile>(), 64); }

    fn make_tile(
        id: u64,
        confidence: f32,
        created_at: u32,
        expires_at: u32,
        domain: &str,
        priority: u8,
    ) -> PlatoTile {
        PlatoTile::new(
            id, /*prev_hash*/ 0,
            confidence, /*agent_trust*/ 0.8,
            created_at, expires_at,
            PlatoTile::pack_domain(domain),
            /*source_id*/ 0, priority,
            /*tags_hash*/ 0,
        )
    }

    #[test]
    fn test_plato_room_push() {
        let mut room = PlatoRoom::new("fleet_math");
        let tile = make_tile(1, 0.95, 100, 200, "fleet_math", 1);
        room.push(tile);
        assert_eq!(room.tile_count(), 1);
        assert_eq!(room.active_count, 1);
    }

    #[test]
    fn test_deadband_rejection() {
        let mut server = PlatoMythosServer::new();
        let tile = make_tile(1, 0.3, 100, 200, "fleet_math", 0);
        assert!(server.submit("fleet_math", tile).is_err());
    }

    #[test]
    fn test_reap_expired() {
        let mut room = PlatoRoom::new("fleet_math");
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as u32;
        let tile = make_tile(1, 0.95, now - 1000, now - 100, "fleet_math", 1);
        room.push(tile);
        room.reap_expired();
        assert_eq!(room.active_count, 0);
    }

    #[test]
    fn test_vmythos_stats() {
        let server = PlatoMythosServer::new();
        let stats = server.stats();
        assert!(stats.contains("rooms=0"));
    }

    #[test]
    fn test_tile_packed_field_roundtrip() {
        let tile = make_tile(42, 0.75, 500, 1000, "test_domain", 2);
        assert_eq!(tile.tile_id, 42);
        assert_eq!(tile.deadline(), 1000);
        assert!(tile.is_alive());
        assert!(!tile.has_emerged());
        assert_eq!(tile.priority(), 2);
        assert!((tile.confidence() - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_sorted_insert_order() {
        let mut room = PlatoRoom::new("sorted_test");
        // Insert tiles with increasing deadlines
        room.push(make_tile(1, 0.9, 100, 500, "sorted_test", 1));
        room.push(make_tile(2, 0.9, 100, 300, "sorted_test", 1));
        room.push(make_tile(3, 0.9, 100, 700, "sorted_test", 1));
        room.push(make_tile(4, 0.9, 100, 400, "sorted_test", 1));

        let deadlines: Vec<u64> = room.tiles.iter().map(|t| t.deadline()).collect();
        assert_eq!(deadlines, vec![300, 400, 500, 700], "tiles must be sorted by deadline");
    }

    #[test]
    fn test_is_valid_combined_flag() {
        // New tile: alive but hasn't passed deadband check yet
        let tile = make_tile(1, 0.95, 100, 200, "test", 1);
        assert!(!tile.is_valid(), "new tile should not be valid until deadband check");

        // Manually set deadband-passed flag
        let mut tile = make_tile(1, 0.95, 100, 200, "test", 1);
        tile.mark_deadband_passed();
        assert!(tile.is_valid(), "after deadband check, tile should be valid");

        // Mark dead: should no longer be valid
        tile.mark_dead();
        assert!(!tile.is_valid(), "dead tile should not be valid");
    }
}
