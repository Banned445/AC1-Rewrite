//! Ledge moves beyond the shimmy and the hand steps (RE/03 §7.6, decoded 2026-10-01):
//! - **corner turns** (`HumanLedge__TryTurnCorner` 0xDD3BB0, candidates from `HumanLedge__FindCornerEdges`
//!   0xDD0600): inner corner (the wall ahead) and outer corner (around the end of the wall);
//! - **side jumps between ledges** (`HumanLedge__TrySideJumpToLedge` 0xDDD490 → `StartLedgeJump` 0xDDCE40,
//!   table 0x1A2C780 / 0x1A2C980);
//! - **hop up to a ledge above** (`HumanLedge__TryWallJumpUp` 0xDD62A0 → the blended `StartLedgeJump`).
//!
//! The move plays its action(s) while the root travels from the current hang to the target hang. Ledge
//! jumps follow the clips' blended displacement plus a linear correction to the target (hypothesis: the
//! same root interpolation mode as the InAir jump, RE/13 §3); corners use the root interpolator over the
//! clip length (0xDD3BB0 → sub_711130, flag 0).

use bevy::prelude::*;

use super::jump_blend::{self, ActionBlend};
use super::ledge::{hang_root, hang_type_at, LedgeHangType};
use super::right_of;
use crate::collision::CollisionWorld;
use crate::guidance::GuidanceWorld;
use crate::tuning::*;

/// Ledge-jump table (0x1A2C780 wall hang / 0x1A2C980 free hang; filled by HumanLedge__StaticInitTables
/// 0xDCC2A0, read from the emulated init `RE/data/ledge_init.pkl`). Index `(type·2 + long)·4 + side`;
/// entry = (landing category, start action, loop action, end action). Types: 0 climb holds, 1 wall-hang
/// ledge, 2 free-hang ledge, 3 ladder. Sides: 0 up, 1 down, 2 left, 3 right.
#[rustfmt::skip]
pub const LEDGE_JUMP_TABLE: [[(u8, [u32; 3]); 32]; 2] = [
    [
        (0, [0x491228AE, 0x491228AF, 0x491228B0]), (0, [0; 3]), (0, [0x491228FE, 0x491228FF, 0x49122900]), (0, [0x49122904, 0x49122905, 0x49122906]),
        (0, [0x491228B1, 0x491228B2, 0x491228B3]), (0, [0; 3]), (0, [0x49122901, 0x49122902, 0x49122903]), (0, [0x49122907, 0x49122908, 0x49122909]),
        (0, [0; 3]), (0, [0; 3]), (1, [0x491228C6, 0x491228C7, 0x491228C8]), (1, [0x491228CC, 0x491228CD, 0x491228CE]),
        (0, [0; 3]), (0, [0; 3]), (1, [0x491228C9, 0x491228CA, 0x491228CB]), (1, [0x491228CF, 0x491228D0, 0x491228D1]),
        (0, [0; 3]), (0, [0; 3]), (2, [0x491228DE, 0x491228DF, 0x491228E0]), (2, [0x491228E6, 0x491228E7, 0x491228E8]),
        (0, [0; 3]), (0, [0; 3]), (2, [0x491228E1, 0x491228E2, 0x491228E3]), (2, [0x491228E9, 0x491228EA, 0x491228EB]),
        (0, [0; 3]), (0, [0; 3]), (3, [0x491228FE, 0x491228FF, 0x52557360]), (3, [0x49122904, 0x49122905, 0x52557362]),
        (0, [0; 3]), (0, [0; 3]), (3, [0x49122901, 0x49122902, 0x52557361]), (3, [0x49122907, 0x49122908, 0x52557363]),
    ],
    [
        (0, [0; 3]), (0, [0; 3]), (0, [0x4C460359, 0x4C46035A, 0x4C46035B]), (0, [0x4C46035F, 0x4C460360, 0x4C460361]),
        (0, [0; 3]), (0, [0; 3]), (0, [0x4C46035C, 0x4C46035D, 0x4C46035E]), (0, [0x4C460362, 0x4C460363, 0x4C460364]),
        (0, [0; 3]), (0, [0; 3]), (1, [0x49122916, 0x49122917, 0x49122918]), (1, [0x4912291C, 0x4912291D, 0x4912291E]),
        (0, [0; 3]), (0, [0; 3]), (1, [0x49122919, 0x4912291A, 0x4912291B]), (1, [0x4912291F, 0x49122920, 0x49122921]),
        (0, [0; 3]), (0, [0; 3]), (2, [0x4C460372, 0x4C460373, 0x4C460374]), (2, [0x4C46037A, 0x4C46037B, 0x4C46037C]),
        (0, [0; 3]), (0, [0; 3]), (2, [0x4C460375, 0x4C460376, 0x4C460377]), (2, [0x4C46037D, 0x4C46037E, 0x4C46037F]),
        (0, [0; 3]), (0, [0; 3]), (3, [0x4C460359, 0x4C46035A, 0x52557360]), (3, [0x4C46035F, 0x4C460360, 0x52557362]),
        (0, [0; 3]), (0, [0; 3]), (3, [0x4C46035C, 0x4C46035D, 0x52557361]), (3, [0x4C460362, 0x4C460363, 0x52557363]),
    ],
];

