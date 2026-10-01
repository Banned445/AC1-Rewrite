//! In-air context (`HumanInAir`, ActorContextID 8) — RE/04.
//!
//! Jumps are NOT ballistic in AC1: a jump clip plays and its root motion gets a linear correction
//! `(target − clipEnd) · t/duration`, so the jump ends exactly on the chosen target
//! (HumanInAir__UpdateJumpMotion 0xE0DEF0). Arrival is checked within 0.01 m (0xE07D00).
//! Real physics only for: the over-drop tail (target > 5 m below → aim 5 m down, then free fall with
//! g = 9.8, horizontal steering ≤ 15 m/s) and plain falls (drift decays 4 m/s², ≤ 5 m/s).
//! Landing: fall height measured from the apex; heavy > 6.3 m, fatal > 7.0 m (0xE00FE0);
//! total drop > 3 m → roll + camera shake (0xE05940).

use bevy::prelude::*;

use super::ledge::{LedgeEntry, LedgeSubState};
use super::targets::JumpTarget;
use super::{switch_context, ActorContextId, Body, HumanDataBundle, Locomotion, Player, TransitionSetup};
use crate::collision::CollisionWorld;
use crate::guidance::{GuidanceHit, GuidanceWorld};
use crate::input::PadInput;
use crate::tuning::*;

/// `LandingEvent::LandingType` (values from the exe, desc 0x19919D0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandingType {
    Safe = 0,
    #[allow(dead_code)]
    SmallDamage = 1, // threshold defaults to FLT_MAX in the game → never triggers
    HeavyDamage = 2,
    Fatal = 3,
}

#[derive(Clone, Copy, Debug)]
pub struct Landing {
    pub kind: LandingType,
    /// Fall height from the apex (m).
    pub fall_height: f32,
    /// Total drop from the jump/fall start (m).
    pub total_drop: f32,
    pub roll: bool,
}

/// `HumanInAirData::FallOrigin` (desc 0x199587C).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FallOrigin {
    #[default]
    Ground = 0,
    Climb = 1,
    HangWall = 2,
    HangFree = 3,
}

