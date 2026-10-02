//! Greybox test level: rooftops with gaps sized around the game's jump bands (RE/01 §7b),
//! a tall tower for fall-damage tests and a few low obstacles for step-up.
//! Placeholder until real level geometry is loaded from .forge.

use bevy::prelude::*;

use crate::collision::{Aabb3, CollisionWorld};
use crate::guidance::{GuidanceEdge, GuidanceSubType, GuidanceWorld};

pub struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.62, 0.72, 0.82)))
            // sky fill light: with Bevy's default (80) every face turned from the sun renders near-black, and the
            // baked folds of Altaïr's robe texture read as dark blotches
            .insert_resource(GlobalAmbientLight { color: Color::srgb(0.80, 0.86, 1.0), brightness: 1500.0, ..default() })
            .add_systems(Startup, build_level);
    }
}

/// (centre x, centre z, size x, size z, height)
const BUILDINGS: &[(f32, f32, f32, f32, f32)] = &[
    // a row of rooftops with gaps of 2, 3.5, 5 and 6.5 m
    (0.0, 12.0, 6.0, 6.0, 3.0),
    (8.0, 12.0, 6.0, 6.0, 3.5),
    (17.5, 12.0, 6.0, 6.0, 3.0),
    (28.5, 12.0, 6.0, 6.0, 4.0),
    (41.0, 12.0, 6.0, 6.0, 3.0),
    // stepped heights going up by ~1.2 m (just under the 1.3 m max-up of ground jumps)
    (0.0, 24.0, 5.0, 5.0, 2.0),
    (7.0, 24.0, 5.0, 5.0, 3.2),
    (14.0, 24.0, 5.0, 5.0, 4.4),
    (21.0, 24.0, 5.0, 5.0, 5.6),
    // a high block (too high to jump up to) and drops for landing tests
    (30.0, 26.0, 6.0, 6.0, 9.5),
    (-12.0, 4.0, 5.0, 5.0, 6.0),   // 6 m drop: heavy-damage threshold is 6.3 m
    (-12.0, 14.0, 5.0, 5.0, 8.0),  // 8 m drop: fatal (> 7.0 m)
    (-12.0, 24.0, 5.0, 5.0, 3.5),  // 3.5 m drop: roll (> 3 m)
    // low obstacles (step-up) and a wall
    (6.0, 0.0, 1.5, 1.5, 0.3),
    (9.0, 0.0, 1.5, 1.5, 0.6),
    (0.0, -8.0, 14.0, 0.6, 2.4),
    // --- stage 2: climbing ---
    (-20.0, 30.0, 6.0, 6.0, 9.6),  // climb tower: hold bands on its -Z face (CLIMB_FACE)
    (12.0, 42.0, 8.0, 0.6, 2.6),   // jump-up wall: top edge 2.6 m (ledge band max up 3.0 m)
    (16.0, 41.1, 1.0, 1.2, 3.5),   // pillar at the wall's +X end: blocks shimmy (inner corner)
    // --- ledge moves (RE/03 §7.6) ---
    (33.0, 50.0, 6.0, 0.6, 2.6),   // L-wall, part A (x 30..36): hang on its -Z face …
    (36.3, 47.15, 0.6, 6.3, 4.0),  // … part B rises above it at x 36, with a stone ledge at 2.6 m (WALL_LEDGES)
    (20.0, 50.0, 6.0, 0.6, 2.6),   // side-jump wall C (x 17..23) …
    (25.5, 50.0, 3.0, 0.6, 2.6),   // … and D (x 24..27): a 1 m gap in the same edge line
    (42.0, 50.0, 4.0, 0.6, 2.6),   // hop-up wall E (x 40..44): hang at 2.6 m …
    (42.0, 50.15, 4.0, 0.3, 4.2),  // … with a ledge 1.6 m higher, set back 0.3 m (E2)
    // --- standing straight jump bands (0xB21DA0) ---
    (50.0, 50.3, 3.0, 1.0, 1.6),   // knee height: jump, hangknee, stand on top
    (56.0, 50.3, 3.0, 1.0, 2.2),   // 2.0–2.5 m with a wall below: jump into a wall hang
    // --- hang-type switch (0xDE1060): wall F (x 60..63) continues as an overhang slab (SLABS) with no wall below
    (61.5, 50.0, 3.0, 0.6, 2.6),
    // --- wall run (Walling, RE/05 §1): faces at z 59.25, run at them along +Z ---
    (70.0, 60.0, 3.0, 1.5, 1.8), // probe A: pull-up onto the top from the entry
    (76.0, 60.0, 3.0, 1.5, 3.8), // the vertical step, then probe C: hang from the top edge
    (82.0, 60.0, 3.0, 1.5, 6.0), // no ledge in reach: vertical end, drop back
];

