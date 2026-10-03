//! Static world collision + the character proxy (RE/01 §7, RE/02 §7.1).
//!
//! The game's `CharacterController` (vftable 0x168C484, ctor 0x57B4F0) is Havok's `hkpCharacterProxy`: each frame
//! `CharacterController__Integrate` 0x57C7C0 casts its shape along the velocity (≤ 10 iterations), turns the contacts
//! into planes (0x57A6B0: the plane is pushed out by the keep distance 0.05; a walkable contact steeper than the
//! max slope, cos 45° at +176, also gets a vertical plane, 0x57A1D0), and lets the simplex solver (0x101F3F0) slide
//! the velocity along them. With the "stick to ground" flag (+127) it then casts the shape down and sets it on what
//! it finds (`CharacterController__StickToGround` 0x57D240).
//!
//! The Human's shape (`HumanGround__OnEnterInit` 0xDA7D20 → `PhysicComponent__SetHeight / SetRadius` 0x52ED20 /
//! 0x52ED40, built by 0x52E9C0): a vertical capsule of radius 0.4 m (shape 0.35 + keep distance 0.05) and height
//! 1.8 m, scaled by entity+0x7C (1 for Altaïr). On the ground (`HumanGround__OnActivateSetup` 0xDAE6E0) both flags
//! are set: the capsule is **lifted 0.37 m** (+100, `CharacterController__ApplyStepOffset` 0x579500; its height drops
//! to 1.8 − 0.37) and the stick-to-ground cast reaches **0.58 m** (+96) below the feet. So on the ground nothing lower
//! than 0.37 m touches the capsule (a step), the rounded bottom slides up edges whose contact is within 45° of vertical,
//! and the feet follow drops of up to 0.58 m. Jumps, InAir, Ledge, Climb and Walling clear the stick flag, which also
//! drops the lift: in the air the capsule spans the full 1.8 m from the feet.
//!
//! The port keeps the shape, lift, snap and slope rule on axis-aligned greybox boxes: sub-stepped depenetration in
//! place of Havok's cast + simplex solver (PORT).

use bevy::prelude::*;

use crate::tuning::*;

/// Ctor 0x57B4F0 +176: max walkable slope, cos 45°.
pub const MAX_SLOPE_COS: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// `Human__ShouldFallOffSupport` 0xB23CB0: with no contact flatter than 45°, a ray straight down of 0.8 m.
pub const OFF_SUPPORT_RAY: f32 = 0.8;

#[derive(Clone, Copy, Debug)]
pub struct Aabb3 {
    pub min: Vec3,
    pub max: Vec3,
}

#[derive(Resource, Default)]
pub struct CollisionWorld {
    pub boxes: Vec<Aabb3>,
}

pub struct MoveResult {
    pub position: Vec3,
    pub hit_wall: bool,
    pub hit_ceiling: bool,
    pub landed: bool,
}

/// What the stick-to-ground cast found under the capsule (0x57D240).
#[derive(Clone, Copy, Debug)]
pub struct Support {
    /// Feet height resting on it.
    pub y: f32,
    /// Up component of the contact normal (1 = flat under the capsule; less on a rim).
    pub normal_y: f32,
}

impl CollisionWorld {
    /// Capsule segment (sphere centres) for feet at `feet`, lifted by `lift`.
    fn segment(feet: Vec3, lift: f32) -> (Vec3, Vec3) {
        let a = feet + Vec3::Y * (lift + CAPSULE_RADIUS);
        let b = feet + Vec3::Y * (CAPSULE_HEIGHT - CAPSULE_RADIUS).max(lift + CAPSULE_RADIUS);
        (a, b)
    }

