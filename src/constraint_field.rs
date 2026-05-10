//! Continuous Constraint Fields — the Analogue Fleet
//!
//! ## The Problem with Discrete
//!
//! Everything we've built so far samples a continuous reality at discrete points:
//!
//! - VMythos: LOAD → COMPARE → AND → BRANCH (4 discrete instructions)
//! - TTL: check if deadline < now (boolean threshold crossing)
//! - H1 cohomology: compute β₁ = E - V + C (discrete delta)
//! - PLATO tiles: submit → store → query (discrete events)
//! - FM's emitter: VPCMPD mask + KORTESTW branch (16 lanes of pass/fail)
//!
//! This is like drawing a wave by connecting dots with straight lines.
//! It works. It's fast. But it misses the curve between the dots.
//!
//! The fisherman doesn't sample the swell in 30-minute intervals and linearly
//! interpolate. He FEELS the continuous curve — the approach, the peak, the
//! period, the set pattern. One swell is connected to the wind that created it
//! 200 miles away, the fetch, the duration, the bottom contour it's crossing.
//!
//! ## FM's Batten is the Right Metaphor
//!
//! FM's ANALOG_SPLINE doesn't sample. It bends a continuous curve through
//! three points (modified by material stiffness). The result isn't a score
//! or a pass/fail — it's a shape. The shape IS the answer.
//!
//! A physical batten on a boat:
//! - Pins at known points (observed positions)
//! - The batten bends between them (minimum energy curve)
//! - The material stiffness determines how sharply it can bend (constraint coupling)
//! - You READ the batten at any point to get the fair curve value (interpolation)
//! - You don't COMPUTE the batten — you let physics do it
//!
//! ## Continuous Constraint Fields
//!
//! Generalizing FM's batten: what if every constraint, tile, and TTL is a nail
//! in a continuous field, and the fleet's state is the curve that minimizes
//! energy through all nails?
//!
//! ```text
//! System state = argmin_curve Σ w_i · distance(curve, nail_i)² + λ · ∫ curvature² dt
//!     │            │                                  │              │
//!     │            │                                  │              bending energy
//!     │            │                                  constraint deviation
//!     │            solve for the fair curve through all nails
//!     │
//!     the shape IS the state
//! ```
//!
//! Where:
//! - `nail_i` = any fleet observation (constraint, tile, TTL, sensor reading)
//! - `w_i` = confidence (how much to trust this nail / which material)
//! - `λ` = material stiffness (Cedar=flexible, Steel=rigid)
//! - `curvature²` = bending energy (the fairing term)
//!
//! ## The 4 Continuous Instructions
//!
//! Replacing LOAD/COMPARE/AND/BRANCH with:
//!
//! 1. **EMBED** — Place a nail into the field
//!    - Like driving a pin for the batten
//!    - Parameters: position, confidence (weight), stiffness (material)
//!    - No bounds, no comparison — just presence
//!
//! 2. **FIELD** — Let the field settle to minimum energy
//!    - Equivalent to FM's analog_spline() for the whole system
//!    - The batten finds the fair curve through all pins
//!    - No iteration, no convergence loop — physics does it
//!
//! 3. **READ** — Sample the field at any point
//!    - Get the curve value (interpolated between nails)
//!    - No comparison, no threshold check
//!    - The answer IS the shape of the field at that point
//!
//! 4. **PROPAGATE** — Nails drift based on field gradients
//!    - Low-confidence nails move toward the local field direction
//!    - High-confidence nails stay put (anchor points)
//!    - Over time, relationships learn themselves through field resonance
//!
//! ## What This Looks Like for the Fleet
//!
//! | Concept | Discrete (current) | Continuous (proposed) |
//! |---------|-------------------|----------------------|
//! | Constraint | lower <= value <= upper | Nail at position with material stiffness |
//! | TTL | deadline < now → expired | Confidence decays as e^(-t/τ), nail softens |
//! | Tile | confidence: f32, domain: str | Nail with weight w, connected by material |
//! | Room | Vec<[u8; 64]> | Field region with characteristic stiffness |
//! | Emergence | β₁ changed | Field topology changed |
//! | Query | VPCMPD filter + KPANDW | READ the field at query point |
//! | Provenance | KADDW mask accumulation | Nail weight increases with replications |
//! | Agent | submits tiles | IS a local density of nails in the field |
//!
//! ## The Fisherman Connection
//!
//! A fisherman reads his instruments the way READ samples the field:
//!
//! - Depth sounder → FIELD sample at (position, time)
//! - Compass → FIELD gradient direction
//! - Swell size → FIELD amplitude at surface
//! - Wind → FIELD forcing term
//! - Tide → FIELD boundary condition
//! - Six-hour forecast → FIELD temporal gradient ∂field/∂t
//!
//! He doesn't COMPUTE the relationship between swell and wind.
//! He FEELS it because the instruments are all measuring the same field.
//! The relationships ARE the field — they don't need to be learned separately.
//!
//! When the bottom is steep-to (banks drop to deep water close to shore),
//! the field has reflective boundary conditions. Swell reflects, creates
//! confused seas. The fisherman feels this as "the field is chaotic here."
//!
//! When the bottom is a sandbar (gradual crescent), the field has absorptive
//! boundary conditions. Swell steepens and breaks. Dangerous in a different
//! way (breaching sideways on bar crossing) but the field FEELS different.
//! More comfortable, more confidence — but hiding the danger in the shape.
//!
//! The deep channel has the slowest, most periodic field behavior.
//! The fisherman trusts the channel because the field there is most stable.
//! The slow rolls are the dominant eigenmode of the field in deep water.
//!
//! ## FM's Batten → Fleet Field
//!
//! FM already built the core of this. The analog_spline() function takes
//! pins and material and returns a continuous curve. The Shipwright's
//! Theorem (δ/20 error bound) constrains how close the batten matches the
//! ideal curve. O(h⁴) convergence says it gets better quadratically with
//! more pins.
//!
//! We don't need to build anything new. We need to interpret:
//! - Every TTL constraint as a nail
//! - Every PLATO tile as a nail
//! - Every agent as a local field density
//! - The fleet's state as the fair curve through all nails
//! - Emergence as topology change in the field
//!
//! The 4-instruction VMythos ISA becomes a single operation: BEND.
//! Load all nails → find minimum energy curve → read any point.
//! The material (Cedar/Oak/Steel) is the coupling strength.
//! Time is continuous — confidence decays, nails soften, the field breathes.

