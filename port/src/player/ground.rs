//! Ground locomotion context (`HumanGround`, ActorContextID 4) — RE/02.
//!
//! - One speed parameter (0..1) split into bands Walk/Jog/Run/Sprint (GetSpeedBand 0xD807B0).
//! - Target = base + 0.25·stick, base 0 (low profile) / 0.5 (high profile) / 0.75 (sprint).
//! - Parameter rises at 1.0/s and falls through the deceleration ResponseCurve (0xDA0810, `move_blend`).
//! - Heading turns toward the wanted direction at 360°/s (0xD95290).
//! - Translation: root motion of the 17-clip locomotion blend (action 0x05923BDB, `move_blend`).
//! - Ground loss → InAir fall (0xD87720 / 0xD8C380); jump request → InAir jump-to-target.

use bevy::prelude::*;

use super::air::{FallOrigin, InAirEntry, Landing, LandingType};
use super::climb::{ClimbEntry, ClimbEntryType};

use super::jump_blend::{self, ActionBlend};
use super::move_blend::MoveBlend;
use super::targets::{edge_ahead, find_jump_target, JumpTarget};
use super::{
    heading_of, switch_context, ActorContextId, Body, HumanDataBundle, Locomotion, Player, SpawnPoint, TransitionSetup,
};
use crate::camera::CameraRig;
use crate::collision::CollisionWorld;
use crate::guidance::GuidanceWorld;
use crate::input::PadInput;
use crate::tuning::*;

/// `HumanGroundData::HumanGroundSubState` (values from the exe, desc 0x19961C8).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HumanGroundSubState {
    #[default]
    Movement = 0,
    Fight = 1,
    FreeRun = 2,
    OrientedMove = 3,
    Hurt = 4,
    ObstacleCollision = 5,
}

/// Speed band as returned by `IHumanGround::GetSpeedBand` (same order as AssassinAbilitySet::MaxSpeed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeedBand {
    None,
    Walk,
    Jog,
    Run,
    Sprint,
}

pub fn speed_band(param: f32) -> SpeedBand {
    if param <= 0.0 {
        SpeedBand::None
    } else if param <= BAND_WALK {
        SpeedBand::Walk
    } else if param <= BAND_JOG {
        SpeedBand::Jog
    } else if param <= BAND_RUN {
        SpeedBand::Run
    } else {
        SpeedBand::Sprint
    }
}

/// Runtime data of the ground context (subset of the reflected HumanGroundData, RE/07).
#[derive(Debug, Default)]
pub struct HumanGroundData {
    pub sub_state: HumanGroundSubState,
    /// HG+0x5E8: the 0..1 speed parameter.
    pub speed_param: f32,
    /// Turn attenuation factor (interpreter +0x10D0).
    pub turn_atten: f32,
    /// `Sprint` flag (HumanGroundData+0x123, name recovered by CRC32).
    pub sprint: bool,
    pub high_profile: bool,
    /// The landing / reception action playing after an InAir landing (0xE05940 / 0xE07D00): its root
    /// motion moves the character and input waits until it ends (its transitions lead back to the
    /// locomotion action 0x05923BDB or wait).
    pub oneshot: Option<GroundOneShot>,
    /// Played when `oneshot` ends (the run stop's settle into the wait).
    pub oneshot_next: Option<ActionBlend>,
    pub last_landing: Option<Landing>,
    /// Incremented on every landing (lets the animator play the landing clip once).
    pub landing_seq: u32,
    /// `HumanGround__UpdateMoveBlend` state: blend weights, timers, lean/bank, step cycle.
    pub blend: MoveBlend,
    /// LedgeStop sub-state (38): the edge it stopped at.
    pub ledge_stop: Option<LedgeStop>,
    /// PORT: no new ledge stop until the stick lets go or turns away from this edge normal (the game's event 69
    /// sender is not traced).
    pub ledge_stop_lock: Option<Vec3>,
}

