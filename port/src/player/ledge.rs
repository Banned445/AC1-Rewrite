//! Ledge context (`HumanLedge`, ActorContextID 9) â€” RE/03 Â§7.
//!
//! - Hangs by two hand contacts on guidance edges. Wall hang: root 1.1 m below the hands and 0.5 m
//!   out from the wall; free hang: root 2.4 m below (0xDD6730). Hang type re-evaluated after each
//!   step (TrySwitchHangType 0xDE1060).
//! - All movement is discrete: pick new hand targets, then interpolate the root over the move's
//!   animation length (PLACEHOLDER durations until clips are decoded). SubState HandPlacement (3)
//!   while a step runs, Movement (1) otherwise.
//! - Shimmy (ProbeLateral 0xDD9640 / StartHandStep 0xDDE0C0): alternating hands; free-space sphere
//!   sweep; step = min(d âˆ’ 0.4, 1.0 âˆ’ spacing); blocked if d âˆ’ 0.15 < 0.7; min step 0.15; chain ends
//!   handled implicitly (no edge further on â†’ no step).
//! - Up/down: transition to Climb when wall holds exist (TryTransitionToClimb 0xDD46A0), else
//!   hand-over-hand step to a ledge 0.6-1.2 m away, else (up, wall hang) the hop up into a free hang
//!   (TryWallJumpUp 0xDD62A0), else (up) "blocked up" -> pull-up if there is standing space (CanPullup 0xDE2270).
//! - Left/right past the end of the edge (or blocked): inner corner, side jump, outer corner
//!   (`ledge_moves`, RE/03 §7.6b).
//! - Let go (Legs) â†’ InAir; lost ledge (HasLostLedge 0xDD20D0) â†’ InAir.

use bevy::prelude::*;

use super::air::{FallOrigin, InAirEntry};
use super::climb::{ClimbEntry, ClimbEntryType};
use super::ledge_moves::{self, LedgeMove};
use super::{
    heading_of, right_of, switch_context, ActorContextId, Body, HumanDataBundle, LimbTargets, Locomotion, Player,
    RootInterp, TransitionSetup,
};
use crate::collision::CollisionWorld;
use crate::guidance::GuidanceWorld;
use crate::input::PadInput;
use crate::tuning::*;

/// `HumanLedgeData::LedgeSubState` (values from the exe, desc 0x1994E38) â€” subset used here.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LedgeSubState {
    #[default]
    Entry = 0,
    Movement = 1,
    TurnCorner = 2,
    HandPlacement = 3,
    Pullup = 4,
    ReboundTransition = 5,
    HangWallReception = 6,
    HangFreeReception = 7,
    TransitionInFromClimb = 9,
    PullDown = 11,
    Grasp = 13,
    ParallelJump = 14,
}

/// `HumanLedgeData::LedgeHangType` (desc 0x1994E68). WallFree is treated as Free by the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LedgeHangType {
    #[default]
    Wall = 0,
    Free = 1,
}

#[derive(Clone, Copy, Debug)]
pub struct LedgeEntry {
    pub hand_l: Vec3,
    pub hand_r: Vec3,
    /// Wall normal (pointing out of the solid).
    pub normal: Vec3,
    pub from_feet: Vec3,
    pub sub_state: LedgeSubState,
    /// The reception after a jump at the ledge (`ledge_moves::arrival_move`); replaces the plain grab.
    pub entry_move: Option<LedgeMove>,
    /// Further moves played after `entry_move` (the pull-down's descent and reception).
    pub entry_rest: [Option<LedgeMove>; 2],
}

