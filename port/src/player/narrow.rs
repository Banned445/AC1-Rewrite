//! HumanNarrowObject (context 12), beams (RE/05 §2).
//!
//! Entry from Ground: Movement event 72 (guard 0xD9F4C0 → fill 0xD84A40 → context 12). The beam class state
//! (`HumanNarrowObjectBeam`, 0xF6E000–0xF81000) aligns to the entry point (`AlignToBeamEntry` 0xF7EBA0), then
//! in Main moves by the beam actions' **root motion projected onto the beam line**
//! (`ConstrainRootMotionToBeam` 0xF7C3A0): it stops 0.3 m before an end, and steps off an end that has free
//! space beyond it (`CanStepOffBeamEnd` 0xF77C00: within 0.16 m of the end, free capsule at end + 0.5·dir).
//! Stick classification vs the facing (`ClassifyStickDir` 0xF76150): |a| < 75° forward, > 135° back (turn).

use bevy::prelude::*;

use super::jump_blend::{self, ActionBlend};
use super::{switch_context, ActorContextId, Body, HumanDataBundle, Locomotion, Player, TransitionSetup};
use crate::collision::CollisionWorld;
use crate::guidance::{GuidanceSubType, GuidanceWorld};
use crate::input::PadInput;

/// HumanNarrowObject block actions (by clip name).
pub const BEAM_WAIT: [u32; 2] = [0x28A3_A8E2, 0x28A3_A8E3]; // xx_l_beam_crouchwait_foot{l,r}
/// Two items (foot l, foot r), each [crouchwalk, crouchjog].
pub const BEAM_WALK: u32 = 0x28A3_A8E4;
pub const BEAM_START: [u32; 2] = [0x3466_2CB4, 0x3466_2CB5]; // crouchwait_foot{l,r}_tr_crouch{walk,jog}
pub const BEAM_JOG_STOP: [u32; 2] = [0x3466_339D, 0x3466_339E]; // crouchjog_stop_foot{l,r}
pub const BEAM_TURN180: [u32; 2] = [0x28A3_A8EE, 0x28A3_A8EF]; // crouchwait_foot{l,r}_turn180

pub const DUMPED_ACTIONS: &[u32] = &[
    BEAM_WAIT[0], BEAM_WAIT[1], BEAM_WALK, BEAM_START[0], BEAM_START[1], BEAM_JOG_STOP[0], BEAM_JOG_STOP[1], BEAM_TURN180[0], BEAM_TURN180[1],
];

/// 0xF7C3A0: the root stops this far before a beam end.
pub const BEAM_END_STOP: f32 = 0.3;
/// 0xF77C00: step off within this distance of the end.
pub const BEAM_STEP_OFF: f32 = 0.16;
/// Entry alignment time (PORT: `AlignToBeamEntry` runs until the entry reception is done, not timed).
const ENTRY_TIME: f32 = 0.25;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BeamState {
    #[default]
    Entry,
    Wait,
    Start,
    Walk,
    Stop,
    Turn,
}

#[derive(Clone, Copy, Debug)]
pub struct BeamEntry {
    pub p0: Vec3,
    pub p1: Vec3,
    /// Entry point on the beam line.
    pub point: Vec3,
    pub from: Vec3,
    /// Walking toward p1 (true) or p0.
    pub toward_p1: bool,
}

#[derive(Debug, Default)]
pub struct HumanNarrowObjectData {
    pub p0: Vec3,
    pub p1: Vec3,
    /// Distance along the beam from p0.
    pub s: f32,
    pub toward_p1: bool,
    pub state: BeamState,
    pub action: Option<ActionBlend>,
    pub t: f32,
    /// Leading foot of the next walk item (0 left, 1 right).
    pub foot: usize,
    pub entry_from: Vec3,
    pub seq: u32,
    /// Root displacement already applied for the playing action (forward, animation space).
    applied: f32,
}

fn item(id: u32, item: usize, w: &[f32]) -> Option<ActionBlend> {
    let n = jump_blend::action_items(id)?.get(item)?.len();
    let mut v = vec![0.0; n];
    for (k, x) in w.iter().enumerate().take(n) {
        v[k] = *x;
    }
    if w.is_empty() {
        v[0] = 1.0;
    }
    Some(ActionBlend::new(id, item, &v))
}

