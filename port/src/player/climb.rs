//! Climb context (`HumanClimb`, ActorContextID 10) — RE/03 §1–§6.
//!
//! - Every Wait frame a local **hold grid** is built around the feet: columns 0.75 m, rows 0.6 m;
//!   7 or 8 columns/rows depending on the pose (BuildHoldGrid 0xDF6A40). Cells are FOOT cells; the
//!   hand of that side holds the cell two rows higher.
//! - The stick is quantised into 10 directions (QuantizeStickDirection 0xDEB8D0) and a move is looked
//!   up by (pose, direction) in the SHORT table, or first in the LONG table when the stick is pushed
//!   past 0.5 (ChooseMove 0xDFDE90). Tables dumped from StaticInitTables 0xDE4F80 (RE/03 §4.4).
//! - A move is valid when the destination foot cell and the hand cell above it both hold
//!   (IsGridMoveValid 0xDECD70). The root is interpolated over the move's animation length, or 0.5 s
//!   when there is no animation (StartMove 0xDFA0C0) — we have no animations yet, so always 0.5 s.
//! - No move up → transition to a ledge hang (TryTransitionToLedgeHang 0xDF4BA0); Legs → release.

use bevy::prelude::*;

use super::air::{FallOrigin, InAirEntry};
use super::ledge::{LedgeEntry, LedgeSubState};
use super::{heading_of, right_of, switch_context, ActorContextId, Body, HumanDataBundle, LimbTargets, Locomotion, Player, RootInterp, TransitionSetup};
use crate::guidance::GuidanceWorld;
use crate::input::PadInput;
use crate::tuning::*;

/// `HumanClimbData::EntryType` (desc 0x1997864).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClimbEntryType {
    #[default]
    Default = 0,
    FromLedge = 1,
    FromLedgeParallelJump = 2,
    FromGround = 3,
}

#[derive(Clone, Copy, Debug)]
pub struct ClimbEntry {
    pub entry_type: ClimbEntryType,
    pub hand_l: Vec3,
    pub hand_r: Vec3,
    pub foot_l: Vec3,
    pub foot_r: Vec3,
    pub normal: Vec3,
    pub from_feet: Vec3,
}

/// Pose table (0x1A2CD70): foot cells (col, row) of the left and right side.
pub const POSES: [((i32, i32), (i32, i32)); 6] = [
    ((3, 2), (3, 2)), // 0 same column, level ("1M")
    ((3, 3), (3, 2)), // 1 same column, left higher
    ((3, 2), (3, 3)), // 2 same column, right higher
    ((3, 2), (4, 2)), // 3 one column apart, level ("2M")
    ((3, 3), (4, 2)), // 4 apart, left higher
    ((3, 2), (4, 3)), // 5 apart, right higher
];

/// Which side(s) a move displaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    L,
    R,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveEntry {
    None,
    /// Look the move up again with another direction (table codes 9..14).
    Redirect(usize),
    /// (next pose, side, dx, dy) in cells.
    To(usize, Side, i32, i32),
}

use MoveEntry::{None as N, Redirect as Rd, To};
use Side::{Both, L, R};

/// Directions: 0 Up(L) 1 Up(R) 2 Down(L) 3 Down(R) 4 Left 5 Right 6 UpLeft 7 UpRight 8 DownLeft 9 DownRight.
/// SHORT table (0x1A2D070), RE/03 §4.4.
pub const SHORT: [[MoveEntry; 10]; 6] = [
    // pose 0
    [To(1, L, 0, 1), To(2, R, 0, 1), To(2, L, 0, -1), To(1, R, 0, -1), To(3, L, -1, 0), To(3, R, 1, 0),
     To(4, L, -1, 1), To(5, R, 1, 1), To(5, L, -1, -1), To(4, R, 1, -1)],
    // pose 1 (left higher)
    [To(0, R, 0, 1), To(0, R, 0, 1), To(0, L, 0, -1), To(0, L, 0, -1), To(4, L, -1, 0), To(4, R, 1, 0),
     Rd(4), To(3, R, 1, 1), To(3, L, -1, -1), Rd(5)],
    // pose 2 (right higher)
    [To(0, L, 0, 1), To(0, L, 0, 1), To(0, R, 0, -1), To(0, R, 0, -1), To(5, L, -1, 0), To(5, R, 1, 0),
     To(3, L, -1, 1), Rd(5), Rd(4), To(3, R, 1, -1)],
    // pose 3 (apart, level)
    [To(4, L, 0, 1), To(5, R, 0, 1), To(5, L, 0, -1), To(4, R, 0, -1), To(0, R, -1, 0), To(0, L, 1, 0),
     Rd(0), Rd(1), Rd(2), Rd(3)],
    // pose 4 (apart, left higher)
    [To(3, R, 0, 1), To(3, R, 0, 1), To(3, L, 0, -1), To(3, L, 0, -1), To(1, R, -1, 0), To(1, L, 1, 0),
     To(0, R, -1, 1), Rd(0), Rd(2), To(0, L, 1, -1)],
    // pose 5 (apart, right higher)
    [To(3, L, 0, 1), To(3, L, 0, 1), To(3, R, 0, -1), To(3, R, 0, -1), To(2, R, -1, 0), To(2, L, 1, 0),
     Rd(0), To(0, L, 1, 1), To(0, R, -1, -1), Rd(2)],
];

