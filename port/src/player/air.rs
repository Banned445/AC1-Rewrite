//! In-air context (`HumanInAir`, ActorContextID 8) — RE/04.
//!
//! Jumps are NOT ballistic in AC1: a takeoff item and a flight item play back to back (chosen and
//! weighted by Human__ComputeJumpAnimBlend 0xB1EC40, `jump_blend`), and their blended root motion gets a
//! linear correction `(target − animEnd) · t/(T₁+T₂)`, so the jump ends exactly on the chosen target
//! (Human__SetupJumpToTarget 0xB20200, HumanInAir__UpdateJumpMotion 0xE0DEF0). Arrival is checked within
//! 0.01 m (0xE07D00).
//! Real physics only for: the over-drop tail (target > 5 m below → aim 5 m down, then free fall with
//! g = 9.8, horizontal steering ≤ 15 m/s) and plain falls (drift decays 4 m/s², ≤ 5 m/s).
//! Landing: fall height measured from the apex; heavy > 6.3 m, fatal > 7.0 m (0xE00FE0);
//! landing action by drop / distance / speed bucket, total drop > 3 m → damage or damage-roll + camera
//! shake (0xE05940, `jump_blend::landing`).

use bevy::prelude::*;

use super::jump_blend::{self, ActionBlend, TARGET_FREESTEP};
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
    /// The action the Ground context plays on entry (landing 0xE05940 or free-step reception 0xE07D00);
    /// its root motion moves the character until it ends.
    pub action: Option<ActionBlend>,
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
    /// `speed_param` becomes the speed ratio HumanInAir+0x16C (hypothesis: the ground speed parameter);
    /// `foot_left` = the leading foot (byte+60 bits 2–3 of the playing item, or sub_B18850).
    JumpToTarget { from: Vec3, target: JumpTarget, speed_param: f32, foot_left: bool },
    FreeJump { from: Vec3, dir: Vec3, speed_param: f32 },
    Fall { from: Vec3, velocity: Vec3, origin: FallOrigin, speed_param: f32 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AirMode {
    #[default]
    Idle,
    /// Target-driven jump with linear correction.
    Jump {
        from: Vec3,
        clip_end: Vec3,
        aim: Vec3,
        /// PLACEHOLDER arc height (ledge jumps only, see `real`).
        apex: f32,
        duration: f32,
        t: f32,
        /// After the clip, continue as a free fall toward this point (over-drop or free jump).
        then_fall_to: Option<Vec3>,
        /// The game's takeoff + flight items (`HumanInAirData::takeoff/flight`) drive the root. False for
        /// jumps at a ledge (their flight comes from the ledge code, not 0xB20200): PLACEHOLDER arc.
        real: bool,
        /// Takeoff item duration T₁ (HumanInAirData+0x1E4).
        t_takeoff: f32,
        /// Character forward at takeoff (the jump origin matrix +0x10).
        fwd: Vec3,
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
    /// HumanInAir+0x16C: speed ratio for the landing / reception choice (hypothesis: the ground speed
    /// parameter at takeoff; its writer is not traced, the ctor sets 0).
    pub speed_ratio: f32,
    /// Takeoff and flight items of the current jump (+0x1A4 / +0x1A8 and the weight arrays +0x1BC..).
    pub takeoff: Option<ActionBlend>,
    pub flight: Option<ActionBlend>,
    pub foot_left: bool,
    /// Jump / fall start (JumpOrigin +0x10 translation).
    pub start: Vec3,
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
            InAirEntry::JumpToTarget { from, target, speed_param, foot_left } => {
                self.speed_ratio = speed_param;
                self.foot_left = foot_left;
                self.start_y = from.y;
                self.start = from;
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
                self.mode = if target.hang.is_some() {
                    placeholder_jump(from, aim, speed_param, then_fall_to)
                } else {
                    self.real_jump(from, aim, target.type_flags, then_fall_to)
                };
            }
            InAirEntry::FreeJump { from, dir, speed_param } => {
                self.start_y = from.y;
                self.start = from;
                self.apex_y = from.y;
                self.prev_y = from.y;
                self.speed_ratio = speed_param;
                self.foot_left = true;
                // PORT: the game always jumps to a target (vt28 resolves one, 0xD832F0); with none in
                // range the port jumps FREE_JUMP_DISTANCE ahead with the free-step blend, then falls.
                let aim = from + dir * FREE_JUMP_DISTANCE;
                self.mode = self.real_jump(from, aim, TARGET_FREESTEP, Some(aim));
            }
            InAirEntry::Fall { from, origin, speed_param, .. } => {
                self.speed_ratio = speed_param;
                self.start_y = from.y;
                self.start = from;
                self.apex_y = from.y;
                self.prev_y = from.y;
                self.fall_origin = origin;
                self.mode = AirMode::Fall { steer_to: None };
            }
        }
    }
}