/// Haystacks (centre x, centre z, size x, size z, height): not solid, jump targets of type 0x800. The first
/// one sits 4.5 m off the high block's +X face (roof 9.5 m): the Leap of Faith test.
pub const HAYSTACKS: &[(f32, f32, f32, f32, f32)] = &[(37.5, 26.0, 2.2, 2.2, 1.5)];

/// Floating slabs (centre x, top y, centre z, size x, size z, thickness): free-hang ledges.
const SLABS: &[(f32, f32, f32, f32, f32, f32)] = &[
    (2.0, 3.0, 36.0, 6.0, 1.2, 0.3),
    (64.5, 2.6, 50.0, 3.0, 0.6, 0.3), // overhang continuing wall F's ledge (hang-type switch test)
];

/// Extra ledges on wall faces (p0, p1, outward normal): stone ledges that are not roof edges.
const WALL_LEDGES: &[(Vec3, Vec3, Vec3)] = &[
    // along part B's -X face, meeting part A's ledge at the inner corner (x 36, z 49.7)
    (Vec3::new(35.92, 2.6, 44.0), Vec3::new(35.92, 2.6, 49.7), Vec3::NEG_X),
];

/// Climb tower face: x range, face z (normal -Z), band heights 0.6 m apart, plus a missing patch.
const CLIMB_FACE: (f32, f32, f32) = (-22.6, -17.4, 27.0);
const CLIMB_BANDS: std::ops::RangeInclusive<i32> = 1..=15; // 0.6 .. 9.0 m
/// How far the stone bands stick out of the tower face.
const CLIMB_BAND_DEPTH: f32 = 0.08;
/// Missing holds (x range, y range) to test blocked grid moves.
const CLIMB_GAP: ((f32, f32), (f32, f32)) = ((-18.6, -17.4), (2.9, 4.3));

/// Pure level data (collision + guidance), shared by the renderer and the simulation tests.
pub fn geometry() -> (CollisionWorld, GuidanceWorld) {
    let mut collision = CollisionWorld::default();
    let mut guidance = GuidanceWorld::default();
    collision.boxes.push(Aabb3 { min: Vec3::new(-100.0, -1.0, -100.0), max: Vec3::new(100.0, 0.0, 100.0) });
    for &(x, z, sx, sz, h) in BUILDINGS {
        let min = Vec3::new(x - sx * 0.5, 0.0, z - sz * 0.5);
        let max = Vec3::new(x + sx * 0.5, h, z + sz * 0.5);
        collision.boxes.push(Aabb3 { min, max });
        if h >= 1.0 {
            add_roof_edges(&mut guidance, min, max);
        }
    }
    for &(x, top, z, sx, sz, t) in SLABS {
        let min = Vec3::new(x - sx * 0.5, top - t, z - sz * 0.5);
        let max = Vec3::new(x + sx * 0.5, top, z + sz * 0.5);
        collision.boxes.push(Aabb3 { min, max });
        add_roof_edges(&mut guidance, min, max);
    }
    for &(x, z, sx, sz, h) in HAYSTACKS {
        guidance.haystacks.push(Aabb3 { min: Vec3::new(x - sx * 0.5, 0.0, z - sz * 0.5), max: Vec3::new(x + sx * 0.5, h, z + sz * 0.5) });
    }
    for &(p0, p1, n1) in WALL_LEDGES {
        guidance.edges.push(GuidanceEdge { p0, p1, n0: Vec3::Y, n1, subtype: GuidanceSubType::LedgeGrab });
    }
    // climbing holds: horizontal stone bands on the tower face, split at the gap
    let (x0, x1, fz) = CLIMB_FACE;
    let ((gx0, gx1), (gy0, gy1)) = CLIMB_GAP;
    for k in CLIMB_BANDS {
        let y = 0.6 * k as f32;
        let segments: Vec<(f32, f32)> =
            if (gy0..=gy1).contains(&y) { vec![(x0, gx0)] } else { vec![(x0, x1)] };
        for (a, b) in segments {
            guidance.edges.push(GuidanceEdge {
                // holds are the bands' outer top edges (the band sticks out CLIMB_BAND_DEPTH from the face)
                p0: Vec3::new(a, y, fz - CLIMB_BAND_DEPTH),
                p1: Vec3::new(b, y, fz - CLIMB_BAND_DEPTH),
                n0: Vec3::Y,
                n1: Vec3::NEG_Z,
                subtype: GuidanceSubType::LedgeGrab,
            });
        }
    }
    (collision, guidance)
}