/// LONG table (0x1A2D7F0): strong push — hand-over-hand 1.2 m, or both sides shuffle.
pub const LONG: [[MoveEntry; 10]; 6] = [
    [N; 10],
    [To(2, R, 0, 2), To(2, R, 0, 2), To(2, L, 0, -2), To(2, L, 0, -2), N, N, N, N, N, N],
    [To(1, L, 0, 2), To(1, L, 0, 2), To(1, R, 0, -2), To(1, R, 0, -2), N, N, N, N, N, N],
    [N, N, N, N, To(3, Both, -1, 0), To(3, Both, 1, 0), N, N, N, N],
    [To(5, R, 0, 2), To(5, R, 0, 2), To(5, L, 0, -2), To(5, L, 0, -2), To(4, Both, -1, 0), To(4, Both, 1, 0), N, N, N, N],
    [To(4, L, 0, 2), To(4, L, 0, 2), To(4, R, 0, -2), To(4, R, 0, -2), To(5, Both, -1, 0), To(5, Both, 1, 0), N, N, N, N],
];

/// Action id per (pose, dir) of the SHORT table (0X1A2D070 + 24*(pose*10+dir) + 20), dumped by
/// emulating HumanClimb__StaticInitTables (RE/data/climb_init.pkl). 0 = none / redirect entry.
pub const SHORT_ACTIONS: [[u32; 10]; 6] = [
    [0x019A05F1, 0x019A05F2, 0x019A05F4, 0x019A05F3, 0x019A05F5, 0x019A05F6, 0x019A05F7, 0x019A05F8, 0x019A05F9, 0x019A05FA],
    [0x019A36CF, 0x019A36CF, 0x019A36D0, 0x019A36D0, 0x019A36D1, 0x019A36D2, 0xFFFFFFFF, 0x019A36D3, 0x019A36D4, 0xFFFFFFFF],
    [0x019A36E7, 0x019A36E7, 0x019A36E8, 0x019A36E8, 0x019A36E9, 0x019A36EA, 0x019A36EB, 0xFFFFFFFF, 0xFFFFFFFF, 0x019A36EC],
    [0x01A267EC, 0x01A267ED, 0x01A267EF, 0x01A267EE, 0x01A267F0, 0x01A267F2, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF],
    [0x01A26815, 0x01A26815, 0x01A26817, 0x01A26817, 0x01A26819, 0x01A2681B, 0x01A2681D, 0xFFFFFFFF, 0xFFFFFFFF, 0x01A2681E],
    [0x01A26833, 0x01A26833, 0x01A26835, 0x01A26835, 0x01A26837, 0x01A26839, 0xFFFFFFFF, 0x01A2683B, 0x01A2683C, 0xFFFFFFFF],
];
/// Action id per (pose, dir) of the LONG table (0X1A2D7F0 + 24*(pose*10+dir) + 20), dumped by
/// emulating HumanClimb__StaticInitTables (RE/data/climb_init.pkl). 0 = none / redirect entry.
pub const LONG_ACTIONS: [[u32; 10]; 6] = [
    [0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000],
    [0x019A36D5, 0x019A36D5, 0x019A36D6, 0x019A36D6, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000],
    [0x019A36ED, 0x019A36ED, 0x019A36EE, 0x019A36EE, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000],
    [0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x01A267F1, 0x01A267F3, 0x00000000, 0x00000000, 0x00000000, 0x00000000],
    [0x01A26816, 0x01A26816, 0x01A26818, 0x01A26818, 0x01A2681A, 0x01A2681C, 0x00000000, 0x00000000, 0x00000000, 0x00000000],
    [0x01A26834, 0x01A26834, 0x01A26836, 0x01A26836, 0x01A26838, 0x01A2683A, 0x00000000, 0x00000000, 0x00000000, 0x00000000],
];