    /// Push the capsule out of every overlapping box. Contacts steeper than the max slope push horizontally (the
    /// vertical plane of 0x57A1D0): the capsule cannot climb them. Returns the corrected feet.
    fn depenetrate(&self, mut feet: Vec3, lift: f32, normals: &mut Vec<Vec3>) -> Vec3 {
        for _ in 0..4 {
            let mut moved = false;
            for b in &self.boxes {
                let (a, c) = Self::segment(feet, lift);
                let p = closest_segment_point_to_aabb(a, c, b);
                let q = p.clamp(b.min, b.max);
                let d = p - q;
                let dist = d.length();
                if dist < CAPSULE_RADIUS {
                    let n = if dist > 1e-5 { d / dist } else { smallest_exit_axis(p, b) };
                    let depth = CAPSULE_RADIUS - dist + 1e-4;
                    let flat = Vec3::new(n.x, 0.0, n.z);
                    let (push_dir, push) = if n.y > 0.0 && n.y < MAX_SLOPE_COS && flat.length_squared() > 1e-8 {
                        // too steep to walk on: a vertical wall through the contact
                        let h = flat.normalize();
                        (h, depth / h.dot(n).max(0.3))
                    } else {
                        (n, depth)
                    };
                    feet += push_dir * push;
                    normals.push(if push_dir == n { n } else { push_dir });
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        feet
    }

    /// Move the capsule from `feet` by `delta`, sliding along what it touches. `grounded` = the Ground setup: the
    /// capsule is lifted by the step offset (`STEP_HEIGHT`, 0.37 m), so low obstacles pass under it; the caller then
    /// sets the feet on the support (`support`).
    pub fn move_capsule(&self, feet: Vec3, delta: Vec3, grounded: bool) -> MoveResult {
        let lift = if grounded { STEP_HEIGHT } else { 0.0 };
        let steps = ((delta.length() / (CAPSULE_RADIUS * 0.5)).ceil() as usize).clamp(1, 64);
        let step = delta / steps as f32;
        let mut pos = feet;
        let mut hit_wall = false;
        let mut hit_ceiling = false;
        let mut landed = false;
        let mut normals = Vec::new();
        for _ in 0..steps {
            normals.clear();
            pos = self.depenetrate(pos + step, lift, &mut normals);
            for n in &normals {
                if n.y >= MAX_SLOPE_COS {
                    landed = true;
                } else if n.y < -0.5 {
                    hit_ceiling = true;
                } else {
                    hit_wall = true;
                }
            }
        }
        MoveResult { position: pos, hit_wall, hit_ceiling, landed }
    }

    /// The stick-to-ground cast (0x57D240): the capsule's rounded bottom swept down from the step offset above the
    /// feet to `SNAP_DOWN` below them. On a box's rim the bottom sphere rests on the corner, lower than the top and
    /// with a tilted contact normal.
    pub fn support(&self, feet: Vec3) -> Option<Support> {
        let r = CAPSULE_RADIUS;
        let mut best: Option<Support> = None;
        for b in &self.boxes {
            let top = b.max.y;
            if top > feet.y + STEP_HEIGHT + 1e-3 || top < feet.y - SNAP_DOWN - r {
                continue;
            }
            let dx = (b.min.x - feet.x).max(feet.x - b.max.x).max(0.0);
            let dz = (b.min.z - feet.z).max(feet.z - b.max.z).max(0.0);
            let d = (dx * dx + dz * dz).sqrt();
            if d >= r {
                continue;
            }
            let rise = (r * r - d * d).sqrt();
            let y = top - (r - rise);
            if y < feet.y - SNAP_DOWN || y > feet.y + STEP_HEIGHT + 1e-3 {
                continue;
            }
            // nothing solid between the capsule and this top (a box above it would hold the capsule first)
            if best.is_none_or(|s| y > s.y) {
                best = Some(Support { y, normal_y: rise / r });
            }
        }
        best
    }

    /// Height of the floor straight below `p` within `max` (a ray, no footprint).
    pub fn floor_height_below(&self, p: Vec3, max: f32) -> Option<f32> {
        self.boxes
            .iter()
            .filter(|b| p.x >= b.min.x && p.x <= b.max.x && p.z >= b.min.z && p.z <= b.max.z && b.max.y <= p.y + 0.02 && b.max.y >= p.y - max)
            .map(|b| b.max.y)
            .reduce(f32::max)
    }

    /// Is there floor within `max` straight below `p` (a ray, no footprint)?
    pub fn floor_below(&self, p: Vec3, max: f32) -> bool {
        self.boxes.iter().any(|b| {
            p.x >= b.min.x && p.x <= b.max.x && p.z >= b.min.z && p.z <= b.max.z && b.max.y <= p.y + 0.02 && b.max.y >= p.y - max
        })
    }

    /// Ground's support rule: the stick-to-ground support, kept while its contact is within 45° of vertical, or with a
    /// floor within 0.8 m straight below (`Human__ShouldFallOffSupport` 0xB23CB0). `None` = the character falls.
    pub fn ground_support(&self, feet: Vec3) -> Option<Support> {
        let s = self.support(feet)?;
        (s.normal_y > MAX_SLOPE_COS || self.floor_below(Vec3::new(feet.x, s.y, feet.z), OFF_SUPPORT_RAY)).then_some(s)
    }

    /// Lower the feet by up to `max` until they touch a box top (or the ground plane at y=0).
    #[allow(dead_code)]
    pub fn snap_down(&self, feet: Vec3, max: f32) -> Vec3 {
        match self.ground_height(feet, max) {
            Some(h) => Vec3::new(feet.x, h, feet.z),
            None => feet,
        }
    }

    /// Is `p` inside any solid box?
    pub fn point_inside(&self, p: Vec3) -> bool {
        self.boxes.iter().any(|b| p.cmpge(b.min).all() && p.cmple(b.max).all())
    }

    /// Distance a sphere of radius `r` can travel from `origin` along `dir` before touching a box
    /// (capped at `max`). Used for the shimmy free-space sweep (ProbeLateral 0xDD9640).
    pub fn sphere_free_distance(&self, origin: Vec3, dir: Vec3, r: f32, max: f32) -> f32 {
        let step = 0.02;
        let mut d = 0.0;
        while d < max {
            let c = origin + dir * (d + step);
            if self.boxes.iter().any(|b| (c - c.clamp(b.min, b.max)).length() < r) {
                return d;
            }
            d += step;
        }
        max
    }

    /// Would a standing capsule fit with its feet at `feet`?
    pub fn capsule_fits(&self, feet: Vec3) -> bool {
        let (a, c) = Self::segment(feet + Vec3::Y * 0.02, 0.0);
        !self.boxes.iter().any(|b| {
            let p = closest_segment_point_to_aabb(a, c, b);
            (p - p.clamp(b.min, b.max)).length() < CAPSULE_RADIUS * 0.95
        })
    }

    /// Height of the supporting surface under a small footprint within `max` below the feet (a query for the
    /// contexts' probes, not the proxy).
    pub fn ground_height(&self, feet: Vec3, max: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        let r = PROBE_FOOTPRINT;
        for b in &self.boxes {
            let inside = feet.x + r > b.min.x && feet.x - r < b.max.x && feet.z + r > b.min.z && feet.z - r < b.max.z;
            if !inside {
                continue;
            }
            let top = b.max.y;
            if top <= feet.y + 0.02 && top >= feet.y - max {
                best = Some(best.map_or(top, |h: f32| h.max(top)));
            }
        }
        best
    }
}

/// PORT: half-width of the contexts' ground queries (`ground_height`).
const PROBE_FOOTPRINT: f32 = 0.21;

fn closest_segment_point_to_aabb(a: Vec3, b: Vec3, bx: &Aabb3) -> Vec3 {
    // Ternary search along the segment for the point closest to the box (distance is convex).
    let dist = |t: f32| {
        let p = a.lerp(b, t);
        (p - p.clamp(bx.min, bx.max)).length_squared()
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let m1 = lo + (hi - lo) / 3.0;
        let m2 = hi - (hi - lo) / 3.0;
        if dist(m1) < dist(m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    a.lerp(b, (lo + hi) * 0.5)
}

fn smallest_exit_axis(p: Vec3, b: &Aabb3) -> Vec3 {
    let cands = [
        (p.x - b.min.x, Vec3::NEG_X),
        (b.max.x - p.x, Vec3::X),
        (p.y - b.min.y, Vec3::NEG_Y),
        (b.max.y - p.y, Vec3::Y),
        (p.z - b.min.z, Vec3::NEG_Z),
        (b.max.z - p.z, Vec3::Z),
    ];
    cands.iter().min_by(|x, y| x.0.total_cmp(&y.0)).unwrap().1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> CollisionWorld {
        // floor slab, a 0.3 m step, a 0.45 m step, a 0.6 m block and a 2 m wall
        let b = |x0: f32, x1: f32, h: f32| Aabb3 { min: Vec3::new(x0, -1.0, -2.0), max: Vec3::new(x1, h, 2.0) };
        CollisionWorld { boxes: vec![b(-20.0, 20.0, 0.0), b(2.0, 3.0, 0.3), b(5.0, 6.0, 0.45), b(8.0, 9.0, 0.6), b(11.0, 12.0, 2.0)] }
    }

    fn walk(w: &CollisionWorld, from: Vec3, to_x: f32) -> Vec3 {
        let mut f = from;
        for _ in 0..400 {
            let r = w.move_capsule(f, Vec3::X * 0.05, true);
            f = r.position;
            match w.ground_support(f) {
                Some(s) => f.y = s.y,
                None => break,
            }
            if f.x >= to_x {
                break;
            }
        }
        f
    }

    #[test]
    fn the_lifted_capsule_steps_over_low_obstacles_and_is_stopped_by_high_ones() {
        let w = world();
        // 0.3 m: under the 0.37 m step offset
        let f = walk(&w, Vec3::new(0.0, 0.0, 0.0), 2.5);
        assert!(f.x >= 2.5 && (f.y - 0.3).abs() < 1e-3, "0.3 m step: {f:?}");
        // 0.45 m: the rounded bottom meets the edge within 45° of vertical and slides up it
        let f = walk(&w, Vec3::new(4.0, 0.0, 0.0), 5.5);
        assert!(f.x >= 5.5 && (f.y - 0.45).abs() < 1e-3, "0.45 m step: {f:?}");
        // 0.6 m: too steep, a wall
        let f = walk(&w, Vec3::new(7.0, 0.0, 0.0), 8.5);
        assert!(f.x < 8.0 - CAPSULE_RADIUS + 0.05 && f.y.abs() < 1e-3, "0.6 m block: {f:?}");
    }

    #[test]
    fn standing_past_a_roof_edge_holds_until_the_rim_tilts_past_45_degrees() {
        let w = CollisionWorld { boxes: vec![Aabb3 { min: Vec3::new(-5.0, 0.0, -5.0), max: Vec3::new(0.0, 3.0, 5.0) }] };
        let on = |x: f32| w.ground_support(Vec3::new(x, 3.0, 0.0));
        assert!((on(-0.5).unwrap().y - 3.0).abs() < 1e-5);
        // 0.2 m past: on the rim, sunk by r − √(r² − d²)
        let s = on(0.2).unwrap();
        assert!(s.y < 3.0 && s.y > 2.9, "{s:?}");
        // 0.3 m past (contact > 45°, 0.4·sin45 = 0.283): falls
        assert!(on(0.3).is_none());
    }
}
