//! Limb IK: pins hands and feet onto the holds / ledge edges chosen by the Ledge and Climb contexts.
//!
//! Game behaviour (RE/11, limb-IK component at Human+1328, `LimbIK__SolveEffectors` 0xE57570):
//! - four limbs (0 L hand, 1 R hand, 2 L foot, 3 R foot), each with a blend weight that rises at
//!   4/s while the limb has a contact (0.25 s fade-in) and falls at 5/s once released (0.2 s);
//! - a limb moving to a new hold travels from the old contact to the new one over the move's
//!   window: goal = lerp(old + anim delta, new, s), where the anim delta is the limb's own motion in
//!   the playing clip (`LimbIK__AccumAnimEffectorDelta` 0xE56450);
//! - the goals are handed to a full-body solver (HumanIK-style effectors with pull weights,
//!   `IKRig__Solve` 0x4FADA0) that may move the body towards them.
//!
//! The port replaces the middleware solver with:
//! 1. a **contact fit**: the animated body is translated so the clip's own wrists/ankles best match
//!    their goals (the body follows the limbs, like the solver's pull) — the clips were authored
//!    around the holds, so the remaining error is a few centimetres;
//! 2. a reach pull for hands still out of reach;
//! 3. an analytic two-bone solve per limb that keeps the animation's bend plane, after which the
//!    hand/foot keeps its **animated world orientation** (the grip/finger pose stays as authored).

use bevy::prelude::*;

use crate::model::Rig;
use crate::player::{Body, LimbTargets};
use crate::tuning::*;

/// Bone ids (CRC32 of the bone name, RE/data/altair_bone_names.json): (upper, middle, end).
pub const LIMB_CHAINS: [(u32, u32, u32); 4] = [
    (0xeb83_0ada, 0x89b9_3a80, 0xb675_f36c), // LeftArm, LeftForeArm, LeftHand
    (0x6bb3_f727, 0x7257_a1aa, 0x75f9_4d30), // RightArm, RightForeArm, RightHand
    (0x1761_83f0, 0x060d_f401, 0x5898_8870), // LeftUpLeg, LeftLeg, LeftFoot
    (0x757f_1291, 0x863d_09fc, 0x9b14_362c), // RightUpLeg, RightLeg, RightFoot
];

/// Per-limb runtime state (weight + travel between holds).
#[derive(Clone, Copy, Debug, Default)]
pub struct LimbState {
    pub weight: f32,
    /// Current IK goal (contact point, world space).
    pub goal: Vec3,
    from: Vec3,
    to: Vec3,
    /// Animated contact position when the travel started (for the anim delta).
    anim_from: Vec3,
    t: f32,
    dur: f32,
    has_goal: bool,
    /// Tag-driven: inside a travel window.
    travel: bool,
    /// Tag-driven: seconds the hold has differed from the contact without a release.
    wait: f32,
}

/// What the playing clip's contact tag says about one limb this frame (LimbIK__UpdateContactsFromAnimTags
/// 0xE56FC0): in contact, between contacts (travel window [t0, t1], `s` = progress), or released.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tag {
    On,
    Travel { s: f32 },
    Off,
}

/// Contact bit of each limb in `AcuatorsContactsTypes`: L hand 4, R hand 5, L toes 2, R toes 3.
pub const LIMB_CONTACT_BIT: [usize; 4] = [4, 5, 2, 3];

pub fn limb_tag(c: &crate::anim::ContactState, limb: usize) -> Option<Tag> {
    if !c.tagged {
        return None;
    }
    let b = LIMB_CONTACT_BIT[limb];
    if c.bits >> b & 1 != 0 {
        return Some(Tag::On);
    }
    match (c.off_since[b], c.next_on[b]) {
        (Some(t0), Some(t1)) if t1 > t0 => Some(Tag::Travel { s: ((c.t - t0) / (t1 - t0)).clamp(0.0, 1.0) }),
        _ => Some(Tag::Off),
    }
}