/// Corner actions (0xDD3BB0: 510126209 + k). Free hang: `hangfree_corner_{left,right}_090_{in,out}`; wall
/// hang: the two-item strafe open + close actions with the hand targets on the new edge.
/// [free, wall] × [left in, left out, right in, right out].
pub const CORNER_ACTIONS: [[u32; 4]; 2] = [
    [0x1E67_E881, 0x1E67_E882, 0x1E67_E883, 0x1E67_E884],
    [0x1E67_E885, 0x1E67_E886, 0x1E67_E887, 0x1E67_E888],
];
/// Blended hop up (0xDDCE40 with flag 1861&2): `hangwall_to_swingback_up_{min,max}_{200,300}_a`, then
/// (0xDDAB00) `…_b` with the same weights while the root moves to the new ledge; it ends in a free hang.
pub const HOP_UP: u32 = 0x4CD6_8F4F;
pub const HOP_UP_B: u32 = 0x4CD6_8F50;

pub const DUMPED_ACTIONS: &[u32] = &[
    0x491228AE, 0x491228AF, 0x491228B0, 0x491228B1, 0x491228B2, 0x491228B3,
    0x491228C6, 0x491228C7, 0x491228C8, 0x491228C9, 0x491228CA, 0x491228CB,
    0x491228CC, 0x491228CD, 0x491228CE, 0x491228CF, 0x491228D0, 0x491228D1,
    0x491228DE, 0x491228DF, 0x491228E0, 0x491228E1, 0x491228E2, 0x491228E3,
    0x491228E6, 0x491228E7, 0x491228E8, 0x491228E9, 0x491228EA, 0x491228EB,
    0x49122916, 0x49122917, 0x49122918, 0x49122919, 0x4912291A, 0x4912291B,
    0x4912291C, 0x4912291D, 0x4912291E, 0x4912291F, 0x49122920, 0x49122921,
    0x4C460372, 0x4C460373, 0x4C460374, 0x4C460375, 0x4C460376, 0x4C460377,
    0x4C46037A, 0x4C46037B, 0x4C46037C, 0x4C46037D, 0x4C46037E, 0x4C46037F,
    0x1E67_E881, 0x1E67_E882, 0x1E67_E883, 0x1E67_E884, 0x1E67_E885, 0x1E67_E886, 0x1E67_E887, 0x1E67_E888,
    HOP_UP, HOP_UP_B,
    // hang-type switches (0xDE1060)
    TO_WALL[0], TO_WALL[1], TO_FREE[0], TO_FREE[1], TO_FREE[2], TO_FREE[3],
    // straight jump to a hand target (0xB21DA0) and its arrivals (0xE07D00)
    0x0129_0ECF, 0x0127_2A69, 0x0127_1631, 0x0127_1639, 0x0121_A598, 0x0121_A8B1,
    0x0129_0ED0, 0x0127_2A6A, 0x0127_163A, 0x0127_1632, 0x0121_B072, 0x0127_23A5,
    ACT_WAIST_TO_KNEE, ACT_KNEE_TO_WAIT,
    // running jump onto a ledge: wall reception and free-hang swing (0xE07D00 generic branch)
    RECEPTION_SURFACE_WALL, SWING_RECEPTION,
    // pull-down (0xDDE4D0 / 0xDDE980)
    PULLDOWN_ORIENT[0], PULLDOWN_ORIENT[1], PULLDOWN_DESCENT, PULLDOWN_WALL[0], PULLDOWN_WALL[1], PULLDOWN_FREE[0], PULLDOWN_FREE[1],
    // ledge stop (HumanGround sub-state 38, 0xD93C60 / 0xD7D9D0)
    LEDGE_STOP_START, LEDGE_STOP_END,
    // pull-up (Pullup_Start 0xDDBE80)
    ACT_PULLUP_WALL, ACT_PULLUP_FREE,
];

/// Ledge stop: `xx_h_ledge_stop_start_footl` (played on entry, 0xD93C60) and `xx_h_ledge_stop_end_footl` (played
/// when the start action is done, 0xD7D9D0; its transition 0x06E8BD7E leads to wait).
pub const LEDGE_STOP_START: u32 = 0x06E8_BD7F;
pub const LEDGE_STOP_END: u32 = 0x06E8_BD7D;

/// One item of an action with weight 1 (for the ground's one-shot actions).
pub fn single_item(id: u32, item: usize) -> Option<ActionBlend> {
    single(id, item)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    Corner { inner: bool },
    SideJump { long: bool },
    HopUp,
    /// Received after a jump at a ledge (0xE07D00).
    Arrival,
    /// Wall ↔ free hang (0xDE1060).
    SwitchHang { to_wall: bool },
    /// Onto the top (Pullup_Start 0xDDBE80).
    Pullup,
    /// Ground → hang over an edge (0xDDE4D0 / 0xDDE980): orientation, descent, reception.
    PullDown { stage: u8 },
}

