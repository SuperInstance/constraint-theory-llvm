//! Analog Compute Module — Spline-based analog computation for PLATO rooms
//! 
//! Physical splines solve constraint problems for free via material physics.
//! This module provides digital simulations of analog behavior for testing
//! and benchmarking, designed to be replaced with physical measurements later.
//!
//! Key design: This module is the "lofting floor" — digital simulation
//! before we commit to physical prototype (Phase 3).


/// Material properties for analog spline computation.
/// E = Young's modulus in GPa. Density in g/cm³.
/// These match the FLUX-C constant table (flux-c-analog-design.md).
#[derive(Debug, Clone, Copy)]
pub enum Material {
    Cedar,      // E = 6.0 GPa, density = 0.4 g/cm³, very flexible
    Oak,        // E = 12.0 GPa, density = 0.7 g/cm³, moderate stiffness  
    Fiberglass, // E = 30.0 GPa, density = 2.0 g/cm³, semi-rigid
    Steel,     // E = 200.0 GPa, density = 7.8 g/cm², rigid
}

impl Material {
    /// Young's modulus in GPa
    pub fn e(&self) -> f32 {
        match self {
            Material::Cedar => 6.0,
            Material::Oak => 12.0,
            Material::Fiberglass => 30.0,
            Material::Steel => 200.0,
        }
    }
    /// Density in g/cm³
    pub fn density(&self) -> f32 {
        match self {
            Material::Cedar => 0.4,
            Material::Oak => 0.7,
            Material::Fiberglass => 2.0,
            Material::Steel => 7.8,
        }
    }
    /// Material variation tolerance (typical ±5%)
    pub fn variation(&self) -> f32 {
        0.05
    }
}

/// A point in 2D space with optional tangent direction
#[derive(Debug, Clone)]
pub struct Nail {
    pub x: f32,
    pub y: f32,
    pub tangent: Option<(f32, f32)>, // (dx, dy) tangent direction
}

/// Result of analog spline computation
#[derive(Debug)]
pub struct SplineResult {
    /// Control points used (the "nails")
    pub control_points: Vec<(f32, f32)>,
    /// Material used
    pub material: Material,
    /// Output curve points (x, y, curvature)
    pub curve_points: Vec<(f32, f32, f32)>,
    /// GUARD tolerance for tile validity
    pub tolerance: f32,
}

/// ANALOG_SPLINE: Compute a quadratic Bézier spline from 3 boundary points.
/// 
/// This simulates the behavior of a physical batten bent between three nails.
/// The output curve has C1 continuity (continuous first derivative) at the
/// middle control point.
/// 
/// Preconditions:
///   - points.len() == 3 (three nails define a unique quadratic)
///   - all points finite
///   - points[0].y <= points[1].y <= points[2].y (ascending Y)
///   - material.e() > 0.0
/// 
/// Postconditions:
///   - curve passes through points[0] and points[2] exactly
///   - curve has C1 continuity at points[1]
///   - all output coordinates finite
pub fn analog_spline(points: &[(f32, f32)], material: Material, tension: f32) -> SplineResult {
    assert!(points.len() == 3, "ANALOG_SPLINE requires exactly 3 control points");
    
    let (x0, y0) = points[0];
    let (x1, y1) = points[1];
    let (x2, y2) = points[2];
    
    // Quadratic Bézier: B(t) = (1-t)²P0 + 2(1-t)tP1 + t²P2, t ∈ [0,1]
    // The middle point P1 is the control point that determines the curve shape
    
    let num_samples = (20.0 + tension * 80.0) as usize; // 20-100 samples based on tension
    let mut curve_points = Vec::with_capacity(num_samples);
    
    for i in 0..num_samples {
        let t = i as f32 / (num_samples - 1) as f32;
        let t_sq = t * t;
        let one_minus_t = 1.0 - t;
        let one_minus_t_sq = one_minus_t * one_minus_t;
        
        // Bézier formula
        let bx = one_minus_t_sq * x0 + 2.0 * one_minus_t * t * x1 + t_sq * x2;
        let by = one_minus_t_sq * y0 + 2.0 * one_minus_t * t * y1 + t_sq * y2;
        
        // Compute curvature at this point
        // For quadratic Bézier: first derivative B'(t), second derivative B''(t)
        // B'(t) = 2[(P1-P0)(1-t) + (P2-P1)t]
        // B''(t) = 2(P2 - 2P1 + P0)
        let d2x = 2.0 * (x2 - 2.0 * x1 + x0);
        let d2y = 2.0 * (y2 - 2.0 * y1 + y0);
        let curvature = (d2x * d2x + d2y * d2y).sqrt();
        
        curve_points.push((bx, by, curvature));
    }
    
    // GUARD tolerance: ε + material_variation × tension
    let eps = 1e-6;
    let tol = eps + material.variation() * tension;
    
    SplineResult {
        control_points: points.to_vec(),
        material,
        curve_points,
        tolerance: tol,
    }
}