impl LimbState {
    /// Tag-driven update (game rules, RE/11 §2 + RE/13 §5). `target` is the context's hold; `anim` the
    /// clip's own contact point this frame.
    pub fn update_tagged(&mut self, target: Option<Vec3>, anim: Vec3, tag: Tag, dt: f32) -> Option<Vec3> {
        let Some(p) = target else { return self.update(None, anim, 0.0, dt) };
        match tag {
            Tag::Off => {
                // released by the animation: fade out from where the limb let go
                self.weight = (self.weight - dt * IK_WEIGHT_OUT_RATE).max(0.0);
                self.dur = 0.0;
                self.travel = false;
            }
            Tag::On => {
                self.weight = (self.weight + dt * IK_WEIGHT_IN_RATE).min(1.0);
                if !self.has_goal {
                    *self = LimbState { weight: self.weight, goal: p, from: p, to: p, anim_from: anim, has_goal: true, ..Default::default() };
                } else if self.travel {
                    // contact made: the limb is on its new hold
                    self.goal = self.to;
                    self.travel = false;
                    self.wait = 0.0;
                }
                if (p - self.goal).length() > IK_RETARGET_DIST {
                    // a new hold chosen while the limb is still in contact: it stays on the old one until
                    // the animation releases it. PORT: if this clip never releases it, settle after a wait.
                    self.wait += dt;
                    if self.wait > IK_CONTACT_WAIT {
                        self.goal = self.goal.lerp(p, (dt * 8.0).min(1.0));
                    }
                } else {
                    self.goal = p;
                    self.wait = 0.0;
                }
                self.to = self.goal;
            }
            Tag::Travel { s } => {
                if !self.travel {
                    // contact released with a re-contact ahead: travel from here to the new hold (a limb
                    // that never had a contact starts at the hold)
                    self.travel = true;
                    self.from = if self.has_goal { self.goal } else { p };
                    self.anim_from = anim;
                }
                self.to = p;
                self.weight = (self.weight + dt * IK_WEIGHT_IN_RATE).min(1.0);
                self.has_goal = true;
                let followed = self.from + (anim - self.anim_from);
                self.goal = followed.lerp(self.to, s);
            }
        }
        (self.weight > 0.0).then_some(self.goal)
    }
    /// Advance one frame towards `target` (None = released). `anim` is where the clip puts this
    /// limb's contact point this frame. Returns the goal while weighted.
    pub fn update(&mut self, target: Option<Vec3>, anim: Vec3, transit: f32, dt: f32) -> Option<Vec3> {
        match target {
            Some(p) => {
                // 0xE57570: weight += dt * 4, clamped to 1
                self.weight = (self.weight + dt * IK_WEIGHT_IN_RATE).min(1.0);
                if !self.has_goal {
                    // new contact: no travel, the weight fade carries the limb in
                    *self = LimbState { weight: self.weight, goal: p, from: p, to: p, anim_from: anim, t: 0.0, dur: 0.0, has_goal: true, travel: false, wait: 0.0 };
                } else if (p - self.to).length() > IK_RETARGET_DIST {
                    // the limb moves to a new hold: travel from where it is now
                    self.from = self.goal;
                    self.to = p;
                    self.anim_from = anim;
                    self.t = 0.0;
                    self.dur = transit.max(1e-3);
                }
                if self.dur > 0.0 {
                    self.t = (self.t + dt).min(self.dur);
                    let s = self.t / self.dur;
                    // goal = lerp(old + anim delta, new, s): the clip's reach arc, corrected onto the hold
                    let followed = self.from + (anim - self.anim_from);
                    self.goal = followed.lerp(self.to, s);
                    if self.t >= self.dur {
                        self.dur = 0.0;
                        self.goal = self.to;
                    }
                } else {
                    self.goal = self.to;
                }
            }
            None => {
                // released: weight -= dt * 5; the goal stays where the limb let go
                self.weight = (self.weight - dt * IK_WEIGHT_OUT_RATE).max(0.0);
                self.has_goal = false;
            }
        }
        (self.weight > 0.0).then_some(self.goal)
    }
}

#[derive(Component, Default)]
pub struct LimbIk {
    pub limbs: [LimbState; 4],
    /// Last contact-fit translation (world), for the log.
    pub fit: Vec3,
    /// Joint indices of each chain (resolved once from the rig).
    chains: Option<[[usize; 3]; 4]>,
}

pub struct IkPlugin;

impl Plugin for IkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (add_limb_ik, solve_limbs).chain().after(crate::anim::apply_clip));
        if std::env::var_os("AC_IK_LOG").is_some() {
            app.add_systems(PostUpdate, log_ik_error.after(bevy::transform::TransformSystems::Propagate));
        }
    }
}

fn add_limb_ik(mut commands: Commands, q: Query<Entity, (With<Rig>, Without<LimbIk>)>) {
    for e in &q {
        commands.entity(e).insert(LimbIk::default());
    }
}

/// Rigid transform (the rig has unit scale everywhere).
#[derive(Clone, Copy)]
struct Iso {
    rot: Quat,
    pos: Vec3,
}

impl Iso {
    fn mul(&self, t: &Transform) -> Iso {
        Iso { rot: self.rot * t.rotation, pos: self.pos + self.rot * t.translation }
    }
}

