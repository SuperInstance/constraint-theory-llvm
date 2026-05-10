//! PLATO-on-VMythos — room server as SIMD array
//!
//! This is a minimal version that compiles without serde_json or static_assertions.

use std::time::{SystemTime, UNIX_EPOCH};

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

// Compile-time size check (replaces static_assertions)
const _PLATO_TILE_SIZE: [(); 64] = [(); std::mem::size_of::<PlatoTile>()];

impl PlatoTile {
    pub fn priority(&self) -> u8 { ((self.flags >> 4) & 0xF) as u8 }
    pub fn is_alive(&self) -> bool { (self.flags & 0b100) == 0 }
    pub fn has_emerged(&self) -> bool { (self.flags & 0b1000) != 0 }

    pub fn passes_deadband(&self) -> bool {
        let threshold = match self.priority() {
            0 => 0.99, 1 => 0.80, _ => 0.50,
        };
        self.confidence >= threshold
    }

    pub fn pack_domain(domain: &str) -> u32 {
        let mut hash: u32 = 0x811C_9DC5;
        for b in domain.bytes() { hash ^= b as u32; hash = hash.wrapping_mul(0x0100_0193); }
        hash
    }
}

#[derive(Debug, Clone)]
pub struct PlatoRoom {
    pub tiles: Vec<PlatoTile>,
    pub active_count: usize,
    pub domain_id: u32,
}

impl PlatoRoom {
    pub fn new(domain: &str) -> Self {
        PlatoRoom { tiles: Vec::new(), active_count: 0, domain_id: PlatoTile::pack_domain(domain) }
    }

    pub fn push(&mut self, tile: PlatoTile) {
        let alive = tile.is_alive();
        self.tiles.push(tile);
        if alive { self.active_count += 1; }
    }

    pub fn query(&self, min_confidence: f32, max_age_sec: u32, domain_mask: u32) -> Vec<usize> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as u32;
        let mut results = Vec::new();
        for (i, tile) in self.tiles.iter().enumerate() {
            let pass_confidence = tile.confidence >= min_confidence;
            let age = now.saturating_sub(tile.created_at);
            let pass_age = age <= max_age_sec;
            let pass_domain = domain_mask == 0 || (tile.domain_id & domain_mask) != 0;
            let pass_deadband = tile.passes_deadband();
            if pass_confidence && pass_age && pass_domain && pass_deadband && tile.is_alive() {
                results.push(i);
            }
        }
        results
    }

    pub fn reap_expired(&mut self) -> Vec<usize> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as u32;
        let mut reaped = Vec::new();
        for (i, tile) in self.tiles.iter_mut().enumerate() {
            if tile.is_alive() && now >= tile.expires_at {
                tile.flags |= 0b100;
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
                    tile.flags |= 0b1000;
                }
            }
        }
        self.tiles.iter().enumerate()
            .filter(|(_, t)| t.has_emerged())
            .map(|(i, _)| i)
            .collect()
    }
}

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
        let domain_mask = PlatoTile::pack_domain(query);
        let mask = if query.is_empty() { 0 } else { domain_mask };
        let mut results = Vec::new();
        for (room_idx, room) in self.rooms.iter().enumerate() {
            for tile_idx in room.query(min_confidence, max_age_sec, mask) {
                results.push((room_idx, tile_idx));
            }
        }
        results
    }

    pub fn stats(&self) -> String {
        let total: usize = self.rooms.iter().map(|r| r.tiles.len()).sum();
        let active: usize = self.rooms.iter().map(|r| r.active_count).sum();
        format!("rooms={} total_tiles={} active_tiles={} iterations={}", self.rooms.len(), total, active, self.iterations)
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_size() { assert_eq!(std::mem::size_of::<PlatoTile>(), 64); }

    #[test]
    fn test_plato_room_push() {
        let mut room = PlatoRoom::new("fleet_math");
        let tile = PlatoTile {
            tile_id: 1, prev_hash: 0, confidence: 0.95, agent_trust: 0.8,
            created_at: 100, expires_at: 200,
            domain_id: PlatoTile::pack_domain("fleet_math"),
            source_id: 0, flags: 0b0001_0000, tags_hash: 0, reserved: [0u8; 16],
        };
        room.push(tile);
        assert_eq!(room.tiles.len(), 1);
        assert_eq!(room.active_count, 1);
    }

    #[test]
    fn test_deadband_rejection() {
        let mut server = PlatoMythosServer::new();
        let tile = PlatoTile {
            tile_id: 1, prev_hash: 0, confidence: 0.3, agent_trust: 0.1,
            created_at: 100, expires_at: 200,
            domain_id: PlatoTile::pack_domain("fleet_math"),
            source_id: 0, flags: 0b0000_0000, tags_hash: 0, reserved: [0u8; 16],
        };
        assert!(server.submit("fleet_math", tile).is_err());
    }

    #[test]
    fn test_reap_expired() {
        let mut room = PlatoRoom::new("fleet_math");
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as u32;
        let tile = PlatoTile {
            tile_id: 1, prev_hash: 0, confidence: 0.95, agent_trust: 0.8,
            created_at: now - 1000, expires_at: now - 100,
            domain_id: PlatoTile::pack_domain("fleet_math"),
            source_id: 0, flags: 0b0001_0000, tags_hash: 0, reserved: [0u8; 16],
        };
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
}