impl HumanInAirData {
    /// Human__SetupJumpToTarget 0xB20200 for a ground running jump (kind 0): pick and weight the takeoff and
    /// flight items (0xB1EC40), T₁/T₂ = their Σw·T durations, anim end = their summed displacement.
    fn real_jump(&mut self, from: Vec3, aim: Vec3, target_type: u32, then_fall_to: Option<Vec3>) -> AirMode {
        let flat = Vec3::new(aim.x - from.x, 0.0, aim.z - from.z);
        let fwd = flat.normalize_or(Vec3::NEG_Z);
        let b = jump_blend::compute(aim.y - from.y, flat.length(), target_type, self.foot_left, 1.0);
        let takeoff = ActionBlend::new(b.takeoff, 0, &b.takeoff_w);
        let flight = ActionBlend::new(b.flight, 0, &b.flight_w);
        let (t1, t2) = (takeoff.duration(), flight.duration());
        self.takeoff = Some(takeoff);
        self.flight = Some(flight);
        let end = add(takeoff.disp(1.0), flight.disp(1.0));
        let clip_end = from + to_world(end, fwd);
        AirMode::Jump { from, clip_end, aim, apex: 0.0, duration: (t1 + t2).max(1e-3), t: 0.0, then_fall_to, real: true, t_takeoff: t1, fwd }
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Animation space (x right, y forward, z up) → world, for a character facing `fwd`.
fn to_world(d: [f32; 3], fwd: Vec3) -> Vec3 {
    super::right_of(fwd) * d[0] + fwd * d[1] + Vec3::Y * d[2]
}

/// Blended root displacement of the jump at time `t` (takeoff item, then the flight item from its end).
fn jump_disp(air: &HumanInAirData, t: f32, t1: f32, duration: f32) -> [f32; 3] {
    let (Some(to), Some(fl)) = (air.takeoff, air.flight) else { return [0.0; 3] };
    if t < t1 {
        to.disp(t / t1.max(1e-4))
    } else {
        add(to.disp(1.0), fl.disp((t - t1) / (duration - t1).max(1e-4)))
    }
}

/// PLACEHOLDER jump to a ledge: the jump-into-hang flights (0x01271631 / 0x0121A598 / 0x0121A8B1) are set
/// up by the ledge code, not traced yet — nominal clip length per distance class, parabolic lift.
fn placeholder_jump(from: Vec3, aim: Vec3, speed_param: f32, then_fall_to: Option<Vec3>) -> AirMode {
    let flat = Vec3::new(aim.x - from.x, 0.0, aim.z - from.z);
    let dist = flat.length();
    let nominal = if dist < 2.5 { dist.max(1.0) } else if dist < 5.0 { 4.0 } else { 6.0 };
    let dir = flat.normalize_or_zero();
    let clip_end = from + dir * nominal; // where the uncorrected clip would end (same height)
    let duration = (JUMP_DUR_BASE + JUMP_DUR_PER_M * dist) * (1.15 - 0.3 * speed_param.clamp(0.0, 1.0));
    let apex = JUMP_APEX_BASE + JUMP_APEX_PER_M * dist;
    AirMode::Jump { from, clip_end, aim, apex, duration, t: 0.0, then_fall_to, real: false, t_takeoff: 0.0, fwd: dir }
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
    Landing { kind, fall_height, total_drop, roll: total_drop > ROLL_DROP && kind != LandingType::Fatal, action: None }
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
        // landed by arriving on a jump target (0xE07D00) rather than by ground contact (0xE05200)
        let mut on_target = false;
        let mut hang_on: Option<LedgeEntry> = None;
        match air.mode {
            AirMode::Jump { from, clip_end, aim, apex, duration, t, then_fall_to, real, t_takeoff, fwd } => {
                let t1 = (t + dt).min(duration);
                let s = t1 / duration;
                let clip_pos = if real {
                    // the takeoff + flight items' blended root motion (0xE0DEF0)
                    from + to_world(jump_disp(air, t1, t_takeoff, duration), fwd)
                } else {
                    // PLACEHOLDER: straight line to clip_end + parabolic lift
                    from.lerp(clip_end, s) + Vec3::Y * (4.0 * apex * s * (1.0 - s))
                };
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
                        None => {
                            landed_at = Some(body.feet.y);
                            on_target = real;
                        }
                        _ => landed_at = Some(body.feet.y),
                    }
                } else {
                    air.mode = AirMode::Jump { from, clip_end, aim, apex, duration, t: t1, then_fall_to, real, t_takeoff, fwd };
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
            let mut landing = classify_landing(air.apex_y, air.start_y, y);
            landing.action = Some(if on_target && matches!(air.target_flags, 1 | 0x10000) {
                // free-step reception: the flight's weights, normal / `_fast` by +0x16C (0xE07D00).
                // PORT: the game continues in NarrowObject (on the edge); the port stays in Ground.
                let fw = air.flight.map(|f| f.weights().to_vec()).unwrap_or_default();
                let id = jump_blend::RECEPTION_FREESTEP[(!air.foot_left) as usize];
                ActionBlend::new(id, 0, &jump_blend::reception_weights(&fw, 0.0))
            } else {
                // ground landing (0xE05940): drop and horizontal distance from the jump/fall start; the
                // "stick forward" test (wanted move within 75°) uses the pad direction vs the facing
                let horiz = Vec2::new(body.feet.x - air.start.x, body.feet.z - air.start.z).length();
                let stick_forward = pad.speed01 > 0.0 && pad.dir.dot(body.forward()) >= 75f32.to_radians().cos();
                let (id, w) = jump_blend::landing(air.start_y - y, horiz, stick_forward, air.speed_ratio, air.foot_left);
                ActionBlend::new(id, 0, &w)
            });
            air.mode = AirMode::Idle;
            switch_context(&mut loco, &mut data, TransitionSetup::ToMovement { landing: Some(landing) });
        }
    }
}