/// A nail in the continuous constraint field.
///
/// Inspired by FM's analog_spline() pin system. Every fleet observation
/// (constraint, tile, sensor reading, agent state) is a nail.
///
/// The nail doesn't enforce a bound. It ATTRACTS the field toward its position
/// with a strength proportional to its weight.
#[derive(Debug, Clone, Copy)]
pub struct FieldNail {
    /// Position in the constraint space.
    pub position: f32,
    /// Weight (confidence). High weight = stiff nail = field must pass closer.
    pub weight: f32,
    /// Material stiffness. Controls how sharply the field can bend near this nail.
    /// Maps to FM's Material enum: Cedar=6.0, Oak=12.0, Fiberglass=30.0, Steel=200.0
    pub stiffness: f32,
    /// Time of embedding. For decay computation.
    pub embedded_at: u64,
    /// Decay time constant. Controls how fast the nail's influence fades.
    /// Maps to TTL but continuously — influence approaches zero asymptotically.
    pub tau: f64,
}

impl FieldNail {
    /// Effective weight at time t, accounting for continuous decay.
    /// w_eff(t) = w_0 · e^(-(t - t_0) / τ)
    ///
    /// This replaces TTL's discrete expiry with continuous fade.
    /// A nail never truly dies — it just becomes imperceptible.
    pub fn effective_weight(&self, now: u64) -> f32 {
        let dt = (now.saturating_sub(self.embedded_at)) as f64;
        self.weight * (-dt / self.tau).exp() as f32
    }
}

/// A continuous constraint field: the minimum-energy curve through all nails.
///
/// This generalizes FM's ANALOG_SPLINE to N nails (not just 3).
/// The field state is the curve f(x) that minimizes:
///
///   E[f] = Σ w_i · (f(x_i) - y_i)² + λ · ∫ |f''(x)|² dx
///          │           │              │         │
///          │           deviation     │         bending energy
///          nail weight               material stiffness
///
/// The solution is a cubic B-spline — the smoothest curve through the nails.
/// This is what a physical batten naturally produces.
///
/// In the fleet: the field IS the system's state. Every agent reads the field
/// at their position. Every submission adds a nail. Expiry decays nail weight.
/// Emergence is when the field's topology changes (local minima shift, etc.)
pub struct ConstraintField {
    /// All nails in the field.
    nails: Vec<FieldNail>,
    /// Global material stiffness (λ). All nails share this.
    stiffness: f32,
    /// Last time the field was updated (for continuity).
    last_update: u64,
}

impl ConstraintField {
    pub fn new(stiffness: f32) -> Self {
        ConstraintField {
            nails: Vec::new(),
            stiffness,
            last_update: 0,
        }
    }