/// What the switching context asks InAir to do (TransitionSetupDataToInAir, RE/01 §4.2).
#[derive(Clone, Copy, Debug)]
pub enum InAirEntry {
    JumpToTarget { from: Vec3, target: JumpTarget, speed_param: f32 },
    FreeJump { from: Vec3, dir: Vec3, speed_param: f32 },
    Fall { from: Vec3, velocity: Vec3, origin: FallOrigin },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AirMode {
    #[default]
    Idle,
    /// Target-driven jump clip with linear correction.
    Jump {
        from: Vec3,
        clip_end: Vec3,
        aim: Vec3,
        apex: f32,
        duration: f32,
        t: f32,
        /// After the clip, continue as a free fall toward this point (over-drop or free jump).
        then_fall_to: Option<Vec3>,
    },
    /// Ballistic fall (optionally steered toward `steer_to`).
    Fall { steer_to: Option<Vec3> },
}

/// Runtime data of the InAir context (subset of reflected HumanInAirData, RE/07).
#[derive(Debug, Default)]
pub struct HumanInAirData {
    /// Incremented on every entry (lets the animator restart the jump clip).
    pub seq: u32,
    /// The jump target (+0x230), kept so arrival can hand off to the right context.
    pub target: Option<JumpTarget>,
    /// Seconds since entering InAir (used to avoid re-catching the ledge just released).
    pub time_in_air: f32,
    /// Last catch (fall height ≥ 3 m selects the long catch animations in the game, 0xE0BB70).
    pub long_catch: bool,
    pub mode: AirMode,
    /// JumpApexPosition (+0xD0): where descent started — the fall-height reference.
    pub apex_y: f32,
    pub apex_reached: bool,
    /// Height at the jump/fall start (total-drop reference).
    pub start_y: f32,
    pub fall_origin: FallOrigin,
    /// Jump target type flags (+0x290).
    pub target_flags: u32,
    pub prev_y: f32,
}

impl HumanInAirData {
    pub fn enter(&mut self, entry: InAirEntry) {
        self.seq = self.seq.wrapping_add(1);
        self.apex_reached = false;
        self.fall_origin = FallOrigin::Ground;
        self.target_flags = 0;
        self.target = None;
        self.time_in_air = 0.0;
        match entry {
            InAirEntry::JumpToTarget { from, target, speed_param } => {
                self.start_y = from.y;
                self.apex_y = from.y;
                self.prev_y = from.y;
                self.target_flags = target.type_flags;
                self.target = Some(target);
                // over-drop rule (0xB1B8C0): target > 5 m below → aim 5 m down, then free-fall.
                let mut aim = target.position;
                let mut then_fall_to = None;
                if target.position.y < from.y - OVERDROP {
                    aim.y = from.y - OVERDROP;
                    then_fall_to = Some(target.position);
                }
                self.mode = jump_clip(from, aim, speed_param, then_fall_to);
            }
            InAirEntry::FreeJump { from, dir, speed_param } => {
                self.start_y = from.y;
                self.apex_y = from.y;
                self.prev_y = from.y;
                let aim = from + dir * FREE_JUMP_DISTANCE;
                self.mode = jump_clip(from, aim, speed_param, Some(aim));
            }
            InAirEntry::Fall { from, origin, .. } => {
                self.start_y = from.y;
                self.apex_y = from.y;
                self.prev_y = from.y;
                self.fall_origin = origin;
                self.mode = AirMode::Fall { steer_to: None };
            }
        }
    }
}

/// PLACEHOLDER "clip": the real takeoff/flight clips are chosen by 0xB1EC40 from distance class
/// (2.5 / 5 / 7 m for ground targets) — here a nominal clip length per class stands in, and the
/// game's linear correction makes it land exactly on `aim`.
fn jump_clip(from: Vec3, aim: Vec3, speed_param: f32, then_fall_to: Option<Vec3>) -> AirMode {
    let flat = Vec3::new(aim.x - from.x, 0.0, aim.z - from.z);
    let dist = flat.length();
    let nominal = if dist < 2.5 { dist.max(1.0) } else if dist < 5.0 { 4.0 } else { 6.0 };
    let dir = flat.normalize_or_zero();
    let clip_end = from + dir * nominal; // where the uncorrected clip would end (same height)
    let duration = (JUMP_DUR_BASE + JUMP_DUR_PER_M * dist) * (1.15 - 0.3 * speed_param.clamp(0.0, 1.0));
    let apex = JUMP_APEX_BASE + JUMP_APEX_PER_M * dist;
    AirMode::Jump { from, clip_end, aim, apex, duration, t: 0.0, then_fall_to }
}

fn classify_landing(apex_y: f32, start_y: f32, land_y: f32) -> Landing {
    let fall_height = (apex_y - land_y).max(0.0);
    let total_drop = (start_y - land_y).max(0.0);
    let kind = if fall_height > FALL_FATAL {
        LandingType::Fatal
    } else if fall_height > FALL_HEAVY {
        LandingType::HeavyDamage
    } else {
        LandingType::Safe
    };
    Landing { kind, fall_height, total_drop, roll: total_drop > ROLL_DROP && kind != LandingType::Fatal }
}

/// CheckAirCatch 0xE0BB70 / FindLedgeCatch 0xE0A990: look for an edge in the hand box at the reach
/// height, facing the character within 70°.
fn find_air_catch(feet: Vec3, facing: Vec3, guidance: &GuidanceWorld) -> Option<GuidanceHit> {
    for reach in [CATCH_REACH_WALL, CATCH_REACH_LEDGE] {
        let p = feet + Vec3::Y * reach + facing * 0.3;
        if let Some(h) = guidance.probe(p, CATCH_BOX_R, CATCH_BOX_V, Some(facing), CATCH_MAX_ANGLE) {
            return Some(h);
        }
    }
    None
}

pub fn update_air(
    time: Res<Time>,
    pad: Res<PadInput>,
    collision: Res<CollisionWorld>,
    guidance: Res<GuidanceWorld>,
    mut q: Query<(&mut Locomotion, &mut Body, &mut HumanDataBundle), With<Player>>,
) {
    let dt = time.delta_secs().min(1.0 / 20.0);
    for (mut loco, mut body, mut data) in &mut q {
        if loco.current != ActorContextId::InAir {
            continue;
        }
        if loco.just_switched {
            loco.just_switched = false;
            continue;
        }
        let air = &mut data.air;
        air.time_in_air += dt;

        let mut landed_at: Option<f32> = None;
        let mut hang_on: Option<LedgeEntry> = None;
        match air.mode {
            AirMode::Jump { from, clip_end, aim, apex, duration, t, then_fall_to } => {
                let t1 = (t + dt).min(duration);
                let s = t1 / duration;
                // clip root motion (placeholder): straight line to clip_end + parabolic lift
                let clip_pos = from.lerp(clip_end, s) + Vec3::Y * (4.0 * apex * s * (1.0 - s));
                // linear correction toward the real target (0xE0DEF0)
                let correction = (aim - clip_end) * s;
                let next = clip_pos + correction;
                let r = collision.move_capsule(body.feet, next - body.feet, false);
                body.velocity = (r.position - body.feet) / dt.max(1e-4);
                body.heading = super::heading_of(Vec3::new(aim.x - from.x, 0.0, aim.z - from.z).normalize_or(body.forward()));
                body.feet = r.position;
                let blocked = (r.position - next).length() > 0.05;
                if blocked {
                    // hit something mid-jump (anti-stuck 0xE0B1E0, simplified): fall from here
                    air.mode = AirMode::Fall { steer_to: None };
                    body.velocity.x *= 0.3;
                    body.velocity.z *= 0.3;
                } else if t1 >= duration {
                    // arrival (0xE07D00): within 0.01 m by construction
                    debug_assert!((body.feet - aim).length() < ARRIVAL_TOLERANCE + 0.05);
                    // hand-off by target type (ledge targets → Ledge context)
                    if let Some((mid, normal)) = air.target.and_then(|t| t.hang) {
                        hang_on = Some(LedgeEntry::at(mid, normal, body.feet, LedgeSubState::HangWallReception));
                    }
                    match then_fall_to {
                        _ if hang_on.is_some() => {}
                        Some(p) if collision.ground_height(body.feet + Vec3::Y * 0.05, 0.1).is_none() => {
                            air.mode = AirMode::Fall { steer_to: if p == aim { None } else { Some(p) } };
                        }
                        _ => landed_at = Some(body.feet.y),
                    }
                } else {
                    air.mode = AirMode::Jump { from, clip_end, aim, apex, duration, t: t1, then_fall_to };
                }
            }
            AirMode::Fall { steer_to } => {
                body.velocity.y -= GRAVITY * dt;
                let mut h = Vec3::new(body.velocity.x, 0.0, body.velocity.z);
                match steer_to {
                    Some(p) => {
                        // steer horizontally onto the real target, ≤ 15 m/s (0xE0DEF0 / 0xE00730)
                        let fall_left = (body.feet.y - p.y).max(0.01);
                        let t_left = (2.0 * fall_left / GRAVITY).sqrt().max(dt);
                        let want = Vec3::new(p.x - body.feet.x, 0.0, p.z - body.feet.z) / t_left;
                        h = want.clamp_length_max(FREEFALL_MAX_HORIZONTAL);
                    }
                    None => {
                        // non-target drift: decays 4 m/s², capped 5 m/s (0x19BA434/438)
                        let len = h.length();
                        let new_len = (len - DRIFT_DECEL * dt).max(0.0).min(DRIFT_MAX);
                        h = if len > 1e-4 { h * (new_len / len) } else { Vec3::ZERO };
                    }
                }
                body.velocity.x = h.x;
                body.velocity.z = h.z;
                let r = collision.move_capsule(body.feet, body.velocity * dt, false);
                body.feet = r.position;
                if r.hit_wall {
                    body.velocity.x = 0.0;
                    body.velocity.z = 0.0;
                }
                if r.hit_ceiling && body.velocity.y > 0.0 {
                    body.velocity.y = 0.0;
                }
                if r.landed && body.velocity.y <= 0.0 {
                    landed_at = Some(body.feet.y);
                } else if pad.legs_held && body.velocity.y <= 0.5 && air.time_in_air > 0.3 {
                    // grab requested (SetGrabRequested 0xE102D0) → catch a ledge in reach
                    if let Some(h) = find_air_catch(body.feet, body.forward(), &guidance) {
                        air.long_catch = air.apex_y - body.feet.y >= 3.0;
                        hang_on = Some(LedgeEntry::at(h.point, h.wall_normal, body.feet, LedgeSubState::HangWallReception));
                    }
                }
            }
            AirMode::Idle => {
                air.mode = AirMode::Fall { steer_to: None };
            }
        }

        // apex tracking: first frame vertical velocity < -0.001 (RE/04 §1)
        if !air.apex_reached && body.feet.y - air.prev_y < -0.001 * dt {
            air.apex_reached = true;
            air.apex_y = air.prev_y;
        }
        if !air.apex_reached {
            air.apex_y = air.apex_y.max(body.feet.y);
        }
        air.prev_y = body.feet.y;
        body.grounded = false;

        if let Some(entry) = hang_on {
            air.mode = AirMode::Idle;
            body.velocity = Vec3::ZERO;
            switch_context(&mut loco, &mut data, TransitionSetup::ToLedge(entry));
        } else if let Some(y) = landed_at {
            body.grounded = true;
            body.velocity.y = 0.0;
            let landing = classify_landing(air.apex_y, air.start_y, y);
            air.mode = AirMode::Idle;
            switch_context(&mut loco, &mut data, TransitionSetup::ToMovement { landing: Some(landing) });
        }
    }
}