/// Where the IK end bone (wrist / ankle) goes for a contact point, in world space. The offsets come
/// from the game's hang/climb clips (see tuning.rs): the wrist hangs 0.1 m below the edge point.
pub fn effector_goal(limb: usize, contact: Vec3, normal: Vec3) -> Vec3 {
    if limb < 2 {
        contact + Vec3::Y * IK_HAND_DROP + normal * IK_HAND_OUT
    } else {
        contact + Vec3::Y * IK_FOOT_UP + normal * IK_FOOT_OUT
    }
}

/// Inverse of `effector_goal`: the contact point an end bone at `p` corresponds to.
fn contact_of(limb: usize, p: Vec3, normal: Vec3) -> Vec3 {
    p - (effector_goal(limb, Vec3::ZERO, normal))
}

fn solve_limbs(
    time: Res<Time>,
    mut q: Query<(&Rig, &Body, &LimbTargets, &mut LimbIk, Option<&crate::anim::AnimPlayer>)>,
    mut joints: Query<&mut Transform>,
) {
    let dt = time.delta_secs().min(1.0 / 20.0);
    for (rig, body, targets, mut ik, player) in &mut q {
        let contacts = player.map(|p| p.contacts).unwrap_or_default();
        if ik.chains.is_none() {
            let find = |id: u32| rig.bone_ids.iter().position(|b| *b == id);
            let mut chains = [[usize::MAX; 3]; 4];
            for (i, (a, b, c)) in LIMB_CHAINS.iter().enumerate() {
                if let (Some(a), Some(b), Some(c)) = (find(*a), find(*b), find(*c)) {
                    chains[i] = [a, b, c];
                }
            }
            if chains.iter().any(|c| c[0] == usize::MAX) {
                warn!("limb IK: limb bones not found in the skeleton; IK disabled");
            }
            ik.chains = Some(chains);
        }
        let chains = ik.chains.unwrap();
        if chains.iter().any(|c| c[0] == usize::MAX) {
            continue;
        }
        if std::env::var_os("AC_NO_IK").is_some() {
            continue;
        }

        // ---------------------------------------------------------------- world pose of the rig (animated)
        let player = Iso { rot: Quat::from_rotation_y(body.heading), pos: body.feet };
        let Ok(root_t) = joints.get(rig.root).copied() else { continue };
        let root = player.mul(&root_t);
        let mut global: Vec<Iso> = Vec::with_capacity(rig.joints.len());
        for (i, e) in rig.joints.iter().enumerate() {
            let l = joints.get(*e).copied().unwrap_or_default();
            let parent = rig.parents[i].map(|p| global[p]).unwrap_or(root);
            global.push(parent.mul(&l));
        }

        // ---------------------------------------------------------------- limb goals
        let n = targets.normal;
        let wanted = [
            targets.hands.map(|h| h.0),
            targets.hands.map(|h| h.1),
            targets.feet.map(|f| f.0),
            targets.feet.map(|f| f.1),
        ];
        let mut goals: [Option<(Vec3, f32)>; 4] = [None; 4];
        for i in 0..4 {
            let anim_contact = contact_of(i, global[chains[i][2]].pos, n);
            // the clip's contact tags decide attach / travel / release when the clip has them (game);
            // otherwise the context's timing is used
            let g = match limb_tag(&contacts, i) {
                Some(tag) => ik.limbs[i].update_tagged(wanted[i], anim_contact, tag, dt),
                None => ik.limbs[i].update(wanted[i], anim_contact, targets.transit, dt),
            };
            goals[i] = g.map(|g| (g, ik.limbs[i].weight));
        }
        if goals.iter().all(|g| g.is_none()) {
            ik.fit = Vec3::ZERO;
            continue;
        }

        // ---------------------------------------------------------------- contact fit (+ reach pull)
        // weighted mean of (goal − animated end bone); dividing by max(Σw, 1) fades it with the weights
        let mut sum = Vec3::ZERO;
        let mut wsum = 0.0;
        for (limb, goal) in goals.iter().enumerate() {
            let Some((contact, w)) = *goal else { continue };
            sum += (effector_goal(limb, contact, n) - global[chains[limb][2]].pos) * w;
            wsum += w;
        }
        let mut offset = (sum / wsum.max(1.0)).clamp_length_max(IK_FIT_MAX);
        // hands still out of reach pull the body further (a few passes: one hand's pull can move the
        // other shoulder away)
        for _ in 0..4 {
            let mut step = Vec3::ZERO;
            for limb in 0..2 {
                let Some((contact, w)) = goals[limb] else { continue };
                let [ia, ib, ic] = chains[limb];
                let reach = (global[ib].pos - global[ia].pos).length() + (global[ic].pos - global[ib].pos).length();
                let to = effector_goal(limb, contact, n) - (global[ia].pos + offset);
                let short = to.length() - reach * IK_REACH_FRACTION;
                if short * w > step.length() {
                    step = to.normalize() * short * w;
                }
            }
            offset += step;
        }
        // PORT: the whole-body adjustment never exceeds the reach of a limb
        let offset = offset.clamp_length_max(IK_FIT_MAX * 2.0);
        ik.fit = offset;
        if offset != Vec3::ZERO {
            for g in &mut global {
                g.pos += offset;
            }
            if let Ok(mut t) = joints.get_mut(rig.root) {
                t.translation += player.rot.inverse() * offset;
            }
        }

        // ---------------------------------------------------------------- per-limb two-bone solve
        let forward = body_forward(body.heading);
        for (limb, goal) in goals.iter().enumerate() {
            let Some((contact, w)) = *goal else { continue };
            let [ia, ib, ic] = chains[limb];
            let (a, b, c) = (global[ia].pos, global[ib].pos, global[ic].pos);
            let target = c.lerp(effector_goal(limb, contact, n), w);
            // elbows bend back, knees forward (used only when the limb is straight)
            let bend_hint = if limb < 2 { -forward } else { forward };
            let (da, db) = two_bone(a, b, c, target, bend_hint);
            let parent_a = rig.parents[ia].map(|p| global[p].rot).unwrap_or(root.rot);
            let parent_b = rig.parents[ib].map(|p| global[p].rot).unwrap_or(root.rot);
            let parent_c = rig.parents[ic].map(|p| global[p].rot).unwrap_or(root.rot);
            // new globals: A' = da·A, B' = db·B (db already includes da); everything under B moves with db
            if let Ok(mut t) = joints.get_mut(rig.joints[ia]) {
                t.rotation = (parent_a.inverse() * (da * global[ia].rot)).normalize();
            }
            if let Ok(mut t) = joints.get_mut(rig.joints[ib]) {
                t.rotation = ((da * parent_b).inverse() * (db * global[ib].rot)).normalize();
            }
            // the hand/foot keeps its animated world orientation (grip and toe placement as authored)
            if let Ok(mut t) = joints.get_mut(rig.joints[ic]) {
                let keep = global[ic].rot;
                let new_parent = db * parent_c;
                let solved = (new_parent.inverse() * keep).normalize();
                t.rotation = t.rotation.slerp(solved, w);
            }
        }
    }
}