impl HumanNarrowObjectData {
    pub fn enter(&mut self, e: BeamEntry) {
        self.p0 = e.p0;
        self.p1 = e.p1;
        self.s = (e.point - e.p0).dot(self.dir());
        self.toward_p1 = e.toward_p1;
        self.entry_from = e.from;
        self.foot = 0;
        self.play(BeamState::Entry, item(BEAM_WAIT[0], 0, &[]));
    }

    fn play(&mut self, state: BeamState, a: Option<ActionBlend>) {
        self.state = state;
        self.action = a;
        self.t = 0.0;
        self.applied = 0.0;
        self.seq = self.seq.wrapping_add(1);
    }

    /// Beam direction p0 → p1 (horizontal).
    pub fn dir(&self) -> Vec3 {
        let d = self.p1 - self.p0;
        Vec3::new(d.x, 0.0, d.z).normalize_or_zero()
    }

    pub fn len(&self) -> f32 {
        let d = self.p1 - self.p0;
        Vec3::new(d.x, 0.0, d.z).length()
    }

    pub fn facing(&self) -> Vec3 {
        if self.toward_p1 { self.dir() } else { -self.dir() }
    }

    /// Root on the beam line at `s`.
    pub fn point(&self, s: f32) -> Vec3 {
        let u = (s / self.len().max(1e-4)).clamp(0.0, 1.0);
        self.p0.lerp(self.p1, u)
    }

    /// Distance left to the end the character faces.
    fn to_end(&self) -> f32 {
        if self.toward_p1 { self.len() - self.s } else { self.s }
    }

    pub fn current(&self) -> Option<(ActionBlend, f32)> {
        self.action.map(|a| (a, (self.t / a.duration().max(1e-4)).min(1.0)))
    }
}

/// Movement event 72's guard 0xD9F4C0: a beam (guidance) in the box ahead of the feet: ±0.75 m sideways,
/// 0–1.0 m ahead, ±0.53 m vertically (`sub_116E1A0`); the facing within 30° of the beam axis (straight entry,
/// mode 2: |dot| ≥ 0.866, `TryMountBeam` 0xE52AD0). Side entries (mode 3) are not ported.
pub fn try_mount_beam(feet: Vec3, forward: Vec3, guidance: &GuidanceWorld) -> Option<BeamEntry> {
    let f = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    let r = super::right_of(f);
    for e in &guidance.edges {
        if e.subtype != GuidanceSubType::Beam {
            continue;
        }
        let axis = Vec3::new(e.p1.x - e.p0.x, 0.0, e.p1.z - e.p0.z).normalize_or_zero();
        let along = axis.dot(f);
        if along.abs() < 0.866 {
            continue;
        }
        // the beam end nearest to the feet, a little onto the beam
        let (start, toward_p1) = if along > 0.0 { (e.p0, true) } else { (e.p1, false) };
        let d = start - feet;
        let ahead = d.dot(f);
        if !(0.0..=1.0).contains(&ahead) || d.dot(r).abs() > 0.75 || d.y.abs() > 0.53 {
            continue;
        }
        let point = start + if toward_p1 { axis } else { -axis } * 0.2;
        return Some(BeamEntry { p0: e.p0, p1: e.p1, point, from: feet, toward_p1 });
    }
    None
}

/// `CanStepOffBeamEnd` 0xF77C00: free capsule (r 0.25) at end + 0.5·dir, 1.2 m above the feet, and floor there.
fn can_step_off(n: &HumanNarrowObjectData, collision: &CollisionWorld) -> bool {
    let end = if n.toward_p1 { n.p1 } else { n.p0 };
    let beyond = end + n.facing() * 0.5;
    !collision.point_inside(beyond + Vec3::Y * 1.2) && collision.ground_height(beyond + Vec3::Y * 0.3, 0.6).is_some()
}