/// ANALOG_WATER_LEVEL: Compute the least-squares level surface through points.
/// 
/// Given a set of 2D points, finds the horizontal line Y that minimizes
/// the sum of squared vertical deviations. This is the arithmetic mean of Y values.
/// 
/// This simulates a communicating vessels water level system — physics solves
/// Laplace's equation for gravitational potential.
pub fn analog_water_level(points: &[(f32, f32)]) -> f32 {
    assert!(points.len() >= 3, "ANALOG_WATER_LEVEL requires at least 3 points");
    
    let sum_y: f32 = points.iter().map(|p| p.1).sum();
    sum_y / points.len() as f32
}

/// ANALOG_STORY_POLE: Transfer a level surface to different heights via deltas.
/// 
/// Given an anchor Y and an array of vertical deltas, produces absolute Y positions:
/// result[i] = anchor + sum(deltas[0..i])
/// 
/// This simulates a shipwright's story pole — a notched stick where each notch
/// represents a frame's position, transferred from lofting floor to hull.
pub fn analog_story_pole(anchor: f32, deltas: &[f32]) -> Vec<f32> {
    let mut results = Vec::with_capacity(deltas.len());
    let mut cumulative = anchor;
    
    for delta in deltas {
        cumulative += delta;
        results.push(cumulative);
    }
    
    results
}

/// ANALOG_SECTOR: Divide a total distance into N equal proportional segments.
/// 
/// Returns N segment lengths, each = distance / divisor.
/// 
/// This simulates the shipwright's sector — a folding geometric tool that
/// divides lengths into equal parts WITHOUT doing division arithmetically.
/// It uses similar triangles to compute proportional divisions.
pub fn analog_sector(distance: f32, divisor: u32) -> Vec<f32> {
    assert!(divisor >= 2 && divisor <= 256, "ANALOG_SECTOR: divisor must be 2..256");
    assert!(distance > 0.0, "ANALOG_SECTOR: distance must be positive");
    
    let segment_length = distance / divisor as f32;
    vec![segment_length; divisor as usize]
}

/// Tile validity check: is a tile position within tolerance of the room spline?
pub fn tile_valid(tile: (f32, f32), spline: &SplineResult) -> bool {
    // Find the Y value of the spline at the tile's X coordinate
    let (tx, ty) = tile;
    
    // Binary search for closest segment
    let x0 = spline.control_points[0].0;
    let x2 = spline.control_points[2].0;
    
    if tx <= x0 {
        let (_, sy, _) = spline.curve_points[0];
        let deviation = (ty - sy).abs();
        return deviation < spline.tolerance;
    }
    if tx >= x2 {
        let (_, sy, _) = spline.curve_points[spline.curve_points.len() - 1];
        let deviation = (ty - sy).abs();
        return deviation < spline.tolerance;
    }
    
    // Find closest point on spline
    let t = (tx - x0) / (x2 - x0);
    let idx = (t * (spline.curve_points.len() - 1) as f32) as usize;
    let idx = idx.min(spline.curve_points.len() - 1);
    
    let (_, sy, _) = spline.curve_points[idx];
    let deviation = (ty - sy).abs();
    deviation < spline.tolerance
}