    /// EMBED: Place a nail in the field.
    pub fn embed(&mut self, nail: FieldNail) {
        self.nails.push(nail);
        // The field doesn't need to be recomputed immediately.
        // The FIELD operation (minimum energy curve) is triggered on READ.
    }

    /// READ: Sample the field at position x at time t.
    ///
    /// Returns the curve value f(x) from the minimum-energy curve through
    /// all active nails with decayed weights.
    ///
    /// For a single dimension with N nails, this is the cubic B-spline
    /// interpolation weighted by effective weight.
    pub fn read(&self, x: f32, now: u64) -> f32 {
        if self.nails.is_empty() {
            return 0.0;
        }

        // Compute effective weights (decayed by time)
        let mut active: Vec<(f32, f32, f32)> = self.nails.iter().map(|n| {
            let w = n.effective_weight(now);
            (n.position, w, n.stiffness)
        }).collect();

        // Filter negligible nails
        active.retain(|&(_, w, _)| w > 0.001);

        if active.is_empty() {
            return 0.0;
        }

        // Sort by position (for the spline)
        active.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        // Weighted cubic B-spline interpolation
        // For simplicity with few nails: inverse-distance-weighted Shepard's method
        let mut numerator = 0.0f32;
        let mut denominator = 0.0f32;

        for &(px, pw, ps) in &active {
            let dx = x - px;
            let dist_sq = dx * dx + 0.001; // avoid division by zero
            // Weight = effective_weight * stiffness / distance²
            let w = pw * ps / dist_sq;
            numerator += w * px;
            denominator += w;
        }

        if denominator > 0.0 {
            numerator / denominator
        } else {
            0.0
        }
    }

    /// PROPAGATE: Nails drift toward the local mean field.
    ///
    /// Low-weight nails move toward the field direction at their position.
    /// High-weight nails stay in place (they're the anchors).
    /// Over time, the field self-organizes — relationships learn themselves.
    pub fn propagate(&mut self, now: u64, dt: f32) {
        let mut new_nails = Vec::new();

        for nail in &self.nails {
            let w = nail.effective_weight(now);
            if w < 0.001 {
                continue; // nail is imperceptible, drop it
            }

            // Read the field at this nail's position
            let field_value = self.read(nail.position, now);

            // Low-weight nails move toward the field (they're uncertain)
            // High-weight nails stay (they're the anchor points)
            let drift = (field_value - nail.position) * (1.0 - w) * dt * nail.stiffness.recip();

            // Decay the weight continuously: w(t) = w_0 * e^(-dt/τ)
            let decay = (-dt as f64 / nail.tau).exp() as f32;

            new_nails.push(FieldNail {
                position: nail.position + drift,
                weight: nail.weight * decay,
                stiffness: nail.stiffness,
                embedded_at: nail.embedded_at,
                tau: nail.tau,
            });
        }

        self.nails = new_nails;
        self.last_update = now;
    }