fn body_forward(heading: f32) -> Vec3 {
    Vec3::new(-heading.sin(), 0.0, -heading.cos())
}

/// Analytic two-bone IK (law of cosines). Returns the global rotation deltas `(da, db)`: the upper
/// bone's new global rotation is `da·A`, the middle bone's is `db·B`, and the end lands on `t`
/// (clamped to reach). The bend plane of the current pose is kept.
pub fn two_bone(a: Vec3, b: Vec3, c: Vec3, t: Vec3, bend_hint: Vec3) -> (Quat, Quat) {
    let eps = 1e-4;
    let lab = (b - a).length();
    let lcb = (c - b).length();
    if lab < eps || lcb < eps {
        return (Quat::IDENTITY, Quat::IDENTITY);
    }
    let lat = (t - a).length().clamp(eps.max((lab - lcb).abs() + 1e-3), lab + lcb - 1e-3);
    let acos = |x: f32| x.clamp(-1.0, 1.0).acos();

    let ac = (c - a).normalize_or_zero();
    let ab = (b - a).normalize_or_zero();
    let at = (t - a).normalize_or_zero();
    let ac_ab_0 = acos(ac.dot(ab));
    let ba_bc_0 = acos((a - b).normalize_or_zero().dot((c - b).normalize_or_zero()));
    let ac_ab_1 = acos((lcb * lcb - lab * lab - lat * lat) / (-2.0 * lab * lat));
    let ba_bc_1 = acos((lat * lat - lab * lab - lcb * lcb) / (-2.0 * lab * lcb));

    // bend-plane normal; falls back to the hint when the limb is straight
    let mut axis0 = ac.cross(ab);
    if axis0.length_squared() < 1e-8 {
        axis0 = ac.cross((ab + bend_hint * 0.1).normalize_or_zero());
    }
    let axis0 = axis0.normalize_or_zero();
    if axis0 == Vec3::ZERO {
        return (Quat::IDENTITY, Quat::IDENTITY);
    }
    let r0 = Quat::from_axis_angle(axis0, ac_ab_1 - ac_ab_0);
    let r1 = Quat::from_axis_angle(axis0, ba_bc_1 - ba_bc_0);
    let axis1 = ac.cross(at);
    let r2 = if axis1.length_squared() < 1e-10 { Quat::IDENTITY } else { Quat::from_axis_angle(axis1.normalize(), acos(ac.dot(at))) };
    let da = r2 * r0;
    // B's new global = da · r1 · B (r1 acts about B in the original frame)
    (da, da * r1)
}