/// Compute distance from tile to spline curve (for diagnostics)
pub fn tile_distance(tile: (f32, f32), spline: &SplineResult) -> f32 {
    let (tx, ty) = tile;
    let x0 = spline.control_points[0].0;
    let x2 = spline.control_points[2].0;
    
    if tx <= x0 {
        let (_, sy, _) = spline.curve_points[0];
        return (ty - sy).abs();
    }
    if tx >= x2 {
        let (_, sy, _) = spline.curve_points[spline.curve_points.len() - 1];
        return (ty - sy).abs();
    }
    
    let t = (tx - x0) / (x2 - x0);
    let idx = (t * (spline.curve_points.len() - 1) as f32) as usize;
    let idx = idx.min(spline.curve_points.len() - 1);
    
    let (_, sy, _) = spline.curve_points[idx];
    (ty - sy).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analog_spline_basic() {
        // Three points: (0,0), (1,2), (2,0) — forms an arch
        let points = [(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)];
        let result = analog_spline(&points, Material::Cedar, 0.5);
        
        // First and last curve points should match first/last control points
        let (x0, y0, _) = result.curve_points[0];
        let (x2, y2, _) = result.curve_points[result.curve_points.len() - 1];
        
        assert!((x0 - 0.0).abs() < 0.01);
        assert!((y0 - 0.0).abs() < 0.01);
        assert!((x2 - 2.0).abs() < 0.01);
        assert!((y2 - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_analog_water_level() {
        let points = [(0.0, 1.0), (1.0, 2.0), (2.0, 3.0)];
        let level = analog_water_level(&points);
        assert!((level - 2.0).abs() < 0.001); // mean of 1,2,3 = 2.0
    }

    #[test]
    fn test_analog_story_pole() {
        let anchor = 10.0;
        let deltas = [1.0, 2.0, 3.0];
        let results = analog_story_pole(anchor, &deltas);
        
        assert!((results[0] - 11.0).abs() < 0.001); // 10 + 1
        assert!((results[1] - 13.0).abs() < 0.001); // 11 + 2
        assert!((results[2] - 16.0).abs() < 0.001); // 13 + 3
    }

    #[test]
    fn test_analog_sector() {
        let segments = analog_sector(10.0, 5);
        assert_eq!(segments.len(), 5);
        for &s in &segments {
            assert!((s - 2.0).abs() < 0.001); // 10/5 = 2.0
        }
    }

    #[test]
    fn test_tile_valid_on_curve() {
        let points = [(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)];
        let spline = analog_spline(&points, Material::Oak, 0.5);
        
        // A tile at the spline's peak (t=0.5 on the Bézier) should be valid.
        // The quadratic Bézier arch peaks at (1.0, 1.0) not (1.5, 1.0).
        // B(0.5) = (1-0.5)²(0,0) + 2(1-0.5)(0.5)(1,2) + 0.5²(2,0) = (0,0) + (0.5,1.0) + (0.5,0) = (1.0,1.0)
        let (tx, ty) = (1.0, 1.0);
        assert!(tile_valid((tx, ty), &spline), 
            "tile at curve peak should be valid, got distance {}", 
            tile_distance((tx, ty), &spline));
    }

    #[test]
    fn test_tile_valid_off_curve() {
        let points = [(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)];
        let spline = analog_spline(&points, Material::Oak, 0.5);
        
        // A tile far from the curve should be invalid
        let (tx, ty) = (1.0, 10.0); // way above the arch peak
        assert!(!tile_valid((tx, ty), &spline));
    }
}

// =============================================================================
// FLUX-C Opcode Encodings (0xD0–0xD3)
// Format G: [opcode(1)][length(1)][payload(N)]
// =============================================================================

/// FLUX-C opcode for ANALOG_SPLINE (0xD0)
/// Encoding: 0xD0, length=34, payload = 3×(x:f32,y:f32) + material_E:f32 + tension:f32
pub const FLUX_ANALOG_SPLINE: u8 = 0xD0;
pub const FLUX_ANALOG_SPLINE_LEN: usize = 32;

/// FLUX-C opcode for ANALOG_WATER_LEVEL (0xD1)
/// Encoding: 0xD1, length=9, payload = point_array_ptr(u32) + count(u32)
pub const FLUX_ANALOG_WATER_LEVEL: u8 = 0xD1;
pub const FLUX_ANALOG_WATER_LEVEL_LEN: usize = 9;

/// FLUX-C opcode for ANALOG_STORY_POLE (0xD2)
/// Encoding: 0xD2, length=10, payload = anchor(f32) + delta_array_ptr(u32) + count(u32)
pub const FLUX_ANALOG_STORY_POLE: u8 = 0xD2;
pub const FLUX_ANALOG_STORY_POLE_LEN: usize = 10;

/// FLUX-C opcode for ANALOG_SECTOR (0xD3)
/// Encoding: 0xD3, length=9, payload = distance(f32) + divisor(u32)
pub const FLUX_ANALOG_SECTOR: u8 = 0xD3;
pub const FLUX_ANALOG_SECTOR_LEN: usize = 9;

/// Encode ANALOG_SPLINE opcode bytes (big-endian f32 layout)
pub fn encode_analog_spline(
    points: &[(f32, f32); 3],
    material_e: f32,
    tension: f32,
) -> [u8; 36] {
    // 36 bytes = 1 opcode + 1 length + 32 payload (3 pts + mat + tens)
    let mut buf = [0u8; 36];
    buf[0] = FLUX_ANALOG_SPLINE;
    buf[1] = 0x20; // length = 32 bytes total for this opcode
    
    
    // Write 3 points (x, y) at offsets 2, 10, 18
    for (i, &(x, y)) in points.iter().enumerate() {
        let base = 2 + i * 8;
        buf[base..base+4].copy_from_slice(&x.to_le_bytes());
        buf[base+4..base+8].copy_from_slice(&y.to_le_bytes());
    }

    // material_E at offset 26 (2 + 24)
    buf[26..30].copy_from_slice(&material_e.to_le_bytes());
    // tension at offset 30 (2 + 24 + 4)
    buf[30..34].copy_from_slice(&tension.to_le_bytes());
    
    buf
}

/// Parse ANALOG_SPLINE opcode bytes into usable values
pub fn decode_analog_spline(buf: &[u8; 36]) -> Option<([(f32,f32); 3], f32, f32)> {
    if buf[0] != FLUX_ANALOG_SPLINE || buf[1] != 0x20 { return None; }
    
    let mut points = [(0f32, 0f32); 3];
    for i in 0..3 {
        let base = 2 + i * 8;
        let x = f32::from_le_bytes(buf[base..base+4].try_into().ok()?);
        let y = f32::from_le_bytes(buf[base+4..base+8].try_into().ok()?);
        points[i] = (x, y);
    }
    let material_e = f32::from_le_bytes(buf[26..30].try_into().ok()?);
    let tension = f32::from_le_bytes(buf[30..34].try_into().ok()?);
    Some((points, material_e, tension))
}