/// A running ledge move.
#[derive(Clone, Copy, Debug)]
pub struct LedgeMove {
    pub kind: MoveKind,
    /// The action items played in sequence.
    pub seq: [Option<ActionBlend>; 4],
    pub durations: [f32; 4],
    pub t: f32,
    pub from: Vec3,
    pub to: Vec3,
    /// Facing (into the wall) at the start and at the end.
    pub facing_from: Vec3,
    pub facing_to: Vec3,
    /// The root follows the clips' displacement (+ correction) instead of the interpolator.
    pub follow_disp: bool,
    /// Interpolator only: the root holds still for this long first (the hop's `_a` item).
    pub lead: f32,
    /// The move ends in a free hang whatever the wall below (the hop, 0xDDAB00 sets LedgeHangType 1).
    pub end_free: bool,
    /// The move ends in a wall hang (free → wall switch, wall receptions).
    pub end_wall: bool,
    /// The move ends standing on top (knee / waist arrivals: Ledge SubState 4 → pull-up → Ground).
    pub end_stand: bool,
    /// Hands and wall normal once the move ends.
    pub hand_l: Vec3,
    pub hand_r: Vec3,
    pub normal: Vec3,
}

impl LedgeMove {
    pub fn duration(&self) -> f32 {
        self.durations.iter().sum::<f32>().max(1e-3)
    }

    /// Item playing at the current time and its phase.
    pub fn current(&self) -> Option<(ActionBlend, f32)> {
        let mut t0 = 0.0;
        for (i, a) in self.seq.iter().enumerate() {
            let Some(a) = a else { continue };
            let d = self.durations[i];
            if self.t < t0 + d || i == self.seq.len() - 1 || self.seq[i + 1..].iter().all(|x| x.is_none()) {
                return Some((*a, ((self.t - t0) / d.max(1e-4)).clamp(0.0, 1.0)));
            }
            t0 += d;
        }
        None
    }

    /// Summed displacement of the sequence up to time `t` (animation space).
    fn disp(&self, t: f32) -> [f32; 3] {
        let mut o = [0.0f32; 3];
        let mut t0 = 0.0;
        for (i, a) in self.seq.iter().enumerate() {
            let Some(a) = a else { continue };
            let d = self.durations[i];
            let ph = ((t - t0) / d.max(1e-4)).clamp(0.0, 1.0);
            let v = a.disp(ph);
            for k in 0..3 {
                o[k] += v[k];
            }
            t0 += d;
        }
        o
    }

    /// Advance; returns the root position and whether the move ended.
    pub fn advance(&mut self, dt: f32) -> (Vec3, bool) {
        let total = self.duration();
        self.t = (self.t + dt).min(total);
        let s = self.t / total;
        let p = if self.follow_disp {
            let to_world = |d: [f32; 3]| right_of(self.facing_from) * d[0] + self.facing_from * d[1] + Vec3::Y * d[2];
            let end = self.from + to_world(self.disp(total));
            self.from + to_world(self.disp(self.t)) + (self.to - end) * s
        } else {
            let s = ((self.t - self.lead) / (total - self.lead).max(1e-4)).clamp(0.0, 1.0);
            self.from.lerp(self.to, s)
        };
        (p, self.t >= total)
    }

    /// Where the clips' own displacement ends (no correction).
    pub fn natural_end(&self) -> Vec3 {
        let d = self.disp(self.duration());
        self.from + right_of(self.facing_from) * d[0] + self.facing_from * d[1] + Vec3::Y * d[2]
    }

    /// Facing during the move (turned from the old wall to the new one).
    pub fn facing(&self) -> Vec3 {
        let s = (self.t / self.duration()).clamp(0.0, 1.0);
        self.facing_from.lerp(self.facing_to, s).normalize_or(self.facing_to)
    }
}

fn single(id: u32, item: usize) -> Option<ActionBlend> {
    jump_blend::action_items(id).filter(|i| i.len() > item).map(|_| ActionBlend::new(id, item, &[1.0]))
}

fn seq_durations(seq: &[Option<ActionBlend>; 4]) -> [f32; 4] {
    let mut d = [0.0; 4];
    for (i, a) in seq.iter().enumerate() {
        d[i] = a.map(|a| a.duration()).unwrap_or(0.0);
    }
    d
}