/// `AC_IK_LOG=1`: after transform propagation, log how far each weighted end bone is from its goal
/// (verifies the solve on the rendered skeleton), plus the contact-fit translation.
pub fn log_ik_error(q: Query<(&Rig, &LimbIk, &LimbTargets)>, globals: Query<&GlobalTransform>, mut frame: Local<u32>) {
    *frame += 1;
    let every: u32 = std::env::var("AC_IK_LOG").ok().and_then(|v| v.parse().ok()).filter(|n| *n > 1).unwrap_or(30);
    if *frame % every != 0 {
        return;
    }
    for (rig, ik, targets) in &q {
        let Some(chains) = ik.chains.filter(|c| c.iter().all(|l| l[0] != usize::MAX)) else { continue };
        let mut parts = Vec::new();
        for (i, l) in ik.limbs.iter().enumerate() {
            if l.weight <= 0.0 {
                continue;
            }
            let Ok(g) = globals.get(rig.joints[chains[i][2]]) else { continue };
            let want = effector_goal(i, l.goal, targets.normal);
            parts.push(format!("{}={:.3}m(w{:.2})", ["LH", "RH", "LF", "RF"][i], (g.translation() - want).length(), l.weight));
        }
        if !parts.is_empty() {
            info!("ik error: {} | fit {:.3}m", parts.join(" "), ik.fit.length());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forward kinematics of the solved chain must reach the target.
    fn check(a: Vec3, b: Vec3, c: Vec3, t: Vec3) -> f32 {
        let (da, db) = two_bone(a, b, c, t, Vec3::Z);
        let b2 = a + da * (b - a);
        let c2 = b2 + db * (c - b);
        (c2 - t).length()
    }

    #[test]
    fn two_bone_reaches_targets() {
        let (a, b, c) = (Vec3::ZERO, Vec3::new(0.0, -0.3, 0.05), Vec3::new(0.0, -0.58, 0.0));
        for t in [Vec3::new(0.2, -0.3, 0.2), Vec3::new(-0.1, 0.4, 0.1), Vec3::new(0.0, -0.2, 0.3)] {
            assert!(check(a, b, c, t) < 1e-3, "target {t} missed");
        }
        // straight chain uses the hint
        assert!(check(Vec3::ZERO, Vec3::new(0.0, -0.3, 0.0), Vec3::new(0.0, -0.6, 0.0), Vec3::new(0.0, -0.4, 0.1)) < 1e-3);
        // out of reach: the chain straightens towards the target
        let t = Vec3::new(0.0, -2.0, 0.0);
        let (da, db) = two_bone(a, b, c, t, Vec3::Z);
        let c2 = a + da * (b - a) + db * (c - b);
        assert!(c2.normalize().dot(t.normalize()) > 0.999);
    }

    #[test]
    fn limb_weight_and_travel_follow_the_game_rates() {
        let mut l = LimbState::default();
        let dt = 1.0 / 60.0;
        let mut frames = 0;
        while l.weight < 1.0 {
            l.update(Some(Vec3::ZERO), Vec3::ZERO, 0.5, dt);
            frames += 1;
        }
        assert!((15..=16).contains(&frames), "{frames}"); // 0.25 s at 4/s
        // retarget: the goal follows the clip's own limb motion, corrected linearly onto the new hold
        let mut anim = Vec3::ZERO;
        let mut mid = Vec3::ZERO;
        for k in 0..30 {
            anim += Vec3::new(0.0, 0.02, 0.0); // the clip lifts the limb
            let g = l.update(Some(Vec3::X), anim, 0.5, dt).unwrap();
            if k == 14 {
                mid = g;
            }
        }
        assert!((l.goal - Vec3::X).length() < 1e-5);
        assert!(mid.y > 0.1, "follows the clip's arc mid-way: {mid}");
        let mut frames = 0;
        while l.update(None, Vec3::ZERO, 0.5, dt).is_some() {
            frames += 1;
        }
        assert!((11..=12).contains(&frames), "{frames}"); // 0.2 s at 5/s
    }
}