/// `HumanGround__LedgeStop_Enter` 0xD93C60 / `HumanGround__LedgeStop_PlayEnd` 0xD7D9D0.
#[derive(Clone, Copy, Debug)]
pub struct LedgeStop {
    /// Edge report: point (+16) and outward normal (+32).
    pub point: Vec3,
    pub normal: Vec3,
    /// The end action is playing (the start action, 0x06E8BD7F, is done).
    pub ending: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct GroundOneShot {
    pub blend: ActionBlend,
    pub t: f32,
    pub duration: f32,
    /// Displacement already applied (animation space).
    pub applied: [f32; 3],
}

impl HumanGroundData {
    /// Play a ground one-shot action with its root motion (landings, receptions, ledge stop).
    pub fn play_oneshot(&mut self, b: ActionBlend) {
        self.landing_seq = self.landing_seq.wrapping_add(1);
        self.oneshot = Some(GroundOneShot { blend: b, t: 0.0, duration: b.duration(), applied: [0.0; 3] });
    }

    /// `TransitionSetupDataToMovement::Apply` (0xC80310), simplified.
    pub fn enter(&mut self, landing: Option<Landing>) {
        self.sub_state = HumanGroundSubState::Movement;
        self.ledge_stop = None;
        self.oneshot_next = None;
        if let Some(l) = landing {
            self.landing_seq = self.landing_seq.wrapping_add(1);
            self.oneshot = l.action.map(|b| GroundOneShot { blend: b, t: 0.0, duration: b.duration(), applied: [0.0; 3] });
            // The speed parameter HG+0x5E8 is not reset by OnEnterInit 0xDA7D20, so the landing's exit
            // into locomotion continues at the take-off speed. (hypothesis) heavy-damage landings stop.
            if l.kind == LandingType::HeavyDamage {
                self.speed_param = 0.0;
            }
            self.last_landing = Some(l);
        }
    }
}

fn angle_diff(a: f32, b: f32) -> f32 {
    let mut d = (a - b) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

#[allow(clippy::too_many_arguments)]
pub fn update_ground(
    time: Res<Time>,
    mut pad: ResMut<PadInput>,
    collision: Res<CollisionWorld>,
    guidance: Res<GuidanceWorld>,
    spawn: Res<SpawnPoint>,
    mut rig: ResMut<CameraRig>,
    mut q: Query<(&mut Locomotion, &mut Body, &mut HumanDataBundle), With<Player>>,
) {
    let dt = time.delta_secs().min(1.0 / 20.0);
    for (mut loco, mut body, mut data) in &mut q {
        let g = &mut data.ground;
        if loco.current != ActorContextId::Ground {
            continue;
        }
        if loco.just_switched {
            // AIActor::Update skips a context's first update after it was switched in.
            loco.just_switched = false;
            if let Some(l) = g.last_landing {
                // drop > 3 m: camera shake (drop − 3) / 7 (0xE05940)
                if l.total_drop > 3.0 {
                    rig.shake = ((l.total_drop - 3.0) / 7.0).min(1.0);
                }
                if l.kind == LandingType::Fatal {
                    // "desynchronisation": respawn
                    body.feet = spawn.0;
                    body.velocity = Vec3::ZERO;
                }
            }
            continue;
        }

        // ---------------------------------------------------------------- input → wanted motion
        let busy = g.oneshot.is_some();
        let moving = pad.speed01 > 0.0 && !busy;
        g.high_profile = pad.high_profile;
        g.sprint = pad.high_profile && pad.legs_held; // sprint = high profile + legs (RE/01 §6.2)
        g.sub_state = if g.sprint { HumanGroundSubState::FreeRun } else { HumanGroundSubState::Movement };

        // turn attenuation (interpreter 0xEE65A0): beyond 45° slow down; forced to 1 in low profile
        let want_heading = if moving { heading_of(pad.dir) } else { body.heading };
        let off = angle_diff(want_heading, body.heading).abs();
        let atten_target = if !g.high_profile || off <= TURN_ATTEN_START {
            1.0
        } else {
            (1.0 - (off - TURN_ATTEN_START) / TURN_ATTEN_RANGE).max(TURN_ATTEN_FLOOR)
        };
        g.turn_atten = if atten_target > g.turn_atten {
            (g.turn_atten + TURN_ATTEN_UP_RATE * dt).min(atten_target)
        } else {
            (g.turn_atten - TURN_ATTEN_DOWN_RATE * dt).max(atten_target)
        };

        // speed parameter: target = base + 0.25·stick (RE/02 §1)
        let base = if g.sprint {
            BASE_SPRINT
        } else if g.high_profile {
            BASE_HIGH_PROFILE
        } else {
            BASE_LOW_PROFILE
        };
        // the target follows the stick even while a landing action plays (MoveBlend's transition path keeps
        // ramping toward it, 0xDA0810)
        let target = if pad.speed01 > 0.0 { (base + STICK_SPAN * pad.speed01 * g.turn_atten).min(1.0) } else { 0.0 };
        // stick released (desired mode HG+0x5D8 = 0): the game leaves the Move state instead of decelerating
        // (the curve at HG+0x63C only runs while still moving toward a slower band):
        // - jog or faster (HG+0x5DC, speed > 0.25) → RunStop 0xD98E30 (guard 0xD7EC90): the run-stop action by the
        //   leading foot, its root motion, then its settle into the wait;
        // - walk band → Idle 0xD8B220 (guard 0xD7ED30): the wait with a 0.2 s blend, i.e. stopped at once.
        if pad.speed01 <= 0.0 && g.speed_param > 0.0 && g.oneshot.is_none() {
            if g.speed_param > 0.25 {
                let foot = (g.blend.foot != 0) as usize;
                if let Some(b) = jump_blend::action_items(jump_blend::RUN_STOP[foot]).map(|_| ActionBlend::new(jump_blend::RUN_STOP[foot], 0, &jump_blend::run_stop_weights(g.speed_param))) {
                    g.play_oneshot(b);
                    g.oneshot_next = jump_blend::action_items(jump_blend::RUN_STOP_TO_WAIT[foot]).map(|_| ActionBlend::new(jump_blend::RUN_STOP_TO_WAIT[foot], 0, &[1.0]));
                }
            }
            g.speed_param = 0.0;
            g.blend.speed_param = 0.0;
        }
        // moving again during the run stop: its transitions to walk / jog take over (PORT: the stop is cut)
        if pad.speed01 > 0.0 && g.oneshot.is_some_and(|o| jump_blend::RUN_STOP.contains(&o.blend.id) || jump_blend::RUN_STOP_TO_WAIT.contains(&o.blend.id)) {
            g.oneshot = None;
            g.oneshot_next = None;
        }
        // speed parameter, lean/bank and blend weights (MoveBlend 0xDA0810). The heading snapshot is the
        // heading before this frame's turn (HG+0x600, Movement_PreUpdate 0xD97E30).
        g.blend.speed_param = g.speed_param;
        g.blend.update_speed(target, dt);
        g.blend.update_angles(body.heading, moving.then_some(want_heading), None, false, dt);
        g.blend.update_weights(target, dt);
        g.speed_param = g.blend.speed_param;

        // heading: rotate toward wanted at the player turn rate (0xD95290)
        if moving {
            let d = angle_diff(want_heading, body.heading);
            let step = PLAYER_TURN_RATE * dt;
            body.heading += d.clamp(-step, step);
        }

        // ---------------------------------------------------------------- wall run (Walling, event 49)
        // Input handler 0xEE65A0 (tested before the jumps and the grab): high profile, Legs pressed, the stick
        // pushed within 45° of the facing (> 0.35), ability Walling, and IHumanGround vt112 = event 49's guard
        // (0xDA54F0: the playing item allows a mode exit, then the wall test 0xE18390).
        if g.high_profile && pad.jump_buffered() && moving && pad.dir.dot(body.forward()) >= 45f32.to_radians().cos() {
            if let Some((contact, normal)) = super::walling::wall_ahead(body.feet, body.forward(), &collision) {
                pad.consume_jump();
                let from = body.feet;
                switch_context(&mut loco, &mut data, TransitionSetup::ToWalling(super::walling::WallingEntry { contact, normal, from }));
                continue;
            }
        }

        // ---------------------------------------------------------------- beam (NarrowObject, event 72)
        // Guard 0xD9F4C0: a beam in the box ahead (±0.75 m, 0–1 m, ±0.53 m). PORT trigger: walking at it (the
        // decision layer's event 72 sender is not traced).
        if moving && !busy {
            if let Some(entry) = super::narrow::try_mount_beam(body.feet, body.forward(), &guidance) {
                switch_context(&mut loco, &mut data, TransitionSetup::ToBeam(entry));
                continue;
            }
        }

        // ---------------------------------------------------------------- climb / grab requests
        // interpreter vt736/740 (grab wall) and vt764/768 (climb start): high profile + Legs into a wall
        let forward = body.forward();
        if g.high_profile && pad.legs_held && moving {
            if let Some(setup) = try_wall_grab(body.feet, forward, &guidance, &collision) {
                pad.consume_jump();
                switch_context(&mut loco, &mut data, setup);
                continue;
            }
        }

        // ---------------------------------------------------------------- ledge stop (event 69 / sub-state 38)
        if let Some(ls) = g.ledge_stop {
            // pull-down EdgeStop (LedgeStop_HandleEvent 0xDA4D90, event 70): only while the start action plays
            // (guard 0xD9D580). PORT trigger as for Wait: Legs.
            if !ls.ending && pad.jump_buffered() {
                if let Some(entry) = pulldown_entry(ls.point, ls.normal, body.feet, false, &guidance, &collision) {
                    pad.consume_jump();
                    g.ledge_stop = None;
                    g.oneshot = None;
                    switch_context(&mut loco, &mut data, TransitionSetup::ToLedge(entry));
                    continue;
                }
            }
            if g.oneshot.is_none() {
                // end done → Movement (Locomotion_Update 0xDAF2D0, sub_5017B0); start → end is in the move step
                g.ledge_stop = None;
            }
        }
        if let Some(n) = g.ledge_stop_lock {
            if pad.speed01 <= 0.0 || pad.dir.dot(Vec3::new(n.x, 0.0, n.z).normalize_or_zero()) < FRONT_COS {
                g.ledge_stop_lock = None;
            }
        }
        // Movement event 69 (0xDB1470): guard 0xDA5DE0 = front edge (ClassifyEdgeSide 0xD9D7F0 → 1), drop > 2 m,
        // body space → ToLedgeStop 0xDA99F0. Side edges (3/4, guard 0xDA5D90) go to another state (not ported).
        // PORT trigger: walking (low profile) into such an edge.
        let busy = g.oneshot.is_some();
        if !g.high_profile && moving && !busy && g.ledge_stop_lock.is_none() {
            if let Some((p, n)) = edge_report(body.feet, body.forward(), LEDGE_STOP_REACH, FRONT_COS, &guidance, &collision) {
                if let Some(b) = super::ledge_moves::single_item(super::ledge_moves::LEDGE_STOP_START, 0) {
                    g.ledge_stop = Some(LedgeStop { point: p, normal: n, ending: false });
                    g.ledge_stop_lock = Some(n);
                    g.speed_param = 0.0;
                    g.blend.speed_param = 0.0;
                    g.play_oneshot(b);
                    continue;
                }
            }
        }

        // ---------------------------------------------------------------- pull-down (event 70)
        // The game's decision layer sends event 70 (not traced); guard 0xD9D6C0: facing along the edge's outward
        // normal, a drop of more than 2 m, body space. PORT trigger: Legs in low profile at an edge.
        if !g.high_profile && pad.jump_buffered() && !busy {
            if let Some(entry) = try_pulldown(body.feet, body.forward(), &guidance, &collision) {
                pad.consume_jump();
                switch_context(&mut loco, &mut data, TransitionSetup::ToLedge(entry));
                continue;
            }
        }

        // ---------------------------------------------------------------- jump requests
        // vt24 JumpToGuidanceTarget: high profile + jump buffer + stick > dead-zone (RE/01 §6.3).
        // While free-running with Legs held the game also jumps when it reaches an edge
        // (hypothesis from gameplay; modelled as: Legs held + no floor ahead).
        let forward = body.forward();
        let want_jump = g.high_profile
            && moving
            && (pad.jump_buffered() || (pad.legs_held && edge_ahead(body.feet, forward, &collision)));
        if want_jump && !busy {
            pad.consume_jump();
            let entry = match find_jump_target(body.feet, if moving { pad.dir } else { forward }, &guidance, &collision) {
                // leading foot: the playing locomotion item (footl item = left ahead) (hypothesis)
                Some(t) => InAirEntry::JumpToTarget { from: body.feet, target: t, speed_param: g.speed_param, foot_left: g.blend.foot == 0 },
                // vt28: free jump without a target
                None => InAirEntry::FreeJump { from: body.feet, dir: forward, speed_param: g.speed_param },
            };
            switch_context(&mut loco, &mut data, TransitionSetup::ToInAir(entry));
            continue;
        }

        // ---------------------------------------------------------------- move (blended clip root motion)
        let (delta, speed) = if let Some(mut os) = g.oneshot {
            // landing / reception action: its blended root motion (FROMANIM)
            os.t += dt;
            let d = os.blend.disp(os.t / os.duration.max(1e-4));
            let step = [d[0] - os.applied[0], d[1] - os.applied[1]];
            os.applied = d;
            g.oneshot = (os.t < os.duration).then_some(os);
            if g.oneshot.is_none() {
                if let Some(next) = g.oneshot_next.take() {
                    g.play_oneshot(next);
                }
                if let Some(mut ls) = g.ledge_stop.filter(|l| !l.ending) {
                    // start done → end action, same frame (HumanGround__LedgeStop_PlayEnd 0xD7D9D0)
                    ls.ending = true;
                    g.ledge_stop = Some(ls);
                    if let Some(b) = super::ledge_moves::single_item(super::ledge_moves::LEDGE_STOP_END, 0) {
                        g.play_oneshot(b);
                    }
                }
            }
            let right = super::right_of(forward);
            let delta = right * step[0] + forward * step[1];
            (delta, delta.length() / dt.max(1e-4))
        } else {
            let speed = if g.speed_param > 0.0 { g.blend.advance(dt) } else { 0.0 };
            (forward * speed * dt, speed)
        };
        // PORT: after a ledge stop, still pushing into the same edge holds the character at it (the game re-sends
        // event 69 from its untraced sender; the port does not loop stop / step back)
        let held = g.oneshot.is_none()
            && g.ledge_stop_lock.is_some()
            && edge_report(body.feet, forward, LEDGE_STOP_REACH, FRONT_COS, &guidance, &collision).is_some();
        let (delta, speed) = if held {
            g.speed_param = 0.0;
            g.blend.speed_param = 0.0;
            (Vec3::ZERO, 0.0)
        } else {
            (delta, speed)
        };
        let mut r = collision.move_capsule(body.feet, delta, true);
        if let Some(ls) = g.ledge_stop {
            // PORT: the stop clip's root motion may not carry the feet past the edge (the game places the edge
            // report so the clip ends on it; its sender is not traced)
            let past = (r.position - ls.point).dot(ls.normal) + LEDGE_STOP_MARGIN;
            if past > 0.0 {
                r.position -= Vec3::new(ls.normal.x, 0.0, ls.normal.z) * past;
            }
        }
        body.velocity = forward * speed;
        body.feet = r.position;

        // ground probe (0xD87720): stay snapped to the floor, otherwise start falling
        match collision.ground_height(body.feet + Vec3::Y * 0.05, GROUND_PROBE + 0.05 + STEP_HEIGHT) {
            Some(h) if body.feet.y - h <= STEP_HEIGHT => {
                body.feet.y = h;
                body.grounded = true;
            }
            _ => {
                body.grounded = false;
                let entry = InAirEntry::Fall { from: body.feet, velocity: body.velocity, origin: FallOrigin::Ground, speed_param: g.speed_param };
                switch_context(&mut loco, &mut data, TransitionSetup::ToInAir(entry));
            }
        }
    }
}

/// Ground → Climb / Ledge / jump-to-ledge when pushing into a wall with high profile + Legs.
/// Order (hypothesis from the interpreter's request order, RE/01 §6.3):
/// 1. climb start: hand holds 1.8–2.4 m up and foot holds 1.2 m below them (vt764/768, FromGround);
/// 2. a ledge with the hands 0.7–3.0 m up → the standing straight jump at it (0xB21DA0 bands: knee / waist
///    heights pull straight up onto the top, higher ones end hanging).
fn try_wall_grab(feet: Vec3, forward: Vec3, guidance: &GuidanceWorld, collision: &CollisionWorld) -> Option<TransitionSetup> {
    // The game's probe (input handler 0xEE65A0 → IHuman vt132 / vt136): guidance within 0.75 m of the character's
    // position (box height 0.45), front hemisphere (cone π about the facing). The edge must face the character.
    let reach = |h: f32| {
        guidance
            .probe(feet + Vec3::Y * h, GRAB_PROBE_RADIUS, 0.225, Some(forward), std::f32::consts::FRAC_PI_2)
            .filter(|hit| (hit.point - feet).dot(forward) > 0.0)
    };
    // 1. climb start
    for h in [1.8f32, 2.4] {
        if let Some(hand) = reach(h) {
            if hand.point.y - feet.y < 1.5 {
                continue;
            }
            let foot = guidance.probe(hand.point - Vec3::Y * CLIMB_ROW * CLIMB_HAND_ROWS as f32, 0.2, 0.15, Some(forward), 0.785);
            if let Some(foot) = foot {
                return Some(TransitionSetup::ToClimb(ClimbEntry {
                    entry_type: ClimbEntryType::FromGround,
                    hand_l: hand.point,
                    hand_r: hand.point,
                    foot_l: foot.point,
                    foot_r: foot.point,
                    normal: hand.wall_normal,
                    from_feet: feet,
                }));
            }
        }
    }
    // 2. a ledge whose hands are 0.53–3.0 m above the feet (guard 0xD84190): the standing straight jump at it
    //    (HumanGround 0xD85550 → Human__SetupJumpToHandTarget 0xB21DA0), band by the hand height
    let target = straight_hand_target(feet, forward, guidance, collision, false)?;
    Some(TransitionSetup::ToInAir(InAirEntry::JumpToTarget { from: feet, target, speed_param: 0.0, foot_left: true }))
}

/// The hand target of a standing straight jump (0xB21DA0): a ledge in the 0.75 m grab probe whose hands are
/// 0.53–3.0 m above the feet; `beam` selects the `beam_jumpstraight_*` flights (jump from the beam impulsion).
pub fn straight_hand_target(feet: Vec3, forward: Vec3, guidance: &GuidanceWorld, collision: &CollisionWorld, beam: bool) -> Option<JumpTarget> {
    let reach = |h: f32| {
        guidance
            .probe(feet + Vec3::Y * h, GRAB_PROBE_RADIUS, 0.225, Some(forward), std::f32::consts::FRAC_PI_2)
            .filter(|hit| (hit.point - feet).dot(forward) > 0.0)
    };
    let hand = [0.6f32, 0.9, 1.3, 1.7, 2.1, 2.5, 2.9]
        .into_iter()
        .filter_map(reach)
        .find(|h| (GRAB_MIN_HEIGHT..=STRAIGHT_JUMP_MAX).contains(&(h.point.y - feet.y)))?;
    let n = hand.wall_normal;
    let wall = super::ledge::hang_type_at(hand.point, n, collision) == super::ledge::LedgeHangType::Wall;
    let dz = hand.point.y - feet.y;
    let j = if beam { super::ledge_moves::hang_jump_in_beam(dz, wall)? } else { super::ledge_moves::hang_jump_in(dz, wall)? };
    Some(JumpTarget { position: hand.point + n * j.out - Vec3::Y * j.down, type_flags: j.flags, hang: Some((hand.point, n)), straight: Some(j) })
}

/// Pull-down type Wait (1) from Movement (0xDB1470 event 70 → fill 0xD843E0 → PullDown_Enter 0xDDE4D0): a
/// LedgeGrab edge at the feet within 0.8 m ahead whose outward normal points along the facing (dot > 0),
/// with more than 2.0 m of drop below it (edge report +48).
fn try_pulldown(feet: Vec3, forward: Vec3, guidance: &GuidanceWorld, collision: &CollisionWorld) -> Option<super::ledge::LedgeEntry> {
    let (p, n) = edge_report(feet, forward, 0.5, 0.0, guidance, collision)?;
    pulldown_entry(p, n, feet, true, guidance, collision)
}

/// Grab / climb probe radius around the character (0xEE65A0: IHuman vt136 radius 0.75).
const GRAB_PROBE_RADIUS: f32 = 0.75;
/// Event 68 guard 0xD84190: the edge must be at least 0.53 m above the feet.
const GRAB_MIN_HEIGHT: f32 = 0.53;

/// How far ahead of the feet an edge stops a walk (PORT: the edge report's distance limit +64 is set by the
/// untraced sender; `ClassifyEdgeSide` compares it with the squared horizontal distance).
const LEDGE_STOP_REACH: f32 = 0.45;
/// PORT: the feet stay this far behind the edge during the ledge stop.
const LEDGE_STOP_MARGIN: f32 = 0.2;
/// ClassifyEdgeSide 0xD9D7F0: front = within 60° (120° with the report flag +68).
const FRONT_COS: f32 = 0.5;

/// An edge report for a front edge (`HumanGround__ClassifyEdgeSide` 0xD9D7F0 → 1): a LedgeGrab edge within
/// `reach` ahead whose outward normal is within 60° of the facing, with more than 2.0 m of drop
/// (guards 0xDA5DE0 / 0xD9D6C0; the classifier itself needs > 1.3 m). Returns (point, outward normal).
fn edge_report(feet: Vec3, forward: Vec3, reach: f32, min_cos: f32, guidance: &GuidanceWorld, collision: &CollisionWorld) -> Option<(Vec3, Vec3)> {
    let hit = guidance.probe(feet + forward * reach, reach, 0.2, None, std::f32::consts::PI)?;
    let n = hit.wall_normal;
    let nf = Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
    if nf.dot(forward) <= min_cos {
        return None;
    }
    let below = collision.ground_height(hit.point + n * 0.6 - Vec3::Y * 0.05, 50.0).unwrap_or(hit.point.y - 100.0);
    if hit.point.y - below <= 2.0 {
        return None;
    }
    Some((hit.point, n))
}

/// Pull-down from the ground at edge `p` / normal `n`: type Wait (from Movement) or EdgeStop (from the ledge stop).
fn pulldown_entry(p: Vec3, n: Vec3, feet: Vec3, wait: bool, guidance: &GuidanceWorld, collision: &CollisionWorld) -> Option<super::ledge::LedgeEntry> {
    let [orient, descent, reception] = super::ledge_moves::pulldown(p, n, feet, wait, guidance, collision)?;
    let mut e = super::ledge::LedgeEntry::at((orient.hand_l + orient.hand_r) * 0.5, n, feet, super::ledge::LedgeSubState::PullDown);
    e.hand_l = orient.hand_l;
    e.hand_r = orient.hand_r;
    e.entry_move = Some(orient);
    e.entry_rest = [Some(descent), Some(reception)];
    Some(e)
}

trait AnyHit {
    fn any_hit(self, f: impl Fn(f32) -> bool) -> bool;
}
impl AnyHit for std::ops::RangeInclusive<f32> {
    fn any_hit(self, f: impl Fn(f32) -> bool) -> bool {
        let (a, b) = (*self.start(), *self.end());
        (0..=5).any(|i| f(a + (b - a) * i as f32 / 5.0))
    }
}