/// `HumanLedge__FindCornerEdges` 0xDD0600 (same-height row) + `HumanLedge__TryTurnCorner` 0xDD3BB0.
/// `right` = moving right. Probe: inner = hands mid + 0.6·move − 0.6·facing; outer = hands mid +
/// 0.3·facing + 0.2·move; radius 0.25, vertical 0.4, edges whose wall the character would face within 50°.
pub fn try_corner(
    inner: bool,
    right: bool,
    hand_l: Vec3,
    hand_r: Vec3,
    n: Vec3,
    root: Vec3,
    hang: LedgeHangType,
    guidance: &GuidanceWorld,
    collision: &CollisionWorld,
) -> Option<LedgeMove> {
    let facing = -Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
    let side = if right { right_of(facing) } else { -right_of(facing) };
    let mut base = (hand_l + hand_r) * 0.5;
    base.y = hand_l.y.min(hand_r.y);
    let (p, new_facing) = if inner {
        (base + side * 0.6 - facing * 0.6, side)
    } else {
        (base + facing * 0.3 + side * 0.2, -side)
    };
    let hit = guidance.probe(p, 0.25, 0.4, Some(new_facing), 50f32.to_radians())?;
    let nn = hit.wall_normal;
    let new_hang = hang_type_at(hit.point, nn, collision);
    // a wall hang needs foot holds on the new wall (sub_B16130)
    if hang == LedgeHangType::Wall && new_hang != LedgeHangType::Wall {
        return None;
    }
    // hands: both on the corner point during the turn; SecondHandGrab (state 17, ±0.25 m) re-spreads them
    // (hypothesis: to the normal hand spacing around the point)
    let r = right_of(-nn);
    let (hl, hr) = (hit.point - r * HAND_SPACING * 0.5, hit.point + r * HAND_SPACING * 0.5);
    let to = hang_root(hl, hr, nn, new_hang);
    if !collision.capsule_fits(to + Vec3::Y * 0.05) {
        return None;
    }
    let col = (right as usize) * 2 + (!inner) as usize;
    let id = CORNER_ACTIONS[(hang == LedgeHangType::Wall) as usize][col];
    let seq = if hang == LedgeHangType::Wall { [single(id, 0), single(id, 1), None, None] } else { [single(id, 0), None, None, None] };
    let durations = seq_durations(&seq);
    let durations = if durations.iter().sum::<f32>() > 0.0 { durations } else { [CORNER_FALLBACK_TIME, 0.0, 0.0, 0.0] };
    Some(LedgeMove {
        kind: MoveKind::Corner { inner },
        seq,
        durations,
        t: 0.0,
        from: root,
        to,
        facing_from: facing,
        facing_to: -Vec3::new(nn.x, 0.0, nn.z).normalize_or_zero(),
        follow_disp: false,
        lead: 0.0,
        end_free: false,
        end_wall: false,
        end_stand: false,
        hand_l: hl,
        hand_r: hr,
        normal: nn,
    })
}

/// `HumanLedge__TrySideJumpToLedge` 0xDDD490 (hang → hang, table types 1 / 2): from the hands' midpoint
/// (highest hand) + 0.9·move, search up to 1.6 m further at hand-height offsets 0, +0.6, −0.6 (radius 0.35,
/// vertical 0.3) for an edge on the same side of the wall. `long` = distance / 1.6 ≥ 0.5.
pub fn try_side_jump(
    right: bool,
    hand_l: Vec3,
    hand_r: Vec3,
    n: Vec3,
    root: Vec3,
    hang: LedgeHangType,
    guidance: &GuidanceWorld,
    collision: &CollisionWorld,
) -> Option<LedgeMove> {
    let facing = -Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
    let side = if right { right_of(facing) } else { -right_of(facing) };
    let mut base = (hand_l + hand_r) * 0.5;
    base.y = hand_l.y.max(hand_r.y);
    let start = base + side * 0.9;
    for dz in [0.0f32, 0.6, -0.6] {
        let mut s = 0.0;
        while s <= 1.6 {
            let p = start + side * s + Vec3::Y * dz;
            if let Some(hit) = guidance.probe(p, 0.35, 0.3, Some(facing), 45f32.to_radians()) {
                // must be a different stretch of edge than the one we hang from (the shimmy would reach it)
                let along = (hit.point - base).dot(side);
                if along > 0.9 && guidance.on_edge(base + side * (along * 0.5), n, 0.1).is_none() {
                    let nn = hit.wall_normal;
                    let r = right_of(-nn);
                    // both hands on the new edge (sub_B153C0 finds a hand pair): the first hand lands on the
                    // nearest point, the other one HAND_SPACING further along the move
                    let c = hit.point + side * HAND_SPACING * 0.5;
                    let (hl, hr) = (c - r * HAND_SPACING * 0.5, c + r * HAND_SPACING * 0.5);
                    let new_hang = hang_type_at(c, nn, collision);
                    let to = hang_root(hl, hr, nn, new_hang);
                    if !collision.capsule_fits(to + Vec3::Y * 0.05) {
                        return None;
                    }
                    let dist = Vec2::new(hit.point.x - start.x, hit.point.z - start.z).length();
                    let long = (dist / 1.6).clamp(0.0, 1.0) >= 0.5;
                    let ty = if new_hang == LedgeHangType::Wall { 1 } else { 2 };
                    let entry = LEDGE_JUMP_TABLE[(hang == LedgeHangType::Free) as usize][(ty * 2 + long as usize) * 4 + 2 + right as usize];
                    let seq = [single(entry.1[0], 0), single(entry.1[1], 0), single(entry.1[2], 0), None];
                    let durations = seq_durations(&seq);
                    let found = durations.iter().sum::<f32>() > 0.0;
                    return Some(LedgeMove {
                        kind: MoveKind::SideJump { long },
                        seq,
                        durations: if found { durations } else { [SIDE_JUMP_FALLBACK_TIME, 0.0, 0.0, 0.0] },
                        t: 0.0,
                        from: root,
                        to,
                        facing_from: facing,
                        facing_to: -Vec3::new(nn.x, 0.0, nn.z).normalize_or_zero(),
                        follow_disp: found,
                        lead: 0.0,
                        end_free: false,
                        end_wall: false,
                        end_stand: false,
                        hand_l: hl,
                        hand_r: hr,
                        normal: nn,
                    });
                }
            }
            s += 0.1;
        }
    }
    None
}