fn build_level(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut collision: ResMut<CollisionWorld>,
    mut guidance: ResMut<GuidanceWorld>,
) {
    let (c, g) = geometry();
    *collision = c;
    *guidance = g;

    let ground_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.78, 0.70, 0.55),
        perceptual_roughness: 0.95,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(200.0, 200.0))),
        MeshMaterial3d(ground_mat),
    ));
    let wall_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.86, 0.80, 0.68),
        perceptual_roughness: 0.9,
        ..default()
    });
    for &(x, z, sx, sz, h) in BUILDINGS {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(sx, h, sz))),
            MeshMaterial3d(wall_mat.clone()),
            Transform::from_xyz(x, h * 0.5, z),
        ));
    }
    for &(x, top, z, sx, sz, t) in SLABS {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(sx, t, sz))),
            MeshMaterial3d(wall_mat.clone()),
            Transform::from_xyz(x, top - t * 0.5, z),
        ));
    }
    let hay_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.85, 0.72, 0.30), perceptual_roughness: 1.0, ..default() });
    for &(x, z, sx, sz, h) in HAYSTACKS {
        commands.spawn((Mesh3d(meshes.add(Cuboid::new(sx, h, sz))), MeshMaterial3d(hay_mat.clone()), Transform::from_xyz(x, h * 0.5, z)));
    }
    // visual stone bands on the climb tower
    let band_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.62, 0.55, 0.45), ..default() });
    let (x0, x1, fz) = CLIMB_FACE;
    let ((gx0, _), (gy0, gy1)) = CLIMB_GAP;
    for k in CLIMB_BANDS {
        let y = 0.6 * k as f32;
        let (a, b) = if (gy0..=gy1).contains(&y) { (x0, gx0) } else { (x0, x1) };
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(b - a, 0.08, CLIMB_BAND_DEPTH))),
            MeshMaterial3d(band_mat.clone()),
            Transform::from_xyz((a + b) * 0.5, y - 0.04, fz - CLIMB_BAND_DEPTH * 0.5),
        ));
    }

    // light
    commands.spawn((
        DirectionalLight { illuminance: 12_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(30.0, 60.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Four LedgeGrab edges around a roof: n0 = up (roof), n1 = outward wall normal.
fn add_roof_edges(g: &mut GuidanceWorld, min: Vec3, max: Vec3) {
    let y = max.y;
    let c = [
        Vec3::new(min.x, y, min.z),
        Vec3::new(max.x, y, min.z),
        Vec3::new(max.x, y, max.z),
        Vec3::new(min.x, y, max.z),
    ];
    let normals = [Vec3::NEG_Z, Vec3::X, Vec3::Z, Vec3::NEG_X];
    for i in 0..4 {
        g.edges.push(GuidanceEdge {
            p0: c[i],
            p1: c[(i + 1) % 4],
            n0: Vec3::Y,
            n1: normals[i],
            subtype: GuidanceSubType::LedgeGrab,
        });
    }
}
