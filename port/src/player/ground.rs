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
use super::ledge::{LedgeEntry, LedgeSubState};
use super::move_blend::MoveBlend;
use super::targets::{edge_ahead, find_jump_target, JumpTarget, TARGET_LEDGE};
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
    /// Seconds of landing recovery left (roll / heavy landing), PLACEHOLDER durations.
    pub recovery: f32,
    pub last_landing: Option<Landing>,
    /// Incremented on every landing (lets the animator play the landing clip once).
    pub landing_seq: u32,
    /// `HumanGround__UpdateMoveBlend` state: blend weights, timers, lean/bank, step cycle.
    pub blend: MoveBlend,
}

impl HumanGroundData {
    /// `TransitionSetupDataToMovement::Apply` (0xC80310), simplified.
    pub fn enter(&mut self, landing: Option<Landing>) {
        self.sub_state = HumanGroundSubState::Movement;
        if let Some(l) = landing {
            self.landing_seq = self.landing_seq.wrapping_add(1);
            self.recovery = match l.kind {
                LandingType::Safe if l.roll => 0.55,
                LandingType::Safe => 0.0,
                LandingType::SmallDamage => 0.4,
                LandingType::HeavyDamage => 1.0,
                LandingType::Fatal => 0.0,
            };
            // (hypothesis) soft landings and rolls keep running momentum, as in the game; the RE shows
            // no speed reset for them. Heavy-damage landings stop the character.
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
                if l.roll || l.kind == LandingType::HeavyDamage {
                    rig.shake = 1.0;
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
        let moving = pad.speed01 > 0.0 && g.recovery <= 0.0;
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
        let target = if moving { (base + STICK_SPAN * pad.speed01 * g.turn_atten).min(1.0) } else { 0.0 };
        // speed parameter, lean/bank and blend weights (MoveBlend 0xDA0810). The heading snapshot is the
        // heading before this frame's turn (HG+0x600, Movement_PreUpdate 0xD97E30).
        g.blend.speed_param = g.speed_param;
        g.blend.update_speed(target, dt);
        g.blend.update_angles(body.heading, moving.then_some(want_heading), None, false, dt);
        g.blend.update_weights(target, dt);
        g.speed_param = g.blend.speed_param;
        g.recovery = (g.recovery - dt).max(0.0);

        // heading: rotate toward wanted at the player turn rate (0xD95290)
        if moving {
            let d = angle_diff(want_heading, body.heading);
            let step = PLAYER_TURN_RATE * dt;
            body.heading += d.clamp(-step, step);
        }

        // ---------------------------------------------------------------- climb / grab requests
        // interpreter vt736/740 (grab wall) and vt764/768 (climb start): high profile + Legs into a wall
        let forward = body.forward();
        if g.high_profile && pad.legs_held && moving && g.recovery <= 0.0 {
            if let Some(setup) = try_wall_grab(body.feet, forward, &guidance, &collision) {
                pad.consume_jump();
                switch_context(&mut loco, &mut data, setup);
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
        if want_jump && g.recovery <= 0.0 {
            pad.consume_jump();
            let entry = match find_jump_target(body.feet, if moving { pad.dir } else { forward }, &guidance, &collision) {
                Some(t) => InAirEntry::JumpToTarget { from: body.feet, target: t, speed_param: g.speed_param },
                // vt28: free jump without a target
                None => InAirEntry::FreeJump { from: body.feet, dir: forward, speed_param: g.speed_param },
            };
            switch_context(&mut loco, &mut data, TransitionSetup::ToInAir(entry));
            continue;
        }

        // ---------------------------------------------------------------- move (blended clip root motion)
        let speed = if g.speed_param > 0.0 { g.blend.advance(dt) } else { 0.0 };
        let delta = forward * speed * dt;
        let r = collision.move_capsule(body.feet, delta, true);
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
                let entry = InAirEntry::Fall { from: body.feet, velocity: body.velocity, origin: FallOrigin::Ground };
                switch_context(&mut loco, &mut data, TransitionSetup::ToInAir(entry));
            }
        }
    }
}

/// Ground → Climb / Ledge / jump-to-ledge when pushing into a wall with high profile + Legs.
/// Order (hypothesis from the interpreter's request order, RE/01 §6.3):
/// 1. climb start: hand holds 1.8–2.4 m up and foot holds 1.2 m below them (vt764/768, FromGround);
/// 2. grab a ledge within reach (hands ≤ 2.4 m up, no foot holds) → Ledge;
/// 3. a ledge up to 3 m higher, close ahead → jump up to it (ledge target).
fn try_wall_grab(feet: Vec3, forward: Vec3, guidance: &GuidanceWorld, collision: &CollisionWorld) -> Option<TransitionSetup> {
    // a wall must be right in front
    let wall_close = (0.4..=0.9).any_hit(|d| collision.point_inside(feet + forward * d + Vec3::Y * 1.0));
    if !wall_close {
        return None;
    }
    let reach = |h: f32| guidance.probe(feet + forward * 0.5 + Vec3::Y * h, 0.6, 0.31, Some(forward), 0.785);
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
            // 2. ledge in reach without foot holds
            if hand.point.y - feet.y <= 2.4 {
                return Some(TransitionSetup::ToLedge(LedgeEntry::at(hand.point, hand.wall_normal, feet, LedgeSubState::Grasp)));
            }
        }
    }
    // 3. jump up to a ledge up to 3 m above the hang root
    let high = [2.7f32, 3.0, 3.3, 3.6, 3.9].into_iter().filter_map(reach).find(|h| h.point.y - feet.y > 2.4);
    if let Some(hand) = high {
        let target = JumpTarget {
            position: super::ledge::hang_root(hand.point, hand.point, hand.wall_normal, super::ledge::LedgeHangType::Wall),
            type_flags: TARGET_LEDGE,
            hang: Some((hand.point, hand.wall_normal)),
        };
        if target.position.y - feet.y <= LEDGE_MAX_UP {
            return Some(TransitionSetup::ToInAir(InAirEntry::JumpToTarget { from: feet, target, speed_param: 0.5 }));
        }
    }
    None
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