/// `HumanLedge__TryWallJumpUp` 0xDD62A0 (wall hang, up): an edge above the reach of a hand step, then the
/// blended hop (0xDDCE40): weights over `swingback_up_{min,max}_{200,300}` by v = clamp(targetZ − rootZ −
/// 2.0) and h = clamp(horizontal distance of the hands). (hypothesis) the search: edges 1.3–1.9 m above the
/// hands within 0.6 m horizontally (the box query's arguments are not fully recovered).
pub fn try_hop_up(hand_l: Vec3, hand_r: Vec3, n: Vec3, root: Vec3, hang: LedgeHangType, guidance: &GuidanceWorld, _collision: &CollisionWorld) -> Option<LedgeMove> {
    if hang != LedgeHangType::Wall {
        return None;
    }
    let facing = -Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
    let mid = (hand_l + hand_r) * 0.5;
    for k in [1.3f32, 1.6, 1.9] {
        let Some(hit) = guidance.probe(mid + Vec3::Y * k, 0.6, 0.15, Some(facing), 45f32.to_radians()) else { continue };
        let dy = hit.point.y - mid.y;
        if dy <= VSTEP_MAX {
            continue;
        }
        let nn = hit.wall_normal;
        let r = right_of(-nn);
        let (hl, hr) = (hit.point - r * HAND_SPACING * 0.5, hit.point + r * HAND_SPACING * 0.5);
        let to = hang_root(hl, hr, nn, LedgeHangType::Free);
        let v = (hit.point.y - root.y - 2.0).clamp(0.0, 1.0);
        let h = Vec2::new(hit.point.x - mid.x, hit.point.z - mid.z).length().clamp(0.0, 1.0);
        let w = [(1.0 - h) * (1.0 - v), (1.0 - h) * v, h * (1.0 - v), h * v];
        let a = jump_blend::action_items(HOP_UP).map(|_| ActionBlend::new(HOP_UP, 0, &w));
        let b = jump_blend::action_items(HOP_UP_B).map(|_| ActionBlend::new(HOP_UP_B, 0, &w));
        let (da, db) = (a.map(|a| a.duration()).unwrap_or(0.0), b.map(|b| b.duration()).unwrap_or(0.0));
        let (da, db) = if da + db > 0.0 { (da, db) } else { (0.0, JUMP_UP_TIME) };
        return Some(LedgeMove {
            kind: MoveKind::HopUp,
            seq: [a, b, None, None],
            durations: [da, db, 0.0, 0.0],
            t: 0.0,
            from: root,
            to,
            facing_from: facing,
            facing_to: -Vec3::new(nn.x, 0.0, nn.z).normalize_or_zero(),
            // `_a` plays in place, `_b` moves the root with the interpolator (0xDDAB00 → sub_711130)
            follow_disp: false,
            lead: da,
            end_free: true,
            end_wall: false,
            end_stand: false,
            hand_l: hl,
            hand_r: hr,
            normal: nn,
        });
    }
    None
}

// ---------------------------------------------------------------- jumps into a hang

/// Pull-up chain pieces: hangwaist → hangknee (`xx_h_hangwaist_tr_hangknee_footl`), hangknee → wait.
pub const ACT_WAIST_TO_KNEE: u32 = 0x0127_199E;
pub const ACT_KNEE_TO_WAIT: u32 = 0x0106_C58B;
/// Running jump onto a wall-hang ledge (target type 0x40): `air_surface_tr_hangwall_reception_{straight,
/// 30_out,45_in}_{min,max}` then `hangwall_reception_*_{a,b}` (3 items × 6 clips; 0xE07D00 → 0xE02BA0).
pub const RECEPTION_SURFACE_WALL: u32 = 0x011F_F16A;
/// Running jump onto a free-hang ledge (type 0x80): the swing cycle (0xE07D00, Ledge SubState 8).
pub const SWING_RECEPTION: u32 = 0x023E_0C60;

/// The bands of `Human__SetupJumpToHandTarget` 0xB21DA0 (standing straight jump at a hand target) by the
/// hand height above the feet `dz`: flight action, its 2-clip blend weight, the root offset from the hand
/// point (out along the wall normal, down), target flags, the arrival reception and how it ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HangJumpIn {
    pub flight: u32,
    pub b: f32,
    pub out: f32,
    pub down: f32,
    pub flags: u32,
    pub reception: u32,
    pub end: HangEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HangEnd {
    /// Hang (wall / free / wall-free) after the reception.
    Hang(LedgeHangType),
    /// Knee height: the reception then hangknee → wait (Ledge SubState 4, pull-up).
    StandFromKnee,
    /// Waist height: the reception, hangwaist → hangknee, hangknee → wait.
    StandFromWaist,
}

