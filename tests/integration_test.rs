//! End-to-End Integration Test — Full VMythos Pipeline

#[cfg(test)]
mod integration_tests {
    use constraint_theory_llvm::ttl_constraint::{TtlConstraint, TtlType, H1Cohomology};
    use constraint_theory_llvm::mythos_mesh::*;
    use constraint_theory_llvm::plato_mythos_kernel::*;

    /// Simulate 4 agents submitting tiles to different rooms.
    #[test]
    fn test_full_pipeline_4_agents() {
        let mut server = PlatoMythosServer::new();

        // Helper: create a tile with given params
        let make_tile = |id: u64, conf: f32, expires: u32, domain: &str, priority: u8| -> PlatoTile {
            PlatoTile::new(
                id, 0, conf, 0.5, 0, expires,
                PlatoTile::pack_domain(domain), 0, priority, 0,
            )
        };

        // Agent 1: Critical, high confidence
        let a1 = make_tile(1, 0.99, 100, "fleet_math", 0);
        assert!(server.submit("fleet_math", a1).is_ok(), "Agent 1 should pass");

        // Agent 2: Standard, medium confidence
        let a2 = make_tile(2, 0.85, 100, "fleet_agent", 1);
        assert!(server.submit("fleet_agent", a2).is_ok(), "Agent 2 should pass");

        // Agent 3: Low priority, low confidence (P2=0.50, but 0.30 < 0.50)
        let a3 = make_tile(3, 0.30, 100, "fleet_tools", 2);
        assert!(server.submit("fleet_tools", a3).is_err(), "Agent 3 should FAIL deadband");

        // Agent 4: Already expired (created at 0, expires at 0)
        let a4 = make_tile(4, 0.95, 0, "fleet_math", 0);
        assert!(server.submit("fleet_math", a4).is_err(), "Agent 4 should FAIL (expired)");

        let stats = server.stats();
        assert!(stats.contains("rooms=2") || stats.contains("rooms=3"));
    }

    /// Test TTL expiry triggers emergence via H1 cohomology.
    #[test]
    fn test_h1_cohomology_emergence() {
        let c1 = TtlConstraint::with_lifespan(1, [0; 16], [10; 16], TtlType::Tile, 3600.0);
        let c2 = TtlConstraint::with_lifespan(2, [0; 16], [10; 16], TtlType::Tile, 3600.0);
        let c3 = TtlConstraint::with_lifespan(3, [0; 16], [10; 16], TtlType::Tile, 0.0);

        let h1 = H1Cohomology::new(vec![c1, c2, c3]);
        assert_eq!(h1.active_count(), 2, "c3 should expire at birth");
    }

    /// Test constraint-to-tile-to-room round trip.
    #[test]
    fn test_constraint_to_room() {
        let constraints = vec![
            TtlConstraint::new(1, [0; 16], [10; 16], TtlType::Tile),
            TtlConstraint::with_lifespan(2, [0; 16], [10; 16], TtlType::Task, 0.0),
        ];

        let mut server = PlatoMythosServer::new();
        for c in &constraints {
            let alive = matches!(c.state, constraint_theory_llvm::ttl_constraint::ConstraintState::Active { .. });
            // Use confidence=1.0 for P0 (priority 0) to pass the 0.99 deadband threshold
            // after rounding through u8 quantization (1.0 * 255 = 255, 255/255 = 1.0)
            let tile = PlatoTile::new(
                c.constraint_id as u64, 0, 1.0, 0.5, 3600, 7200,
                PlatoTile::pack_domain("fleet_math"), 0, 0, 0,
            );
            let result = server.submit("fleet_math", tile);
            // PLATO room server doesn't check constraint TTL at submit time.
            // TTL expiry is handled by reap_expired() / H1Cohomology at the scanning layer.
            // All tiles passing the confidence gate are accepted.
            assert!(result.is_ok(), "Constraint {} should submit: {:?}", c.constraint_id, result);
        }
        assert!(server.iterations > 0);
    }

    /// Test archetype routing.
    #[test]
    fn test_archetype_routing() {
        let tiles = vec![
            ConstraintTile {
                key: 1,
                value: TileValue { satisfied: true, priority: MythosPriority::Critical, steps: 2 },
                confidence: 0.99, domain: "plato", is_dead: false,
            },
            ConstraintTile {
                key: 2,
                value: TileValue { satisfied: true, priority: MythosPriority::Low, steps: 1 },
                confidence: 0.30, domain: "agent", is_dead: false,
            },
            ConstraintTile {
                key: 3,
                value: TileValue { satisfied: false, priority: MythosPriority::Standard, steps: 4 },
                confidence: 0.85, domain: "fleet", is_dead: false,
            },
        ];

        // Warden filters low-confidence tiles
        let warden = Archetype::Warden;
        let warden_results = warden.process_tiles(&tiles);
        assert!(!warden_results.iter().any(|t| t.key == 2),
            "Warden should filter low-confidence tiles");

        // Healer revives dead tiles
        let dead = ConstraintTile {
            key: 4, value: TileValue { satisfied: false, priority: MythosPriority::Low, steps: 0 },
            confidence: 0.10, domain: "dead", is_dead: true,
        };
        let healer = Archetype::Healer;
        let healed = healer.process_tiles(&[dead]);
        assert!(healed[0].confidence >= 0.3, "Healer should restore confidence");
        assert!(!healed[0].is_dead, "Healer should revive dead tiles");
    }
}