pub fn update_narrow(
    time: Res<Time>,
    pad: Res<PadInput>,
    collision: Res<CollisionWorld>,
    mut q: Query<(&mut Locomotion, &mut Body, &mut HumanDataBundle), With<Player>>,
) {
    let dt = time.delta_secs().min(1.0 / 20.0);
    for (mut loco, mut body, mut data) in &mut q {
        if loco.current != ActorContextId::NarrowObject {
            continue;
        }
        if loco.just_switched {
            loco.just_switched = false;
            continue;
        }
        let n = &mut data.narrow;
        n.t += dt;
        let facing = n.facing();
        body.velocity = Vec3::ZERO;
        body.grounded = true;
        // stick vs facing (0xF76150)
        let stick = pad.speed01 > 0.0;
        let a = if stick { pad.dir.dot(facing).clamp(-1.0, 1.0).acos() } else { 0.0 };
        let forward = stick && a < 75f32.to_radians();
        let back = stick && a > 135f32.to_radians();
        let jog = pad.high_profile;
        let walk_w = if jog { [0.0, 1.0] } else { [1.0, 0.0] };
        let dur = n.action.map(|a| a.duration()).unwrap_or(0.3);
        let done = n.t >= dur;

        // root motion projected on the beam (0xF7C3A0): the action's forward displacement
        if let Some(act) = n.action {
            let d = act.disp((n.t / dur).min(1.0))[1];
            let step = d - n.applied;
            n.applied = d;
            if matches!(n.state, BeamState::Walk | BeamState::Start | BeamState::Stop) {
                let sign = if n.toward_p1 { 1.0 } else { -1.0 };
                let mut s = n.s + step * sign;
                let stop = if can_step_off(n, &collision) { BEAM_STEP_OFF } else { BEAM_END_STOP };
                let limit_lo = if n.toward_p1 { 0.0 } else { stop };
                let limit_hi = if n.toward_p1 { n.len() - stop } else { n.len() };
                s = s.clamp(limit_lo, limit_hi);
                n.s = s;
            }
        }

        match n.state {
            BeamState::Entry => {
                let k = (n.t / ENTRY_TIME).min(1.0);
                body.feet = n.entry_from.lerp(n.point(n.s), k);
                body.heading = super::heading_of(facing);
                if k >= 1.0 {
                    let w = item(BEAM_WAIT[n.foot], 0, &[]);
                    n.play(BeamState::Wait, w);
                }
                continue;
            }
            BeamState::Wait => {
                if back {
                    let t = item(BEAM_TURN180[n.foot], 0, &[]);
                    n.play(BeamState::Turn, t);
                } else if forward && n.to_end() > BEAM_END_STOP + 0.05 {
                    let st = item(BEAM_START[n.foot], 0, &walk_w);
                    n.play(BeamState::Start, st);
                } else if forward && can_step_off(n, &collision) {
                    let st = item(BEAM_START[n.foot], 0, &walk_w);
                    n.play(BeamState::Start, st);
                } else if done {
                    let w = item(BEAM_WAIT[n.foot], 0, &[]);
                    n.play(BeamState::Wait, w);
                }
            }
            BeamState::Start | BeamState::Walk => {
                if !forward {
                    // released: the jog stops with its stop action, the walk settles into the wait
                    let next = if jog { (BeamState::Stop, item(BEAM_JOG_STOP[n.foot], 0, &[])) } else { (BeamState::Wait, item(BEAM_WAIT[n.foot], 0, &[])) };
                    n.play(next.0, next.1);
                } else if done {
                    if n.state == BeamState::Walk {
                        n.foot ^= 1;
                    }
                    let w = item(BEAM_WALK, n.foot, &walk_w);
                    n.play(BeamState::Walk, w);
                }
            }
            BeamState::Stop => {
                if done {
                    let w = item(BEAM_WAIT[n.foot], 0, &[]);
                    n.play(BeamState::Wait, w);
                }
            }
            BeamState::Turn => {
                if done {
                    n.toward_p1 = !n.toward_p1;
                    let w = item(BEAM_WAIT[n.foot], 0, &[]);
                    n.play(BeamState::Wait, w);
                }
            }
        }
        let n = &data.narrow;
        body.feet = n.point(n.s);
        body.heading = super::heading_of(n.facing());
        // step off the end onto the floor beyond (0xF77C00) → Ground
        if matches!(n.state, BeamState::Walk | BeamState::Start) && n.to_end() <= BEAM_STEP_OFF + 1e-3 && can_step_off(n, &collision) {
            body.feet += n.facing() * (BEAM_STEP_OFF + 0.2);
            switch_context(&mut loco, &mut data, TransitionSetup::ToMovement { landing: None });
        }
    }
}