/// 0xB21DA0 bands (ground variant: the playing action is the straight-jump impulse; the `beam_*` variants and
/// the ≤ 0.7 m `collide_full_*_to_freestep` step-up → NarrowObject are not used by the port). `wall` = the
/// target's sub-type is 8 (hypothesis: a wall below the edge, i.e. a wall hang).
pub fn hang_jump_in(dz: f32, wall: bool) -> Option<HangJumpIn> {
    let c = |x: f32| x.clamp(0.0, 1.0);
    Some(if dz < 0.7 {
        return None;
    } else if dz < 1.5 {
        HangJumpIn { flight: 0x0129_0ECF, b: c((dz - 0.7) / 0.8), out: 0.5, down: 0.0, flags: 4, reception: 0x0129_0ED0, end: HangEnd::StandFromKnee }
    } else if dz < 2.0 {
        HangJumpIn { flight: 0x0127_2A69, b: c((dz - 1.5) * 2.0), out: 0.5, down: 0.0, flags: 4, reception: 0x0127_2A6A, end: HangEnd::StandFromKnee }
    } else if dz < 2.5 {
        if wall {
            HangJumpIn { flight: 0x0127_1631, b: c((dz - 2.0) * 2.0), out: 0.5, down: 1.1, flags: 0x40, reception: 0x0127_1632, end: HangEnd::Hang(LedgeHangType::Wall) }
        } else {
            HangJumpIn { flight: 0x0127_1639, b: c((dz - 2.0) * 2.0), out: 0.5, down: 1.0, flags: 8, reception: 0x0127_163A, end: HangEnd::StandFromWaist }
        }
    } else if wall {
        HangJumpIn { flight: 0x0121_A8B1, b: c((dz - 2.5) * 2.0), out: 0.5, down: 2.4, flags: 0x80, reception: 0x0121_B072, end: HangEnd::Hang(LedgeHangType::Free) }
    } else {
        HangJumpIn { flight: 0x0121_A598, b: c((dz - 2.5) * 2.0), out: 0.0, down: 2.4, flags: 0x80, reception: 0x0127_23A5, end: HangEnd::Hang(LedgeHangType::Free) }
    })
}

/// How a jump at a ledge is received when it arrives (CheckJumpTargetArrival 0xE07D00).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LedgeArrival {
    /// From the standing straight jump (0xB21DA0).
    Straight(HangJumpIn),
    /// From a running jump (0xB20200) onto a ledge: wall reception (type 0x40) or the swing (0x80).
    Surface { free: bool },
}

/// The Ledge context's entry move for an arrival: the reception actions while the root goes from `from` to
/// the hang root (or onto the top for knee / waist heights), following the clips' displacement plus a
/// correction (receptions are FROMANIM; the game interpolates the root over the action, 0xE07D00 →
/// sub_711130).
pub fn arrival_move(arr: LedgeArrival, from: Vec3, hand_l: Vec3, hand_r: Vec3, n: Vec3, collision: &CollisionWorld) -> LedgeMove {
    let facing = -Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
    let mid = (hand_l + hand_r) * 0.5;
    let top = Vec3::new(mid.x, mid.y, mid.z) - n * PULLUP_IN;
    let item = |id: u32, i: usize, w: &[f32]| jump_blend::action_items(id).filter(|it| it.len() > i).map(|_| ActionBlend::new(id, i, w));
    let (seq, to, end_free, end_stand) = match arr {
        LedgeArrival::Straight(j) => {
            let w = [1.0 - j.b, j.b];
            match j.end {
                HangEnd::Hang(h) => {
                    let to = hang_root(hand_l, hand_r, n, if h == LedgeHangType::Free || hang_type_at(mid, n, collision) == LedgeHangType::Free { LedgeHangType::Free } else { LedgeHangType::Wall });
                    ([item(j.reception, 0, &w), item(j.reception, 1, &w), None, None], to, h == LedgeHangType::Free, false)
                }
                HangEnd::StandFromKnee => ([item(j.reception, 0, &w), item(j.reception, 1, &w), item(ACT_KNEE_TO_WAIT, 0, &[1.0]), None], top, false, true),
                HangEnd::StandFromWaist => (
                    [item(j.reception, 0, &w), item(j.reception, 1, &w), item(ACT_WAIST_TO_KNEE, 0, &[1.0]), item(ACT_KNEE_TO_WAIT, 0, &[1.0])],
                    top,
                    false,
                    true,
                ),
            }
        }
        LedgeArrival::Surface { free: false } => {
            // 0xE02790 weights the 6 clips by the wall angle (straight / 30 out / 45 in) and min / max;
            // the port's ledges are straight; (hypothesis) min
            let w = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
            let to = hang_root(hand_l, hand_r, n, LedgeHangType::Wall);
            ([item(RECEPTION_SURFACE_WALL, 0, &w), item(RECEPTION_SURFACE_WALL, 1, &w), item(RECEPTION_SURFACE_WALL, 2, &w), None], to, false, false)
        }
        LedgeArrival::Surface { free: true } => {
            // the swing: front up, front down (PORT: one swing, no SwingStrength decay)
            let to = hang_root(hand_l, hand_r, n, LedgeHangType::Free);
            ([item(SWING_RECEPTION, 0, &[1.0]), item(SWING_RECEPTION, 1, &[1.0]), None, None], to, true, false)
        }
    };
    let durations = seq_durations(&seq);
    let found = durations.iter().sum::<f32>() > 0.0;
    LedgeMove {
        kind: MoveKind::Arrival,
        seq,
        durations: if found { durations } else { [GRAB_TIME, 0.0, 0.0, 0.0] },
        t: 0.0,
        from,
        to,
        facing_from: facing,
        facing_to: facing,
        follow_disp: found,
        lead: 0.0,
        end_free,
        end_wall: false,
        end_stand,
        hand_l,
        hand_r,
        normal: n,
    }
}