/// Pose wait actions (pose table 0x1A2CD70 field animStateId), poses 0..5.
pub const POSE_ACTIONS: [u32; 6] = [0x012D_A39F, 0x012D_A3A0, 0x012D_A3A1, 0x012D_A3A2, 0x012D_A3A3, 0x012D_A3A4];

/// Direction after following redirects (the move's own table entry, which carries its action id).
pub fn resolve_dir(table: &[[MoveEntry; 10]; 6], pose: usize, dir: usize) -> usize {
    let mut d = dir;
    for _ in 0..3 {
        match table[pose][d] {
            MoveEntry::Redirect(nd) => d = nd,
            _ => return d,
        }
    }
    d
}

/// Resolve redirects (codes 9..14 in the game).
pub fn lookup(table: &[[MoveEntry; 10]; 6], pose: usize, dir: usize) -> Option<(usize, Side, i32, i32)> {
    let mut d = dir;
    for _ in 0..3 {
        match table[pose][d] {
            MoveEntry::None => return None,
            MoveEntry::Redirect(nd) => d = nd,
            MoveEntry::To(p, s, dx, dy) => return Some((p, s, dx, dy)),
        }
    }
    None
}

/// 10-way stick quantization (0xDEB8D0). `a` = signed angle from facing; negative = left.
pub fn quantize(stick: Vec3, facing: Vec3) -> usize {
    let a = stick.dot(right_of(facing)).atan2(stick.dot(facing)).to_degrees();
    match a {
        a if (-22.5..0.0).contains(&a) => 0,
        a if (0.0..22.5).contains(&a) => 1,
        a if (-67.5..-22.5).contains(&a) => 6,
        a if (-112.5..-67.5).contains(&a) => 4,
        a if (-157.5..-112.5).contains(&a) => 8,
        a if (22.5..67.5).contains(&a) => 7,
        a if (67.5..112.5).contains(&a) => 5,
        a if (112.5..157.5).contains(&a) => 9,
        a if a < 0.0 => 2,
        _ => 3,
    }
}

/// Runtime data of the Climb context (subset of reflected HumanClimbData, RE/03 §2.2).
#[derive(Debug, Default)]
pub struct HumanClimbData {
    pub entry_type: ClimbEntryType,
    /// HumanClimb+0x38: current pose (0..5).
    pub pose: usize,
    pub foot_l: Vec3,
    pub foot_r: Vec3,
    pub hand_l: Vec3,
    pub hand_r: Vec3,
    pub normal: Vec3,
    pub last_action: &'static str,
    pub moving: Option<RootInterp>,
    /// Action (animation graph id) of the running grid move, from the SHORT/LONG table entry.
    pub move_action: Option<u32>,
    /// Incremented per grid move (restarts the move clip).
    pub move_seq: u32,
}

impl HumanClimbData {
    pub fn enter(&mut self, e: ClimbEntry) {
        self.entry_type = e.entry_type;
        self.normal = e.normal;
        self.foot_l = e.foot_l;
        self.foot_r = e.foot_r;
        self.hand_l = e.hand_l;
        self.hand_r = e.hand_r;
        // EnterCommon 0xDE97B0: pose 3 for a "2M" entry (sides one column apart), else pose 0.
        self.pose = if (e.foot_r - e.foot_l).length() > CLIMB_COL * 0.6 { 3 } else { 0 };
        self.last_action = "enter";
        self.move_action = None;
        self.moving = Some(RootInterp::new(e.from_feet, climb_root(e.foot_l, e.foot_r, e.normal), CLIMB_MOVE_TIME));
    }
}

/// Root (animation origin) while climbing. The game builds it from the hold frames at runtime
/// (ComputeRootFromMove 0xDEC6E0 → 0xB1CC20); the offsets here come from the game's own climb wait clips
/// (`xx_climb_wait_*`, probe in assets/probe.rs): the lower ankle sits 0.03 m above the root and the
/// wrists 0.44 m in front of it, so the root is CLIMB_ROOT_OUT from the hold line.
pub fn climb_root(foot_l: Vec3, foot_r: Vec3, n: Vec3) -> Vec3 {
    let mid = (foot_l + foot_r) * 0.5;
    Vec3::new(mid.x, foot_l.y.min(foot_r.y) - CLIMB_ROOT_BELOW_FOOT, mid.z) + n * CLIMB_ROOT_OUT
}