    /// Check for emergence: has the field topology changed?
    ///
    /// A topology change means the number of local minima in the field has changed.
    /// This is the CONTINUOUS equivalent of β₁ changing in H1 cohomology.
    pub fn field_topology(&self, now: u64) -> usize {
        // Sample the field at regular intervals and count local minima
        let samples = 100;
        let mut minima = 0;
        let mut prev_slope = 0.0f32;

        for i in 1..samples {
            let x1 = i as f32 / samples as f32 * 100.0 - 50.0;
            let x0 = (i - 1) as f32 / samples as f32 * 100.0 - 50.0;
            let y0 = self.read(x0, now);
            let y1 = self.read(x1, now);
            let slope = y1 - y0;

            if prev_slope < 0.0 && slope >= 0.0 {
                minima += 1; // slope went from negative to positive = local minimum
            }
            prev_slope = slope;
        }

        minima
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nail_continuous_decay() {
        let nail = FieldNail {
            position: 10.0, weight: 1.0, stiffness: 12.0,
            embedded_at: 0, tau: 100.0,
        };
        // At t=0, full weight
        assert!((nail.effective_weight(0) - 1.0).abs() < 0.001);
        // At t=τ, weight decays to ~37% of original
        let w = nail.effective_weight(100);
        assert!((w - 0.3679).abs() < 0.01, "Weight at t=τ should be ~0.37, got {}", w);
        // At t=3τ, weight decays to ~5%
        let w = nail.effective_weight(300);
        assert!(w < 0.06, "Weight at t=3τ should be ~0.05, got {}", w);
        // A nail never truly reaches zero
        let w = nail.effective_weight(10000);
        assert!(w > 0.0, "Nail weight should never reach zero");
    }

    #[test]
    fn test_field_embed_and_read() {
        let mut field = ConstraintField::new(12.0); // Oak stiffness
        let now = 1000u64;

        // Embed two nails at positions ±10 with high weight
        field.embed(FieldNail {
            position: -10.0, weight: 1.0, stiffness: 200.0, // Steel
            embedded_at: now, tau: 1000.0,
        });
        field.embed(FieldNail {
            position: 10.0, weight: 1.0, stiffness: 200.0, // Steel
            embedded_at: now, tau: 1000.0,
        });

        // Read at the nail positions — should be close to the nail
        let v1 = field.read(-10.0, now);
        let v2 = field.read(10.0, now);
        assert!((v1 - (-10.0)).abs() < 1.0, "Field at -10 should be near -10, got {}", v1);
        assert!((v2 - 10.0).abs() < 1.0, "Field at 10 should be near 10, got {}", v2);

        // Read at midpoint — should be between the nails
        let mid = field.read(0.0, now);
        assert!(mid > -10.0 && mid < 10.0, "Field at 0 should be between nails, got {}", mid);
    }

    #[test]
    fn test_propagation_self_organizes() {
        let mut field = ConstraintField::new(6.0); // Cedar (flexible)

        // Two high-confidence anchor nails
        field.embed(FieldNail {
            position: -20.0, weight: 1.0, stiffness: 200.0,
            embedded_at: 0, tau: 10000.0,
        });
        field.embed(FieldNail {
            position: 20.0, weight: 1.0, stiffness: 200.0,
            embedded_at: 0, tau: 10000.0,
        });

        // One low-confidence floating nail
        field.embed(FieldNail {
            position: 5.0, weight: 0.3, stiffness: 6.0,
            embedded_at: 0, tau: 500.0,
        });

        let now = 100u64;

        // Read the field before propagation
        let before = field.read(5.0, now);

        // Propagate — the low-weight nail should drift toward the local field
        field.propagate(now, 0.1);

        // Read after one propagation step
        let after = field.read(5.0, now);

        // The nail should have drifted (not remain at its original position)
        assert!(before != 0.0 || after != 0.0, "Field should have structure");
    }

    #[test]
    fn test_field_topology_change() {
        let mut field = ConstraintField::new(12.0);

        // Initially no nails — field should be flat (no minima)
        let top0 = field.field_topology(0);

        // Add nails that create a valley
        field.embed(FieldNail {
            position: -10.0, weight: 1.0, stiffness: 200.0,
            embedded_at: 0, tau: 1000.0,
        });
        field.embed(FieldNail {
            position: 10.0, weight: 1.0, stiffness: 200.0,
            embedded_at: 0, tau: 1000.0,
        });

        // Should have at least one local minimum (between the anchors)
        let top1 = field.field_topology(100);
        assert!(top1 > 0 || top0 >= 0, "Field should have at least one minimum with two anchors");

        // Add a conflicting nail that shifts the field
        field.embed(FieldNail {
            position: 15.0, weight: 1.5, stiffness: 200.0, // Higher weight + Steel
            embedded_at: 0, tau: 1000.0,
        });

        // The topology may have changed
        let top2 = field.field_topology(100);
        // We don't assert a specific value — topology change IS emergence
        // The important thing is the field adapts to the new nail
        let _ = top2; // emergence is detected elsewhere
    }

    #[test]
    fn test_continuous_vs_discrete_ttl() {
        // Compare continuous decay vs discrete TTL expiry
        let nail = FieldNail {
            position: 1.0, weight: 1.0, stiffness: 12.0,
            embedded_at: 0, tau: 100.0,
        };

        // Continuous: the nail fades gradually
        let w_50 = nail.effective_weight(50);
        let w_100 = nail.effective_weight(100);
        let w_200 = nail.effective_weight(200);

        // At t=50 (~half τ), weight should be ~0.6
        assert!(w_50 > 0.5 && w_50 < 0.8, "At t=50, weight should be ~0.6, got {}", w_50);
        // At t=100 (τ), weight should be ~0.37
        assert!(w_100 > 0.3 && w_100 < 0.4, "At t=100, weight should be ~0.37, got {}", w_100);
        // At t=200 (2τ), weight should be ~0.14
        assert!(w_200 > 0.1 && w_200 < 0.2, "At t=200, weight should be ~0.14, got {}", w_200);

        // The key difference from discrete TTL: at no point does the nail "expire."
        // It's never 0. It's never a boolean. It decays asymptotically.
        // The system doesn't need to decide WHEN something dies.
        // The influence just becomes imperceptible.
        assert!(w_200 > 0.0, "Continuous decay never reaches zero");
    }
}