// ---------------------------------------------------------------- pull-up

/// Pull-up actions (Pullup_Start 0xDDBE80): wall hang `xx_h_hangwall{,_45_in,_30_out}_tr_hangknee_footl_{a,b}`;
/// free hang `xx_h_hangfree_tr_hangwaist_{a,b}`, then hangwaist → hangknee; both end hangknee → wait.
pub const ACT_PULLUP_WALL: u32 = 0x0106_D2C5;
pub const ACT_PULLUP_FREE: u32 = 0x0127_19F2;

/// An item with all weight on its first clip (the straight variant), sized to the item's clip count.
fn first_clip(id: u32, item: usize) -> Option<ActionBlend> {
    let n = jump_blend::action_items(id)?.get(item)?.len();
    let mut w = vec![0.0; n.max(1)];
    w[0] = 1.0;
    Some(ActionBlend::new(id, item, &w))
}

/// The pull-up onto the top: the root follows the clips' own displacement (FROMANIM: up first, then in over
/// the lip) and is corrected onto the stand point 0.5 m inside the edge (Pullup_Start 0xDDBE80). A straight
/// line from the hang to that point cuts through the wall. Returns the move and, for the free hang (5 clips),
/// the queued second part.
pub fn pullup_move(hang: LedgeHangType, from: Vec3, hand_l: Vec3, hand_r: Vec3, n: Vec3, top_feet: Vec3) -> (LedgeMove, Option<LedgeMove>) {
    let facing = -Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
    let mk = |seq: [Option<ActionBlend>; 4], from: Vec3, to: Vec3, end_stand: bool| {
        let durations = seq_durations(&seq);
        let found = durations.iter().sum::<f32>() > 0.0;
        LedgeMove {
            kind: MoveKind::Pullup,
            seq,
            durations: if found { durations } else { [GRAB_TIME, 0.0, 0.0, 0.0] },
            t: 0.0,
            from,
            to,
            facing_from: facing,
            facing_to: facing,
            follow_disp: found,
            lead: 0.0,
            end_free: false,
            end_wall: false,
            end_stand,
            hand_l,
            hand_r,
            normal: n,
        }
    };
    let knee_to_wait = [first_clip(ACT_KNEE_TO_WAIT, 0), first_clip(ACT_KNEE_TO_WAIT, 1)];
    match hang {
        LedgeHangType::Wall => (
            mk([first_clip(ACT_PULLUP_WALL, 0), first_clip(ACT_PULLUP_WALL, 1), knee_to_wait[0], knee_to_wait[1]], from, top_feet, true),
            None,
        ),
        LedgeHangType::Free => {
            let mut a = mk([first_clip(ACT_PULLUP_FREE, 0), first_clip(ACT_PULLUP_FREE, 1), first_clip(ACT_WAIST_TO_KNEE, 0), None], from, from, false);
            // first part: the clips' path, corrected only by the second part
            a.to = a.natural_end();
            let b = mk([knee_to_wait[0], knee_to_wait[1], None, None], a.to, top_feet, true);
            (a, Some(b))
        }
    }
}

// ---------------------------------------------------------------- hang-type switch

/// `HumanLedge__TrySwitchHangType` 0xDE1060 actions. Free → wall: [left, other] = `hangfree_tr_hangwall_{left,
/// right}` (up / down use the right one). Wall → free: [left, right, up, down] = `hangwall_tr_hangfree_{left,
/// right,up_left,down_left}`.
pub const TO_WALL: [u32; 2] = [0x01C3_2562, 0x01C3_2563];
pub const TO_FREE: [u32; 4] = [0x01C3_14BD, 0x01C3_14BE, 0x01C3_217A, 0x01C3_217C];

/// Ledge stick direction as the exe numbers it (QuantizeStickDirection 0xDD1920): 0 up, 1 down, 2 left, 3 right.
pub fn switch_move(dir: u8, to_wall: bool, from: Vec3, hand_l: Vec3, hand_r: Vec3, n: Vec3) -> LedgeMove {
    let id = if to_wall { TO_WALL[(dir != 2) as usize] } else { TO_FREE[match dir { 2 => 0, 3 => 1, 0 => 2, _ => 3 }] };
    let a = single(id, 0);
    let d = a.map(|a| a.duration()).unwrap_or(0.0);
    let facing = -Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
    let hang = if to_wall { LedgeHangType::Wall } else { LedgeHangType::Free };
    LedgeMove {
        kind: MoveKind::SwitchHang { to_wall },
        seq: [a, None, None, None],
        durations: [if d > 0.0 { d } else { SHIMMY_OPEN_TIME }, 0.0, 0.0, 0.0],
        t: 0.0,
        from,
        to: hang_root(hand_l, hand_r, n, hang),
        facing_from: facing,
        facing_to: facing,
        // the root interpolator over the action (0xDE1060 → sub_711130)
        follow_disp: false,
        lead: 0.0,
        end_free: !to_wall,
        end_wall: to_wall,
        end_stand: false,
        hand_l,
        hand_r,
        normal: n,
    }
}