/// The hold grid around the current feet (BuildHoldGrid 0xDF6A40).
pub struct HoldGrid {
    origin: Vec3,
    right: Vec3,
    x0: f32,
    cols: i32,
    rows: i32,
    holds: Vec<Option<Vec3>>,
}

impl HoldGrid {
    pub fn build(g: &GuidanceWorld, d: &HumanClimbData) -> Self {
        let facing = -Vec3::new(d.normal.x, 0.0, d.normal.z).normalize_or_zero();
        let right = right_of(facing);
        let (pl, pr) = POSES[d.pose];
        let apart = pl.0 != pr.0;
        let uneven = pl.1 != pr.1;
        let (cols, x0) = if apart { (8, -3.0) } else { (7, -2.625) };
        let rows = if uneven { 8 } else { 7 };
        // frame: x centred on the feet midpoint (pose foot columns are symmetric about it), z = lowest foot
        let mid = (d.foot_l + d.foot_r) * 0.5;
        let origin = Vec3::new(mid.x, d.foot_l.y.min(d.foot_r.y), mid.z);
        let mut holds = vec![None; (cols * rows) as usize];
        for r in 0..rows {
            for c in 0..cols {
                let cx = x0 + CLIMB_COL * 0.5 + CLIMB_COL * c as f32;
                // rows start at -1.5 m with a +0.3 m centre offset → centre = -1.2 + 0.6·r (row 2 = feet)
                let cz = -1.2 + CLIMB_ROW * r as f32;
                let centre = origin + right * cx + Vec3::Y * cz;
                if let Some(h) = g.probe(centre, CLIMB_PROBE_R + 0.4, CLIMB_PROBE_VTOL, Some(facing), 0.785) {
                    // a hit counts only if it lies in the same cell
                    let lx = (h.point - origin).dot(right);
                    let lz = h.point.y - origin.y;
                    if (lx - cx).abs() <= CLIMB_COL * 0.5 + 1e-3 && (lz - cz).abs() <= CLIMB_ROW * 0.5 {
                        // snap the hold onto the cell column (bands are continuous; holds are points)
                        let snapped = h.point + right * (cx - lx);
                        holds[(r * cols + c) as usize] = Some(snapped);
                    }
                }
            }
        }
        HoldGrid { origin, right, x0, cols, rows, holds }
    }

    pub fn hold(&self, c: i32, r: i32) -> Option<Vec3> {
        if c < 0 || r < 0 || c >= self.cols || r >= self.rows {
            return None;
        }
        self.holds[(r * self.cols + c) as usize]
    }

    #[allow(dead_code)]
    pub fn debug_counts(&self) -> (usize, i32, i32, Vec3, Vec3, f32) {
        (self.holds.iter().filter(|h| h.is_some()).count(), self.cols, self.rows, self.origin, self.right, self.x0)
    }
}

/// IsGridMoveValid 0xDECD70: every moving side needs a foot hold and a hand hold two rows up.
fn try_move(grid: &HoldGrid, pose: usize, m: (usize, Side, i32, i32)) -> Option<(usize, [Option<(Vec3, Vec3)>; 2])> {
    let (next, side, dx, dy) = m;
    let (pl, pr) = POSES[pose];
    let mut out = [None, None];
    for (i, (c, r)) in [pl, pr].into_iter().enumerate() {
        let moves = matches!((side, i), (Side::Both, _) | (Side::L, 0) | (Side::R, 1));
        if !moves {
            continue;
        }
        let (nc, nr) = (c + dx, r + dy);
        let foot = grid.hold(nc, nr)?;
        let hand = grid.hold(nc, nr + CLIMB_HAND_ROWS)?;
        out[i] = Some((foot, hand));
    }
    Some((next, out))
}