impl LedgeEntry {
    /// Hands centred on `mid`, `HAND_SPACING` apart along the edge, facing into the wall.
    pub fn at(mid: Vec3, normal: Vec3, from_feet: Vec3, sub_state: LedgeSubState) -> Self {
        let r = right_of(-normal);
        Self {
            hand_l: mid - r * HAND_SPACING * 0.5,
            hand_r: mid + r * HAND_SPACING * 0.5,
            normal,
            from_feet,
            sub_state,
            entry_move: None,
            entry_rest: [None, None],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum After {
    Hang,
    /// Second hand of a vertical hand-over-hand step still to move: (hand index, target).
    SecondHand(usize, Vec3),
    StandOnTop,
}

/// Runtime data of the Ledge context (subset of reflected HumanLedgeData, RE/07 / RE/03 Â§2.3).
#[derive(Debug, Default)]
pub struct HumanLedgeData {
    pub sub_state: LedgeSubState,
    pub hang_type: LedgeHangType,
    pub hand_l: Vec3,
    pub hand_r: Vec3,
    pub normal: Vec3,
    /// +0x85: shimmy alternation flag. false = lead hand reaches out next, true = trailing hand closes.
    pub alt_flag: bool,
    /// +0x746: stick up and nothing else possible â†’ pull-up allowed.
    pub blocked_up: bool,
    pub last_action: &'static str,
    /// Incremented whenever a discrete move starts (lets the animator restart one-shot clips).
    pub step_seq: u32,
    /// Running vertical hand step: (first hand was the left one, this is the second half).
    pub vstep: Option<(bool, bool)>,
    moves: Vec<RootInterp>,
    after: Option<After>,
    /// Running corner turn / ledge jump / hop up (`ledge_moves`).
    pub mv: Option<LedgeMove>,
    /// Moves queued after `mv` (pull-down stages); each starts from where the previous one ended.
    pub queue: Vec<LedgeMove>,
    /// Free hang forced by the hop (0xDDAB00) until the next move re-evaluates the wall below.
    /// PORT: the game switches back through TrySwitchHangType's free → wall anims (not ported).
    pub force_free: bool,
    /// The hang type has been evaluated for this hang (it then changes only through moves: switches,
    /// corners, jumps — 0xDE1060).
    pub hang_set: bool,
}

impl HumanLedgeData {
    pub fn enter(&mut self, e: LedgeEntry) {
        self.hand_l = e.hand_l;
        self.hand_r = e.hand_r;
        self.normal = e.normal;
        self.sub_state = e.sub_state;
        self.alt_flag = false; // EnterCommon 0xDE26D0 clears +0x85
        self.blocked_up = false;
        self.moves.clear();
        self.mv = None;
        self.hang_set = false;
        self.after = Some(After::Hang);
        self.last_action = "grab";
        self.step_seq += 1;
        self.queue = e.entry_rest.iter().flatten().copied().collect();
        if let Some(m) = e.entry_move {
            // the game's reception for the jump that arrived (0xE07D00), or the pull-down's first stage
            self.mv = Some(m);
            self.sub_state = LedgeSubState::HandPlacement;
        } else {
            // the root moves onto the hang pose (reception/grasp anims in the game)
            self.moves.push(RootInterp::new(e.from_feet, Vec3::NAN, GRAB_TIME));
        }
    }
}

impl HumanLedgeData {
    /// A discrete move (grab, step, pull-up) is running.
    pub fn moving(&self) -> bool {
        !self.moves.is_empty() || self.mv.is_some()
    }
}

fn hand_mid(d: &HumanLedgeData) -> Vec3 {
    (d.hand_l + d.hand_r) * 0.5
}

/// Foot supports for a hang at `mid` (`sub_DE0A60` from TrySwitchHangType 0xDE1060): two rays of 1.2 m toward
/// the wall, starting 1 m below the hands, 0.5 m back from the wall and 0.1 m to either side (collision layer
/// 43). Returns (left foot hits, right foot hits).
pub fn feet_on_wall(mid: Vec3, n: Vec3, collision: &CollisionWorld) -> (bool, bool) {
    let facing = -Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
    let r = right_of(facing);
    let start = mid - Vec3::Y - facing * 0.5;
    let hit = |o: Vec3| collision.sphere_free_distance(o, facing, 0.01, 1.2) < 1.2 - 1e-3;
    (hit(start - r * 0.1), hit(start + r * 0.1))
}

/// A wall under the hands within leg length (the free hang becomes WallFree: legs held off the wall).
pub fn wall_below_hands(mid: Vec3, n: Vec3, collision: &CollisionWorld) -> bool {
    [0.6f32, 1.2, 1.8].iter().any(|&d| collision.point_inside(mid - n * 0.15 - Vec3::Y * d))
}

/// Hang type a hang at `mid` gets: wall if a foot finds support (the free → wall rule of 0xDE1060).
pub fn hang_type_at(mid: Vec3, n: Vec3, collision: &CollisionWorld) -> LedgeHangType {
    let (a, b) = feet_on_wall(mid, n, collision);
    if a || b {
        LedgeHangType::Wall
    } else {
        LedgeHangType::Free
    }
}

/// The hang type a step to `mid` needs, if it differs from `cur` (0xDE1060): free → wall when a foot finds
/// support, wall → free unless both feet do.
fn needs_switch(cur: LedgeHangType, mid: Vec3, n: Vec3, collision: &CollisionWorld) -> Option<LedgeHangType> {
    let (a, b) = feet_on_wall(mid, n, collision);
    match cur {
        LedgeHangType::Free if a || b => Some(LedgeHangType::Wall),
        LedgeHangType::Wall if !(a && b) => Some(LedgeHangType::Free),
        _ => None,
    }
}

/// Root (feet) for a hang (ComputeRootFromHandTargets 0xDD6730).
pub fn hang_root(hand_l: Vec3, hand_r: Vec3, n: Vec3, hang: LedgeHangType) -> Vec3 {
    let mid = (hand_l + hand_r) * 0.5;
    match hang {
        LedgeHangType::Wall => {
            Vec3::new(mid.x, hand_l.y.min(hand_r.y) - WALL_HANG_DROP, mid.z) + n * WALL_HANG_OUT
        }
        LedgeHangType::Free => {
            Vec3::new(mid.x, hand_l.y.max(hand_r.y) - FREE_HANG_DROP, mid.z) + n * FREE_HANG_OUT
        }
    }
}

/// Root for a hang, with the WallFree case (LedgeHangType 2): a free hang with a wall under the hands plays
/// `xx_h_hangwallfree_wait`, whose hands are 0.46 m in front of the root (fingertips 0.52 m, 2.41 m up), so its
/// root sits 0.5 m out from the wall and 2.4 m below the hands, as the game's band offsets for the jump into it
/// (+0.5·n, −2.4 m, 0xB21DA0). `hangfree_wait` has the hands right above the root (FREE_HANG_OUT).
pub fn hang_root_at(hand_l: Vec3, hand_r: Vec3, n: Vec3, hang: LedgeHangType, collision: &CollisionWorld) -> Vec3 {
    let mid = (hand_l + hand_r) * 0.5;
    if hang == LedgeHangType::Free && wall_below_hands(mid, n, collision) {
        Vec3::new(mid.x, hand_l.y.max(hand_r.y) - FREE_HANG_DROP, mid.z) + n * WALLFREE_HANG_OUT
    } else {
        hang_root(hand_l, hand_r, n, hang)
    }
}

/// Ledge stick quantization (QuantizeStickDirection 0xDD1920): |a| â‰¤ 45Â° up, beyond 135Â° down,
/// otherwise left/right. `a` is measured from the character facing (into the wall).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgeDir {
    Up,
    Down,
    Left,
    Right,
}

pub fn quantize(stick: Vec3, facing: Vec3) -> LedgeDir {
    let r = right_of(facing);
    let a = stick.dot(r).atan2(stick.dot(facing));
    let deg = a.to_degrees();
    if deg.abs() <= 45.0 {
        LedgeDir::Up
    } else if deg.abs() >= 135.0 {
        LedgeDir::Down
    } else if deg > 0.0 {
        LedgeDir::Right
    } else {
        LedgeDir::Left
    }
}

fn start_move(d: &mut HumanLedgeData, mv: LedgeMove, what: &'static str) {
    d.mv = Some(mv);
    d.force_free = false;
    d.step_seq += 1;
    d.sub_state = LedgeSubState::HandPlacement;
    d.blocked_up = false;
    d.last_action = what;
}

/// Both foot holds 1.2 m below the hands exist â†’ the wall is climbable (TryTransitionToClimb).
fn climb_holds_below(guidance: &GuidanceWorld, hand_l: Vec3, hand_r: Vec3, n: Vec3) -> Option<(Vec3, Vec3)> {
    let drop = CLIMB_ROW * CLIMB_HAND_ROWS as f32;
    let fl = guidance.probe(hand_l - Vec3::Y * drop, CLIMB_PROBE_R, CLIMB_PROBE_VTOL * 0.5, Some(-n), 0.785)?;
    let fr = guidance.probe(hand_r - Vec3::Y * drop, CLIMB_PROBE_R, CLIMB_PROBE_VTOL * 0.5, Some(-n), 0.785)?;
    Some((fl.point, fr.point))
}

#[allow(clippy::too_many_arguments)]
pub fn update_ledge(
    time: Res<Time>,
    mut pad: ResMut<PadInput>,
    collision: Res<CollisionWorld>,
    guidance: Res<GuidanceWorld>,
    mut q: Query<(&mut Locomotion, &mut Body, &mut HumanDataBundle, &mut LimbTargets), With<Player>>,
) {
    let dt = time.delta_secs().min(1.0 / 20.0);
    for (mut loco, mut body, mut data, mut limbs) in &mut q {
        if loco.current != ActorContextId::Ledge {
            continue;
        }
        if loco.just_switched {
            loco.just_switched = false;
            limbs.feet = None;
            continue;
        }
        let d = &mut data.ledge;
        let n = d.normal;
        let facing = -Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
        body.heading = heading_of(facing);
        body.velocity = Vec3::ZERO;
        body.grounded = false;
        if !d.hang_set {
            d.hang_type = hang_type_at(hand_mid(d), n, &collision);
            d.hang_set = true;
        }
        // the pull-up clip carries the hands from the lip onto the top: no hand pinning (game: the
        // animation's contact tags release them)
        limbs.hands = (d.sub_state != LedgeSubState::Pullup).then_some((d.hand_l, d.hand_r));
        limbs.normal = n;
        limbs.transit = d.moves.first().map(|m| m.duration).unwrap_or(SHIMMY_OPEN_TIME) * 0.8;
        // the feet follow the hang clip (braced against the wall, or dangling); only the hands are pinned
        limbs.feet = None;

        // ---------------------------------------------------------------- corner / ledge jump / hop
        if let Some(mut mv) = d.mv {
            let (p, done) = mv.advance(dt);
            body.feet = p;
            body.heading = heading_of(mv.facing());
            limbs.hands = None;
            if done && !d.queue.is_empty() {
                // next queued stage (pull-down: orientation → descent → reception)
                let mut next = d.queue.remove(0);
                next.from = body.feet;
                d.mv = Some(next);
                continue;
            }
            if done && mv.end_stand {
                // knee / waist arrivals end standing on top (Ledge SubState 4 → pull-up). Game: NarrowObject
                // on the edge; PORT: Ground
                d.mv = None;
                limbs.hands = None;
                limbs.feet = None;
                body.grounded = true;
                switch_context(&mut loco, &mut data, TransitionSetup::ToMovement { landing: None });
                continue;
            }
            if done {
                d.hand_l = mv.hand_l;
                d.hand_r = mv.hand_r;
                d.normal = mv.normal;
                d.hang_type = if mv.end_free {
                    LedgeHangType::Free
                } else if mv.end_wall {
                    LedgeHangType::Wall
                } else {
                    hang_type_at(hand_mid(d), mv.normal, &collision)
                };
                d.hang_set = true;
                d.force_free = false;
                d.mv = None;
                d.sub_state = LedgeSubState::Movement;
                if !matches!(mv.kind, ledge_moves::MoveKind::SwitchHang { .. }) {
                    d.alt_flag = false;
                }
            } else {
                d.mv = Some(mv);
            }
            continue;
        }

        // ---------------------------------------------------------------- running move
        if let Some(m) = d.moves.first_mut() {
            if m.to.is_nan() {
                // deferred target (entry): the hang root for the current hands
                m.to = hang_root_at(d.hand_l, d.hand_r, n, d.hang_type, &collision);
            }
            let (p, _) = m.advance(dt);
            body.feet = p;
            if m.done() {
                d.moves.remove(0);
            }
            if !d.moves.is_empty() {
                continue;
            }
            match d.after.take() {
                Some(After::StandOnTop) => {
                    limbs.hands = None;
                    limbs.feet = None;
                    body.grounded = true;
                    // game: ends in NarrowObject (standing on the ledge edge, not ported yet) â†’ Ground
                    switch_context(&mut loco, &mut data, TransitionSetup::ToMovement { landing: None });
                    continue;
                }
                Some(After::SecondHand(i, target)) => {
                    // ContinuePendingVerticalStep 0xDCF3C0: the second hand follows next
                    if i == 0 { d.hand_l = target } else { d.hand_r = target }
                    let to = hang_root_at(d.hand_l, d.hand_r, n, d.hang_type, &collision);
                    d.vstep = Some((i == 1, true));
                    d.moves.push(RootInterp::new(body.feet, to, VSTEP_SECOND_TIME));
                    d.step_seq += 1;
                    d.after = Some(After::Hang);
                    continue;
                }
                _ => {}
            }
            d.sub_state = LedgeSubState::Movement;
        }

        // ---------------------------------------------------------------- checks (Movement state)
        let spacing = (d.hand_l - d.hand_r).length();
        let tol = LOST_LEDGE_R + spacing * 0.5;
        if guidance.on_edge(d.hand_l, n, tol).is_none() || guidance.on_edge(d.hand_r, n, tol).is_none() {
            let origin = if d.hang_type == LedgeHangType::Wall { FallOrigin::HangWall } else { FallOrigin::HangFree };
            limbs.hands = None;
            limbs.feet = None;
            switch_context(&mut loco, &mut data, TransitionSetup::ToInAir(InAirEntry::Fall { from: body.feet, velocity: Vec3::ZERO, origin, speed_param: 0.0 }));
            continue;
        }
        let dir = if pad.speed01 > 0.0 { Some(quantize(pad.dir, facing)) } else { None };
        if pad.jump_buffered() {
            pad.consume_jump();
            limbs.hands = None;
            limbs.feet = None;
            let entry = if pad.high_profile && dir == Some(LedgeDir::Down) {
                // back eject: jump away from the wall (hypothesis: high profile + Legs + stick away)
                InAirEntry::FreeJump { from: body.feet, dir: n, speed_param: 0.5 }
            } else {
                // let go (WantsLetGo 0xDCD4D0 â†’ LetGoToInAir)
                let origin = if d.hang_type == LedgeHangType::Wall { FallOrigin::HangWall } else { FallOrigin::HangFree };
                InAirEntry::Fall { from: body.feet, velocity: Vec3::ZERO, origin, speed_param: 0.0 }
            };
            switch_context(&mut loco, &mut data, TransitionSetup::ToInAir(entry));
            continue;
        }
        let Some(dir) = dir else {
            d.blocked_up = false;
            d.last_action = "idle";
            continue;
        };
        let right = right_of(facing);
        match dir {
            LedgeDir::Up | LedgeDir::Down => {
                let sign = if dir == LedgeDir::Up { 1.0 } else { -1.0 };
                // TryTransitionToClimb: wall holds for the feet (and, going up, a hold above the hands)
                if let Some((fl, fr)) = climb_holds_below(&guidance, d.hand_l, d.hand_r, n) {
                    let above = dir == LedgeDir::Down
                        || guidance.probe(hand_mid(d) + Vec3::Y * CLIMB_ROW, CLIMB_PROBE_R, CLIMB_PROBE_VTOL * 0.5, Some(facing), 0.785).is_some();
                    if above {
                        let entry = ClimbEntry {
                            entry_type: ClimbEntryType::FromLedge,
                            hand_l: d.hand_l,
                            hand_r: d.hand_r,
                            foot_l: fl,
                            foot_r: fr,
                            normal: n,
                            from_feet: body.feet,
                        };
                        switch_context(&mut loco, &mut data, TransitionSetup::ToClimb(entry));
                        continue;
                    }
                }
                // hand-over-hand step to a ledge 0.45â€“1.25 m above/below
                let mid = hand_mid(d);
                let mut target = None;
                for k in [0.6f32, 0.9, 1.2] {
                    if let Some(h) = guidance.probe(mid + Vec3::Y * k * sign, 0.3, 0.2, Some(facing), 0.785) {
                        let dy = (h.point.y - mid.y) * sign;
                        if (VSTEP_MIN..=VSTEP_MAX).contains(&dy) {
                            target = Some(h);
                            break;
                        }
                    }
                }
                if let Some(h) = target {
                    let tl = Vec3::new(d.hand_l.x, h.point.y, d.hand_l.z);
                    let tr = Vec3::new(d.hand_r.x, h.point.y, d.hand_r.z);
                    // the destination needs the other hang type: the switch replaces the hand step (0xDE29E0 order:
                    // TrySwitchHangType before StartHandStep)
                    if let Some(nt) = needs_switch(d.hang_type, (tl + tr) * 0.5, n, &collision) {
                        let mv = ledge_moves::switch_move(if sign > 0.0 { 0 } else { 1 }, nt == LedgeHangType::Wall, body.feet, tl, tr, n, &collision);
                        start_move(d, mv, if nt == LedgeHangType::Wall { "switch to wall hang" } else { "switch to free hang" });
                        continue;
                    }
                    // first hand moves now, the second one next (pending vertical step)
                    let first_left = !d.alt_flag;
                    d.alt_flag = !d.alt_flag;
                    if first_left { d.hand_l = tl } else { d.hand_r = tr }
                    let mid_root = (body.feet + hang_root_at(tl, tr, n, d.hang_type, &collision)) * 0.5;
                    d.vstep = Some((first_left, false));
                    let first_time = match (d.hang_type, sign > 0.0) {
                        (LedgeHangType::Wall, true) => VSTEP_TIME,
                        (LedgeHangType::Wall, false) => VSTEP_DOWN_TIME,
                        _ => VSTEP_SECOND_TIME,
                    };
                    d.moves.push(RootInterp::new(body.feet, mid_root, first_time));
                    d.step_seq += 1;
                    d.after = Some(if first_left { After::SecondHand(1, tr) } else { After::SecondHand(0, tl) });
                    d.sub_state = LedgeSubState::HandPlacement;
                    d.blocked_up = false;
                    d.last_action = if sign > 0.0 { "hand step up" } else { "hand step down" };
                    continue;
                }
                if dir == LedgeDir::Up {
                    // TryWallJumpUp 0xDD62A0: hop up to a ledge above the reach of a hand step (wall hang).
                    // (TryJumpUpToClimb 0xDD5E10, the jump up to climb holds, comes first in the game; the
                    // greybox has no climbable wall above a ledge, so it is not ported yet.)
                    if let Some(mv) = ledge_moves::try_hop_up(d.hand_l, d.hand_r, n, body.feet, d.hang_type, &guidance, &collision) {
                        start_move(d, mv, "hop up");
                        continue;
                    }
                    // nothing else possible: blocked up â†’ pull-up (CanPullup 0xDE2270). In the game the
                    // decision layer sends event 0; here holding up while blocked triggers it (hypothesis).
                    d.blocked_up = true;
                    let top = Vec3::new(mid.x, mid.y, mid.z) - n * PULLUP_IN;
                    let standable = collision
                        .ground_height(top + Vec3::Y * 0.05, 0.3)
                        .filter(|h| (h - mid.y).abs() < 0.25)
                        .is_some();
                    if standable && collision.capsule_fits(Vec3::new(top.x, mid.y, top.z)) {
                        // root: onto the top, 0.5 m in (Pullup_Start 0xDDBE80), along the pull-up clips' own
                        // displacement (up, then in over the lip) plus a correction onto that point
                        let top_feet = Vec3::new(top.x, mid.y, top.z);
                        let (mv, rest) = ledge_moves::pullup_move(d.hang_type, body.feet, d.hand_l, d.hand_r, n, top_feet);
                        start_move(d, mv, "pull-up");
                        d.queue = rest.into_iter().collect();
                        d.sub_state = LedgeSubState::Pullup;
                        continue;
                    }
                    d.last_action = "blocked up";
                } else {
                    d.last_action = "blocked down";
                }
            }
            LedgeDir::Left | LedgeDir::Right => {
                d.blocked_up = false;
                let dv = if dir == LedgeDir::Right { right } else { -right };
                // lead hand = hand on the move side; roles swap when the alternation flag is set
                let move_right_hand = (dir == LedgeDir::Right) != d.alt_flag;
                let (mover, other) = if move_right_hand { (d.hand_r, d.hand_l) } else { (d.hand_l, d.hand_r) };
                let target = if !d.alt_flag {
                    // reach step: free-space sweep from the lead hand, out from the wall and up
                    let origin = mover - dv * SHIMMY_SWEEP_R + n * SHIMMY_SWEEP_OUT + Vec3::Y * SHIMMY_SWEEP_UP;
                    let free = collision.sphere_free_distance(origin, dv, SHIMMY_SWEEP_R, SHIMMY_SWEEP_LEN);
                    if free - SHIMMY_SWEEP_R < SHIMMY_MIN_FREE {
                        // blocked: inner corner (0xDD0600 inner + 0xDD3BB0), else side jump, else blocked
                        let right_dir = dir == LedgeDir::Right;
                        if let Some(mv) = ledge_moves::try_corner(true, right_dir, d.hand_l, d.hand_r, n, body.feet, d.hang_type, &guidance, &collision) {
                            start_move(d, mv, "corner (inner)");
                        } else if let Some(mv) = ledge_moves::try_side_jump(right_dir, d.hand_l, d.hand_r, n, body.feet, d.hang_type, &guidance, &collision) {
                            start_move(d, mv, "side jump");
                        } else {
                            d.last_action = "shimmy blocked (obstacle / inner corner)";
                        }
                        continue;
                    }
                    let step = (free - SHIMMY_WALL_MARGIN).min(SHIMMY_MAX_SPACING - spacing);
                    // furthest point along the edge chain within `step` (implicit chain end)
                    let mut s = step;
                    let mut found = None;
                    while s >= SHIMMY_MIN_STEP {
                        if let Some(h) = guidance.on_edge(mover + dv * s, n, 0.06) {
                            found = Some(h.point);
                            break;
                        }
                        s -= 0.05;
                    }
                    found
                } else {
                    // closing step: trailing hand catches up to HAND_SPACING from the lead
                    guidance.on_edge(other - dv * HAND_SPACING, n, 0.1).map(|h| h.point)
                };
                let Some(t) = target else {
                    // end of the edge: inner corner, side jump, outer corner (Movement_ChooseAction 0xDE29E0 order)
                    let right_dir = dir == LedgeDir::Right;
                    let mv = ledge_moves::try_corner(true, right_dir, d.hand_l, d.hand_r, n, body.feet, d.hang_type, &guidance, &collision)
                        .map(|m| (m, "corner (inner)"))
                        .or_else(|| ledge_moves::try_side_jump(right_dir, d.hand_l, d.hand_r, n, body.feet, d.hang_type, &guidance, &collision).map(|m| (m, "side jump")))
                        .or_else(|| ledge_moves::try_corner(false, right_dir, d.hand_l, d.hand_r, n, body.feet, d.hang_type, &guidance, &collision).map(|m| (m, "corner (outer)")));
                    match mv {
                        Some((mv, what)) => start_move(d, mv, what),
                        None => d.last_action = "end of ledge",
                    }
                    continue;
                };
                let (nl, nr) = if move_right_hand { (d.hand_l, t) } else { (t, d.hand_r) };
                if let Some(nt) = needs_switch(d.hang_type, (nl + nr) * 0.5, n, &collision) {
                    let mv = ledge_moves::switch_move(if dir == LedgeDir::Right { 3 } else { 2 }, nt == LedgeHangType::Wall, body.feet, nl, nr, n, &collision);
                    // the switch carries this shimmy step's hand move: the alternation advances as for the step
                    d.alt_flag = !d.alt_flag;
                    start_move(d, mv, if nt == LedgeHangType::Wall { "switch to wall hang" } else { "switch to free hang" });
                    continue;
                }
                if move_right_hand { d.hand_r = t } else { d.hand_l = t }
                d.alt_flag = !d.alt_flag;
                d.force_free = false;
                let to = hang_root_at(d.hand_l, d.hand_r, n, d.hang_type, &collision);
                // after the toggle: alt_flag set = this was the lead hand's reach (open), clear = closing step
                d.moves.push(RootInterp::new(body.feet, to, if d.alt_flag { SHIMMY_OPEN_TIME } else { SHIMMY_CLOSE_TIME }));
                d.step_seq += 1;
                d.after = Some(After::Hang);
                d.sub_state = LedgeSubState::HandPlacement;
                d.last_action = if dir == LedgeDir::Right { "shimmy right" } else { "shimmy left" };
            }
        }
    }
}
