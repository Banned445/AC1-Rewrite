//! Static world collision + a kinematic capsule "character proxy".
//!
//! The game uses a Havok-style kinematic proxy (CharacterController__Integrate 0x57C7C0, RE/01 §7):
//! velocity in, sweep against the world, slide along contact planes, write position directly.
//! This is a small equivalent for axis-aligned greybox boxes: sub-stepped movement with
//! capsule-vs-box penetration resolution, step-up and a ground probe.

use bevy::prelude::*;

use crate::tuning::*;

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

impl CollisionWorld {
    /// Capsule segment endpoints for a capsule whose feet are at `feet`.
    fn segment(feet: Vec3) -> (Vec3, Vec3) {
        let a = feet + Vec3::Y * CAPSULE_RADIUS;
        let b = feet + Vec3::Y * (CAPSULE_HEIGHT - CAPSULE_RADIUS);
        (a, b)
    }

    /// Push the capsule out of every overlapping box. Returns (corrected feet, push normals).
    fn depenetrate(&self, mut feet: Vec3, normals: &mut Vec<Vec3>) -> Vec3 {
        for _ in 0..4 {
            let mut moved = false;
            for b in &self.boxes {
                let (a, c) = Self::segment(feet);
                // closest point on segment to box, by sampling the clamped segment against the box
                let p = closest_segment_point_to_aabb(a, c, b);
                let q = p.clamp(b.min, b.max);
                let d = p - q;
                let dist = d.length();
                if dist < CAPSULE_RADIUS {
                    let n = if dist > 1e-5 {
                        d / dist
                    } else {
                        // centre inside the box: push out along the smallest axis
                        smallest_exit_axis(p, b)
                    };
                    let push = CAPSULE_RADIUS - dist + 1e-4;
                    feet += n * push;
                    normals.push(n);
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        feet
    }

    /// Move a capsule from `feet` by `delta`, sliding along walls. Allows stepping up `STEP_HEIGHT`
    /// when `grounded`.
    pub fn move_capsule(&self, feet: Vec3, delta: Vec3, grounded: bool) -> MoveResult {
        let steps = ((delta.length() / (CAPSULE_RADIUS * 0.5)).ceil() as usize).clamp(1, 64);
        let step = delta / steps as f32;
        let mut pos = feet;
        let mut hit_wall = false;
        let mut hit_ceiling = false;
        let mut landed = false;
        let mut normals = Vec::new();
        for _ in 0..steps {
            let target = pos + step;
            normals.clear();
            let mut resolved = self.depenetrate(target, &mut normals);
            let blocked_horizontally = normals.iter().any(|n| n.y.abs() < 0.5);
            if blocked_horizontally && grounded && step.y <= 0.0 {
                // try stepping up onto a low obstacle
                let mut up_normals = Vec::new();
                let raised = self.depenetrate(target + Vec3::Y * STEP_HEIGHT, &mut up_normals);
                if !up_normals.iter().any(|n| n.y.abs() < 0.5) {
                    let dropped = self.snap_down(raised, STEP_HEIGHT + 0.05);
                    resolved = dropped;
                    normals = up_normals;
                }
            }
            for n in &normals {
                if n.y > 0.5 {
                    landed = true;
                } else if n.y < -0.5 {
                    hit_ceiling = true;
                } else {
                    hit_wall = true;
                }
            }
            pos = resolved;
        }
        MoveResult { position: pos, hit_wall, hit_ceiling, landed }
    }

    /// Lower the feet by up to `max` until they touch a box top (or the ground plane at y=0).
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
        let (a, c) = Self::segment(feet + Vec3::Y * 0.02);
        !self.boxes.iter().any(|b| {
            let p = closest_segment_point_to_aabb(a, c, b);
            (p - p.clamp(b.min, b.max)).length() < CAPSULE_RADIUS * 0.95
        })
    }

    /// Height of the supporting surface under the capsule within `max` below the feet.
    pub fn ground_height(&self, feet: Vec3, max: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        let r = CAPSULE_RADIUS * 0.7;
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