pub fn update_climb(
    time: Res<Time>,
    mut pad: ResMut<PadInput>,
    guidance: Res<GuidanceWorld>,
    collision: Res<crate::collision::CollisionWorld>,
    mut q: Query<(&mut Locomotion, &mut Body, &mut HumanDataBundle, &mut LimbTargets), With<Player>>,
) {
    let dt = time.delta_secs().min(1.0 / 20.0);
    for (mut loco, mut body, mut data, mut limbs) in &mut q {
        if loco.current != ActorContextId::Climb {
            continue;
        }
        if loco.just_switched {
            loco.just_switched = false;
            continue;
        }
        let d = &mut data.climb;
        let n = d.normal;
        let facing = -Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
        body.heading = heading_of(facing);
        body.velocity = Vec3::ZERO;
        body.grounded = false;
        limbs.hands = Some((d.hand_l, d.hand_r));
        limbs.feet = Some((d.foot_l, d.foot_r));
        limbs.normal = n;
        // a moving limb lands on its new hold a little before the root settles
        limbs.transit = d.moving.map(|m| m.duration).unwrap_or(CLIMB_MOVE_TIME) * 0.8;

        // Move state: advance the root interpolation (StateMove_Update 0xDF5990)
        if let Some(m) = d.moving.as_mut() {
            let (p, _) = m.advance(dt);
            body.feet = p;
            if m.done() {
                d.moving = None;
            }
            continue;
        }

        // Wait state (StateWait_Update 0xDFE760)
        if pad.jump_buffered() {
            pad.consume_jump();
            limbs.hands = None;
            limbs.feet = None;
            let back = pad.high_profile && pad.speed01 > 0.0 && matches!(quantize(pad.dir, facing), 2 | 3 | 8 | 9);
            let entry = if back {
                // back eject (TryBackEject 0xDF2F50) — hypothesis: high profile + Legs + stick away
                InAirEntry::FreeJump { from: body.feet, dir: n, speed_param: 0.5 }
            } else {
                // release (StartRelease 0xDE96A0 → InAir, FallOrigin_Climb)
                InAirEntry::Fall { from: body.feet, velocity: Vec3::ZERO, origin: FallOrigin::Climb, speed_param: 0.0 }
            };
            switch_context(&mut loco, &mut data, TransitionSetup::ToInAir(entry));
            continue;
        }
        if pad.speed01 <= 0.0 {
            d.last_action = "hold";
            continue;
        }
        let dir = quantize(pad.dir, facing);
        let grid = HoldGrid::build(&guidance, d);
        // ChooseMove 0xDFDE90: LONG first when the stick is pushed hard, then SHORT
        let mut chosen = None;
        if pad.magnitude > CLIMB_LONG_STICK {
            chosen = lookup(&LONG, d.pose, dir).and_then(|m| try_move(&grid, d.pose, m).map(|r| (r, m, LONG_ACTIONS[d.pose][resolve_dir(&LONG, d.pose, dir)])));
        }
        if chosen.is_none() {
            chosen = lookup(&SHORT, d.pose, dir).and_then(|m| try_move(&grid, d.pose, m).map(|r| (r, m, SHORT_ACTIONS[d.pose][resolve_dir(&SHORT, d.pose, dir)])));
        }
        if let Some(((next, sides), _, action)) = chosen {
            d.move_action = (action != 0 && action != u32::MAX).then_some(action);
            d.move_seq += 1;
            if let Some((f, h)) = sides[0] {
                d.foot_l = f;
                d.hand_l = h;
            }
            if let Some((f, h)) = sides[1] {
                d.foot_r = f;
                d.hand_r = h;
            }
            d.pose = next;
            d.moving = Some(RootInterp::new(body.feet, climb_root(d.foot_l, d.foot_r, n), CLIMB_MOVE_TIME));
            d.last_action = match dir {
                0 | 1 => "climb up",
                2 | 3 => "climb down",
                4 => "climb left",
                5 => "climb right",
                _ => "climb diagonal",
            };
            continue;
        }
        // no grid move: going up → hang from the current hand holds if they are a top edge
        // (TryTransitionToLedgeHang 0xDF4BA0 / TryReachLedgeAbove 0xDF1730)
        if matches!(dir, 0 | 1 | 6 | 7) {
            let mid = (d.hand_l + d.hand_r) * 0.5;
            let top = Vec3::new(mid.x, mid.y, mid.z) - n * PULLUP_IN;
            let top_edge = collision.ground_height(top + Vec3::Y * 0.05, 0.3).is_some_and(|h| (h - mid.y).abs() < 0.25);
            if top_edge {
                let entry = LedgeEntry {
                    hand_l: d.hand_l,
                    hand_r: d.hand_r,
                    normal: n,
                    from_feet: body.feet,
                    sub_state: LedgeSubState::TransitionInFromClimb,
                    entry_move: None,
                };
                switch_context(&mut loco, &mut data, TransitionSetup::ToLedge(entry));
                continue;
            }
        }
        // OnNoMoveFound 0xDF4410: "blocked" anim for the direction
        d.last_action = "blocked";
    }
}