// ---------------------------------------------------------------- pull-down (ground → hang)

/// Pull-down table 0x1A2C3F0 (index side + 8·type; +4 = the descent action): the ground types have front
/// entries only. [EdgeStop (0), Wait (1)] orientation: `ledge_stop_start_footl_pulldown_front_orientation`,
/// `ledge_lookdown_front_pulldown_front_orientation`.
pub const PULLDOWN_ORIENT: [u32; 2] = [0xB5EB_D80B, 0x06E8_BD80];
/// Descent (types 0 and 1): `ledge_pulldown_soft_front`.
pub const PULLDOWN_DESCENT: u32 = 0x082F_8C53;
/// Reception (0xDDE980 stage 2): wall `pulldown_soft_to_hangwall_{straight,30_out,45_in}_a` (115916161), then its
/// transition action `_b` + `_tr` (0x082F820C); free `pulldown_soft_front_to_hangfree_a` (137338683), then
/// 0x082F9F3C (`_b` + `_tr`).
pub const PULLDOWN_WALL: [u32; 2] = [0x06E8_BD81, 0x082F_820C];
pub const PULLDOWN_FREE: [u32; 2] = [0x082F_9F3B, 0x082F_9F3C];

/// `HumanLedge__PullDown_Enter` 0xDDE4D0 + `HumanLedge__PullDown_Update` 0xDDE980 (front side).
/// `p` = the edge point, `n` = the edge's outward normal (the character faces along it, guard 0xD9D6C0),
/// `wait` = type 1 (from Movement) instead of 0 (from the ledge stop).
/// 1. Orientation: the type's orientation action, root → p + 0.5·n, facing −n.
/// 2. Descent: hands found on the edge at p ± 0.25·side (else the game releases into InAir), the descent
///    action, root → p + 1.0·n − 0.8 m.
/// 3. Reception: foot support (`sub_B16130`) → wall reception (angle blend, straight here) to the wall-hang
///    root, else the free reception to the free-hang root.
pub fn pulldown(p: Vec3, n: Vec3, from: Vec3, wait: bool, guidance: &GuidanceWorld, collision: &CollisionWorld) -> Option<[LedgeMove; 3]> {
    let n = Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
    let facing_out = n;
    let facing_in = -n;
    let r = right_of(facing_in);
    let hl = guidance.on_edge(p - r * 0.25, n, 0.3)?.point;
    let hr = guidance.on_edge(p + r * 0.25, n, 0.3)?.point;
    let mid = (hl + hr) * 0.5;
    let mk = |kind: u8, seq: [Option<ActionBlend>; 4], from: Vec3, to: Vec3, ff: Vec3, ft: Vec3, end_wall: bool, end_free: bool| {
        let durations = seq_durations(&seq);
        let found = durations.iter().sum::<f32>() > 0.0;
        LedgeMove {
            kind: MoveKind::PullDown { stage: kind },
            seq,
            durations: if found { durations } else { [GRAB_TIME, 0.0, 0.0, 0.0] },
            t: 0.0,
            from,
            to,
            facing_from: ff,
            facing_to: ft,
            follow_disp: false,
            lead: 0.0,
            end_free,
            end_wall,
            end_stand: false,
            hand_l: hl,
            hand_r: hr,
            normal: n,
        }
    };
    let item = |id: u32, i: usize, w: &[f32]| jump_blend::action_items(id).filter(|it| it.len() > i).map(|_| ActionBlend::new(id, i, w));
    let p1 = Vec3::new(mid.x, mid.y, mid.z) + n * 0.5;
    let orient = mk(1, [single(PULLDOWN_ORIENT[wait as usize], 0), None, None, None], from, p1, facing_out, facing_in, false, false);
    let p2 = mid + n * 1.0 - Vec3::Y * 0.8;
    let descent = mk(2, [single(PULLDOWN_DESCENT, 0), None, None, None], p1, p2, facing_in, facing_in, false, false);
    let wall = hang_type_at(mid, n, collision) == LedgeHangType::Wall;
    let straight = [1.0, 0.0, 0.0];
    let reception = if wall {
        mk(
            3,
            [item(PULLDOWN_WALL[0], 0, &straight), item(PULLDOWN_WALL[1], 0, &straight), item(PULLDOWN_WALL[1], 1, &straight), None],
            p2,
            hang_root(hl, hr, n, LedgeHangType::Wall),
            facing_in,
            facing_in,
            true,
            false,
        )
    } else {
        mk(
            3,
            [single(PULLDOWN_FREE[0], 0), single(PULLDOWN_FREE[1], 0), single(PULLDOWN_FREE[1], 1), None],
            p2,
            hang_root(hl, hr, n, LedgeHangType::Free),
            facing_in,
            facing_in,
            false,
            true,
        )
    };
    Some([orient, descent, reception])
}
