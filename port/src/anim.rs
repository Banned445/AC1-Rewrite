//! Animation playback on Altaïr's rig.
//!
//! Clips are decoded from the user's own install (RE/10): per-bone local rotation/translation keys
//! (keyed by BoneID), the root DISPLACEMENT track (root motion) and the ACUATORCONTACTS track (limb
//! contact tags). Clips are chosen through the game's **animation graph** (RE/13): the contexts name an
//! *action* by the id the exe uses; an action is a sequence of *items*, each a weighted blend of clips
//! with its own blend time and displacement mode. While a clip plays the rig root converts animation
//! space (Z-up, facing +Y, feet at 0) to Bevy space.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::assets::ac_actions::{ActionGraph, DisplacementMode};
use crate::assets::anims::load_locomotion;
use crate::assets::game_dir;
use crate::model::Rig;
use crate::player::move_blend::ACT_GROUND_LOCOMOTION;
use crate::player::{ground::speed_band, ground::SpeedBand, ActorContextId, HumanDataBundle, Locomotion};

/// A decoded animation clip (Bevy types).
#[derive(Clone, Debug, Default)]
pub struct AnimClip {
    pub name: String,
    pub duration: f32,
    pub rotations: HashMap<u32, Vec<(f32, Quat)>>,
    pub translations: HashMap<u32, Vec<(f32, Vec3)>>,
    /// Root-motion speed (m/s) from the DISPLACEMENT track.
    pub root_speed: f32,
    /// ACUATORCONTACTS keys (bitmask held until the next key; bits = `AcuatorsContactsTypes`).
    pub contacts: Vec<(f32, u8)>,
}

fn find_span<T>(keys: &[(f32, T)], t: f32) -> (usize, usize, f32) {
    if keys.len() == 1 || t <= keys[0].0 {
        return (0, 0, 0.0);
    }
    let last = keys.len() - 1;
    if t >= keys[last].0 {
        return (last, last, 0.0);
    }
    let i = keys.partition_point(|k| k.0 <= t) - 1;
    let (t0, t1) = (keys[i].0, keys[i + 1].0);
    (i, i + 1, if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 })
}

/// Quaternion interpolation as the game does it (0x4916F0): shortest arc; nlerp when the keys are
/// close (dot ≥ 0.98936), slerp otherwise.
fn qinterp(a: Quat, b: Quat, s: f32) -> Quat {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    if a.dot(b) >= 0.989_355_7 { a.lerp(b, s).normalize() } else { a.slerp(b, s) }
}

pub fn sample_rot(keys: &[(f32, Quat)], t: f32) -> Quat {
    let (a, b, s) = find_span(keys, t);
    qinterp(keys[a].1, keys[b].1, s)
}

pub fn sample_vec(keys: &[(f32, Vec3)], t: f32) -> Vec3 {
    let (a, b, s) = find_span(keys, t);
    keys[a].1.lerp(keys[b].1, s)
}

/// Contact bits at time `t` (keys are held: ByteLowest).
pub fn contact_bits(keys: &[(f32, u8)], t: f32) -> u8 {
    if keys.is_empty() {
        return 0;
    }
    let i = keys.partition_point(|k| k.0 <= t + 1e-4);
    keys[i.saturating_sub(1)].1
}

#[derive(Resource, Default)]
pub struct AnimLibrary {
    pub clips: HashMap<String, AnimClip>,
    pub graph: ActionGraph,
    /// Animation resource id → resource (clip) name.
    pub anim_names: HashMap<u32, String>,
    pub status: String,
}

/// One playing item: clips with weights, the blend time into it, and whether the clip's root motion
/// shapes the path (`ACTDisplacementMode::FromAnim`).
#[derive(Clone, Debug, Default)]
pub struct ItemPlay {
    pub layers: Vec<(String, f32)>,
    pub blend: f32,
    pub root_motion: bool,
}

impl AnimLibrary {
    /// The items of a graph action with its authored default weights (layers whose clip is not loaded
    /// are dropped).
    pub fn action_items(&self, id: u32) -> Option<Vec<ItemPlay>> {
        let a = self.graph.actions.get(&id)?;
        let items: Vec<ItemPlay> = a
            .items
            .iter()
            .map(|it| {
                let mut layers = Vec::new();
                for (k, anim) in it.animations.iter().enumerate() {
                    let w = it.weights.get(k).copied().unwrap_or(if k == 0 { 1.0 } else { 0.0 });
                    if let Some(n) = self.anim_names.get(anim).filter(|n| self.clips.contains_key(*n)) {
                        layers.push((n.clone(), w));
                    }
                }
                if layers.iter().all(|l| l.1 <= 0.0) {
                    if let Some(l) = layers.first_mut() {
                        l.1 = 1.0;
                    }
                }
                ItemPlay { layers, blend: it.blend.time, root_motion: it.displacement == DisplacementMode::FromAnim }
            })
            .filter(|i| !i.layers.is_empty())
            .collect();
        (!items.is_empty()).then_some(items)
    }

    /// First action of `block` whose first item plays clip `clip` (for actions whose id is not yet
    /// traced to the exe; RE/13 lists which ones).
    pub fn action_with_clip(&self, block: &str, clip: &str) -> Option<u32> {
        self.graph
            .actions
            .values()
            .filter(|a| a.block == block)
            .find(|a| a.items.first().is_some_and(|it| it.animations.first().and_then(|id| self.anim_names.get(id)).is_some_and(|n| n == clip)))
            .map(|a| a.id)
    }

    fn item_duration(&self, item: &ItemPlay) -> f32 {
        let (mut d, mut w) = (0.0, 0.0);
        for (n, wt) in &item.layers {
            if let Some(c) = self.clips.get(n) {
                d += c.duration * wt;
                w += wt;
            }
        }
        if w > 0.0 { d / w } else { 0.0 }
    }

    fn dominant<'a>(&'a self, item: &ItemPlay) -> Option<&'a AnimClip> {
        item.layers.iter().max_by(|a, b| a.1.total_cmp(&b.1)).and_then(|(n, _)| self.clips.get(n))
    }
}

/// Contact state of the playing item (dominant clip), for the limb IK (RE/11 §2, RE/13 §5).
#[derive(Clone, Copy, Debug, Default)]
pub struct ContactState {
    /// The clip has a contact track.
    pub tagged: bool,
    pub bits: u8,
    /// Seconds into the dominant clip.
    pub t: f32,
    /// Per bit: time the bit last switched off (≤ t), if it is off.
    pub off_since: [Option<f32>; 8],
    /// Per bit: next time (> t) the bit switches on, if it is off.
    pub next_on: [Option<f32>; 8],
}

fn contact_state(c: &AnimClip, t: f32) -> ContactState {
    let mut s = ContactState { tagged: !c.contacts.is_empty(), t, ..default() };
    if !s.tagged {
        return s;
    }
    s.bits = contact_bits(&c.contacts, t);
    for b in 0..8 {
        if s.bits >> b & 1 != 0 {
            continue;
        }
        let mut off = 0.0;
        for (kt, v) in &c.contacts {
            if *kt > t + 1e-4 {
                break;
            }
            if v >> b & 1 == 0 && (off == 0.0 || contact_bits(&c.contacts, kt - 1e-3) >> b & 1 != 0) {
                off = *kt;
            }
        }
        s.off_since[b] = Some(off);
        s.next_on[b] = c.contacts.iter().find(|(kt, v)| *kt > t + 1e-4 && v >> b & 1 != 0).map(|k| k.0);
    }
    s
}

/// Playback state on the player.
#[derive(Component, Default)]
pub struct AnimPlayer {
    /// Identity of what is playing (clip name or `act_xxxxxxxx`).
    pub clip: Option<String>,
    pub items: Vec<ItemPlay>,
    pub item: usize,
    pub phase: f32,
    pub prev: Option<(Vec<(String, f32)>, f32)>,
    pub fade: f32,
    /// Crossfade length for the current transition (s).
    pub fade_time: f32,
    /// Looping, or one-shot (the last item holds its last frame).
    pub looping: bool,
    /// One-shot stretched to this many seconds (e.g. a jump fitted to the jump's duration).
    pub fit: Option<f32>,
    /// Restart key: a new token restarts the action even if it is the same one.
    pub token: u64,
    /// A one-shot that must finish before normal selection resumes in this context.
    pub hold: Option<ActorContextId>,
    /// Contact tags of the current frame (written by `apply_clip`).
    pub contacts: ContactState,
    seen_landing: u32,
    /// InAir entry the fall-entry clip was started for.
    seen_fall: Option<u32>,
    /// Smoothed grasp direction while falling with grab held (HumanInAir+48, 7/s).
    grasp_dir: Vec3,
    /// Ledge grab whose reception has been played.
    caught: Option<u64>,
    /// Phase set by the simulation (the ground step cycle, `MoveBlend`): the player shows it instead of
    /// advancing its own clock.
    sim_phase: Option<f32>,
}

/// What the selector wants playing.
struct Request {
    key: String,
    items: Vec<ItemPlay>,
    looping: bool,
    fit: Option<f32>,
    token: u64,
    fade: f32,
    hold: bool,
}

const CROSSFADE: f32 = 0.2;

pub struct AnimPlugin;

impl Plugin for AnimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnimLibrary>()
            .add_systems(Startup, load_library)
            .add_systems(Update, (choose_clip, apply_clip).chain().after(crate::player::ledge::update_ledge).after(crate::player::climb::update_climb));
    }
}

/// Named one-shots whose DISPLACEMENT track shapes the body's path (the graph's FromAnim mode is used for
/// actions).
pub fn uses_root_motion(name: &str) -> bool {
    name.starts_with("shimmy_") || name.starts_with("pullup_") || name.starts_with("catch_")
}

fn single(name: &str) -> Vec<ItemPlay> {
    vec![ItemPlay { layers: vec![(name.to_string(), 1.0)], blend: CROSSFADE, root_motion: uses_root_motion(name) }]
}

fn looped(clip: &str, fade: f32) -> Request {
    Request { key: clip.into(), items: single(clip), looping: true, fit: None, token: 0, fade, hold: false }
}

fn once(clip: String, token: u64, fit: Option<f32>, fade: f32) -> Request {
    Request { items: single(&clip), key: clip, looping: false, fit, token, fade, hold: false }
}

/// A graph action (by the id the exe uses). Falls back to `fallback` (a clip name) if the action or its
/// clips are missing. The fade is the first item's authored blend time unless `fade` is given.
fn action(lib: &AnimLibrary, ids: &[u32], looping: bool, token: u64, fade: Option<f32>, fallback: Option<&str>) -> Option<Request> {
    let mut items = Vec::new();
    for id in ids {
        items.extend(lib.action_items(*id)?);
    }
    if items.is_empty() {
        return fallback.map(|f| Request { looping, token, ..once(f.to_string(), token, None, fade.unwrap_or(CROSSFADE)) });
    }
    let fade = fade.unwrap_or(items[0].blend.max(0.05));
    let key = ids.iter().map(|i| format!("act_{i:08x}")).collect::<Vec<_>>().join("+");
    Some(Request { key, items, looping, fit: None, token, fade, hold: false })
}

/// Fall-grasp weights (HumanInAir__CheckAirCatch 0xE0BF2B–0xE0C2E9): with grab held the reach direction
/// (stick, else facing) is smoothed at 7/s; `w` = its length (≤ 1); the static fall pose gets 1 − w and
/// the rest goes to front / left / right / back-left / back-right by the signed angle to the facing, in
/// 90° sectors. Clip order in action 0x1F0C22C2: falling01, front, left, right, backleft, backright.
pub fn fall_grasp_weights(smoothed: Vec3, facing: Vec3) -> [f32; 6] {
    let mut w6 = [0.0; 6];
    let flat = Vec3::new(smoothed.x, 0.0, smoothed.z);
    let w = flat.length().clamp(0.0, 1.0);
    w6[0] = 1.0 - w;
    if w < 5e-4 {
        return w6;
    }
    let d = flat.normalize();
    // signed angle from facing; positive = to the right (game: left/right from sub_55E570)
    let right = crate::player::right_of(facing);
    let a = d.dot(right).atan2(d.dot(facing));
    let q = std::f32::consts::FRAC_PI_2;
    if a >= 0.0 {
        if a <= q {
            w6[1] = (1.0 - a / q) * w;
            w6[3] = a / q * w;
        } else {
            let b = (a - q) / q;
            w6[3] = (1.0 - b) * w;
            w6[5] = b * w;
        }
    } else {
        let a = -a;
        if a <= q {
            w6[1] = (1.0 - a / q) * w;
            w6[2] = a / q * w;
        } else {
            let b = (a - q) / q;
            w6[2] = (1.0 - b) * w;
            w6[4] = b * w;
        }
    }
    w6
}

// Action ids used by the exe (RE/13 §4). Ledge tables 0x1A2C490.. (shimmy), 0x1A2C4C0.. (vertical steps).
const ACT_HANG_WALL: u32 = 0x0106_F2E8;
const ACT_HANG_FREE: u32 = 0x0127_19F1;
const ACT_HANG_WALLFREE: u32 = 0x0106_F2E9;
const ACT_PULLUP_WALL: u32 = 0x0106_D2C5;
const ACT_PULLUP_FREE: u32 = 0x0127_19F2;
/// Pull-up outcome "stand": hangknee → wait (transition target of the pull-up's last item).
const ACT_HANGKNEE_TO_WAIT: u32 = 0x0106_C58B;
/// [wall, free] × [left open, left close, right open, right close].
const ACT_SHIMMY: [[u32; 4]; 2] = [[0x01B7_0B35, 0x01B7_0B36, 0x01B7_0B37, 0x01B7_0B38], [0x01A2_490A, 0x01A2_490B, 0x01A2_490C, 0x01A2_490D]];
/// [wall, free] × [up: 1m_u_1lu, 1m_u_1ru, 1lu_u_1m, 1ru_u_1m, down: 1m_d_1lu, 1m_d_1ru, 1lu_d_1m, 1ru_d_1m].
const ACT_VSTEP: [[u32; 8]; 2] = [
    [0x01B7_0FAA, 0x01B7_0FAB, 0x01B7_0FAC, 0x01B7_0FAD, 0x01B7_0FAE, 0x01B7_0FAF, 0x01B7_0FB0, 0x01B7_0FB1],
    [0x01A2_79B9, 0x01A2_79BA, 0x01A2_79BB, 0x01A2_79BC, 0x01A2_79BD, 0x01A2_79BE, 0x01A2_79BF, 0x01A2_79C0],
];
/// Catches (CheckAirCatch): ledge with wall below (3-way angle blend), free hang; [< 3 m, ≥ 3 m].
const ACT_CATCH_WALL: [u32; 2] = [0x1F0C_0C23, 0x1F0C_0C2D];
const ACT_CATCH_FREE: [u32; 2] = [0x1F0C_2EB8, 0x1F0C_2EB9];
/// Falling (6-way grasp blend).
const ACT_FALL: u32 = 0x1F0C_22C2;

/// One item of a graph action with the sim's weights, if all its clips are loaded. Root motion is off:
/// the sim already moves the body along it.
fn sim_item(lib: &AnimLibrary, b: &crate::player::jump_blend::ActionBlend) -> Option<ItemPlay> {
    let items = lib.action_items(b.id)?;
    let mut it = items.get(b.item)?.clone();
    if it.layers.len() != b.n {
        return None;
    }
    for (k, l) in it.layers.iter_mut().enumerate() {
        l.1 = b.w[k];
    }
    it.root_motion = false;
    Some(it)
}

/// Play `b` (phase from the sim): update the weights in place if it is already playing.
fn sim_request(p: &mut AnimPlayer, lib: &AnimLibrary, b: &crate::player::jump_blend::ActionBlend, token: u64, fade: f32) -> Option<Request> {
    let it = sim_item(lib, b)?;
    let key = format!("act_{:08x}", b.id);
    if p.clip.as_deref() == Some(key.as_str()) && p.token == token {
        p.items = vec![it];
        p.item = 0;
        return None;
    }
    Some(Request { key, items: vec![it], looping: false, fit: None, token, fade, hold: false })
}

/// Both items (footl, footr) of the ground locomotion action with all 17 clips loaded.
fn ground_items(lib: &AnimLibrary) -> Option<Vec<ItemPlay>> {
    lib.action_items(ACT_GROUND_LOCOMOTION).filter(|items| items.len() == 2 && items.iter().all(|i| i.layers.len() == 17))
}

/// Pick what plays for the current context.
fn choose_clip(
    time: Res<Time>,
    lib: Res<AnimLibrary>,
    pad: Res<crate::input::PadInput>,
    collision: Res<crate::collision::CollisionWorld>,
    mut q: Query<(&Locomotion, &HumanDataBundle, &crate::player::Body, &mut AnimPlayer)>,
) {
    use crate::player::air::{AirMode, FallOrigin, LandingType};
    use crate::player::ledge::{LedgeHangType, LedgeSubState};
    let dt = time.delta_secs();
    for (loco, data, body, mut p) in &mut q {
        let g = &data.ground;
        p.sim_phase = None;
        if let Some(ctx) = p.hold {
            if ctx == loco.current && !(p.phase >= 1.0 && p.item + 1 >= p.items.len()) {
                continue;
            }
            p.hold = None;
        }
        let req = match loco.current {
            // landing / free-step reception action (0xE05940 / 0xE07D00), shown at the sim's phase
            ActorContextId::Ground if g.oneshot.is_some_and(|os| sim_item(&lib, &os.blend).is_some()) => {
                let os = g.oneshot.unwrap();
                p.seen_landing = g.landing_seq;
                p.sim_phase = Some((os.t / os.duration.max(1e-4)).min(1.0));
                sim_request(&mut p, &lib, &os.blend, 1_000_000 + g.landing_seq as u64, 0.1)
            }
            // obstacle collision / lean (0xD9CB90 / 0xD9DA20): its action at the sim's phase
            ActorContextId::Ground if g.collide.is_some_and(|c| sim_item(&lib, &c.action).is_some()) => {
                let c = g.collide.unwrap();
                let (b, ph) = c.current();
                p.sim_phase = Some(ph);
                sim_request(&mut p, &lib, &b, 1_500_000 + c.seq as u64, 0.1)
            }
            // look-down at an edge (0xD9FC80)
            ActorContextId::Ground if g.look_down.is_some_and(|l| sim_item(&lib, &l.action).is_some()) => {
                let l = g.look_down.unwrap();
                p.sim_phase = Some((l.t / l.action.duration().max(1e-4)).fract());
                sim_request(&mut p, &lib, &l.action, 1_600_000 + g.pose_seq as u64, 0.3)
            }
            ActorContextId::Ground if g.landing_seq != p.seen_landing => {
                p.seen_landing = g.landing_seq;
                let clip = match g.last_landing {
                    Some(l) if l.roll => "roll",
                    Some(l) if l.kind == LandingType::HeavyDamage => "land_hard",
                    _ => "land_soft",
                };
                Some(Request { hold: true, ..once(clip.into(), 1_000_000 + g.landing_seq as u64, None, 0.08) })
            }
            // after a pull-up the action ends standing: let it finish before idling
            ActorContextId::Ground if p.clip.as_deref().is_some_and(|c| c.starts_with(&format!("act_{ACT_PULLUP_WALL:08x}")) || c.starts_with(&format!("act_{ACT_PULLUP_FREE:08x}"))) && !(p.phase >= 1.0 && p.item + 1 >= p.items.len()) => continue,
            // moving: the game's locomotion action 0x05923BDB, item of the leading foot, MoveBlend's 17 weights
            ActorContextId::Ground if g.speed_param > 0.0 && ground_items(&lib).is_some() => {
                let mut it = ground_items(&lib).unwrap()[g.blend.foot].clone();
                for (k, l) in it.layers.iter_mut().enumerate() {
                    l.1 = g.blend.weights[k];
                }
                p.sim_phase = Some(g.blend.phase);
                let key = format!("act_{ACT_GROUND_LOCOMOTION:08x}");
                if p.clip.as_deref() == Some(key.as_str()) {
                    p.items = vec![it];
                    p.item = 0;
                    continue;
                }
                Some(Request { key, items: vec![it], looping: true, fit: None, token: 0, fade: CROSSFADE, hold: false })
            }
            ActorContextId::Ground => Some(looped(
                match speed_band(g.speed_param) {
                    // wait item of the leading foot (MoveBlend foot: 0 = left ahead)
                    SpeedBand::None => match (g.high_profile, g.blend.foot == 0) {
                        (true, true) => "idle_high",
                        (true, false) => "idle_high_r",
                        (false, true) => "idle_low",
                        (false, false) => "idle_low_r",
                    },
                    SpeedBand::Walk => "walk",
                    SpeedBand::Jog => "jog",
                    SpeedBand::Run => "run",
                    SpeedBand::Sprint => "sprint",
                },
                CROSSFADE,
            )),
            ActorContextId::InAir => match data.air.mode {
                // the game's takeoff then flight item (0xB20200), at the sim's time
                AirMode::Jump { real: true, t, t_takeoff, duration, .. } if data.air.flight.is_some_and(|b| sim_item(&lib, &b).is_some()) => {
                    let (b, ph) = if t < t_takeoff && data.air.takeoff.is_some() {
                        (data.air.takeoff.unwrap(), t / t_takeoff.max(1e-4))
                    } else {
                        (data.air.flight.unwrap(), (t - t_takeoff) / (duration - t_takeoff).max(1e-4))
                    };
                    p.sim_phase = Some(ph.min(1.0));
                    sim_request(&mut p, &lib, &b, 2_000_000 + data.air.seq as u64, 0.05)
                }
                // the takeoff/flight clip is stretched over the target-warped jump (RE/04)
                AirMode::Jump { duration, .. } => {
                    let clip = match data.air.target.and_then(|t| t.hang) {
                        Some((pt, n)) if crate::player::ledge::hang_type_at(pt, n, &collision) == LedgeHangType::Wall => "jump_hangwall",
                        Some(_) => "jump_hangfree",
                        None => "jump",
                    };
                    Some(once(clip.into(), 2_000_000 + data.air.seq as u64, Some(duration), 0.1))
                }
                // Leap of Faith free-fall tail: `faith_jump_fall` (0xB1EC40 third action)
                AirMode::Fall { .. } if data.air.flight.is_some_and(|f| f.id == crate::player::jump_blend::FLIGHT_FAITH) => {
                    let b = crate::player::jump_blend::ActionBlend::new(crate::player::jump_blend::FALL_FAITH, 0, &[1.0]);
                    p.sim_phase = Some(0.5);
                    sim_request(&mut p, &lib, &b, 2_500_000 + data.air.seq as u64, 0.2)
                }
                // the jump's own fall action (InAir +416, e.g. `beam_jumpstraight_clear_tr_fall`)
                AirMode::Fall { .. } if data.air.fall_action.is_some_and(|b| sim_item(&lib, &b).is_some()) => {
                    let b = data.air.fall_action.unwrap();
                    p.sim_phase = Some((data.air.fall_t / b.duration().max(1e-4)).min(1.0));
                    sim_request(&mut p, &lib, &b, 2_600_000 + data.air.seq as u64, 0.1)
                }
                _ => {
                    if p.seen_fall != Some(data.air.seq) {
                        p.seen_fall = Some(data.air.seq);
                        p.grasp_dir = Vec3::ZERO;
                        let speed = Vec2::new(body.velocity.x, body.velocity.z).length();
                        let entry = match data.air.fall_origin {
                            FallOrigin::HangFree => "hangfree_to_fall",
                            FallOrigin::HangWall | FallOrigin::Climb => "hangwall_to_fall",
                            FallOrigin::Ground if p.clip.as_deref() == Some("jump") => "jump_to_fall",
                            // fast fall types (odd / 6) at a horizontal speed ≥ 2.5 m/s (0xD8C380); the type → clip
                            // mapping (probe vt112) is not traced, so the clip is still chosen by name
                            FallOrigin::Ground if speed >= 2.5 => "run_to_fall",
                            FallOrigin::Ground => "walk_to_fall",
                        };
                        Some(once(entry.into(), 4_000_000 + data.air.seq as u64, None, 0.12))
                    } else if p.clip.as_deref().is_some_and(|c| c.ends_with("_to_fall")) && p.phase < 1.0 {
                        continue;
                    } else {
                        // the falling blend: grasp weights only while the grab input (Legs) is held
                        let facing = body.forward();
                        let want = if pad.legs_held { if pad.speed01 > 0.0 { pad.dir } else { facing } } else { Vec3::ZERO };
                        let k = (dt * 7.0).min(1.0);
                        let gd = p.grasp_dir;
                        p.grasp_dir = gd + (want - gd) * k;
                        let w = fall_grasp_weights(p.grasp_dir, facing);
                        let r = action(&lib, &[ACT_FALL], true, 0, Some(0.15), Some("fall"));
                        if let Some(mut r) = r {
                            if let Some(it) = r.items.first_mut() {
                                if it.layers.len() == 6 {
                                    for (k, l) in it.layers.iter_mut().enumerate() {
                                        l.1 = w[k];
                                    }
                                }
                            }
                            // same action: just update the weights
                            if p.clip.as_deref() == Some(r.key.as_str()) {
                                p.items = r.items;
                                continue;
                            }
                            Some(r)
                        } else {
                            None
                        }
                    }
                }
            },
            // beam (HumanNarrowObjectBeam): the state's action at the sim's phase
            ActorContextId::NarrowObject if data.narrow.current().is_some_and(|(b, _)| sim_item(&lib, &b).is_some()) => {
                let (b, ph) = data.narrow.current().unwrap();
                p.sim_phase = Some(ph);
                sim_request(&mut p, &lib, &b, 9_000_000 + data.narrow.seq as u64, 0.12)
            }
            // wall run (0xE37590): the sub-state's action at the sim's phase
            ActorContextId::Walling if data.walling.current().is_some_and(|(b, _)| sim_item(&lib, &b).is_some()) => {
                let (b, ph) = data.walling.current().unwrap();
                p.sim_phase = Some(ph);
                sim_request(&mut p, &lib, &b, 8_000_000 + data.walling.seq as u64 * 2 + (b.id == crate::player::walling::VERTICAL_END && b.item == 1) as u64, 0.08)
            }
            // haystack (0xE43140): entry action, then the wait, at the sim's time
            ActorContextId::HayStack if data.hay.action.is_some_and(|b| sim_item(&lib, &b).is_some()) => {
                let b = data.hay.action.unwrap();
                let ph = data.hay.t / b.duration().max(1e-4);
                p.sim_phase = Some(if data.hay.phase == crate::player::hay::HayPhase::Waiting { ph.fract() } else { ph.min(1.0) });
                sim_request(&mut p, &lib, &b, 5_000_000 + data.hay.seq as u64, 0.2)
            }
            // corner turn / ledge jump / hop up (`ledge_moves`): its current action at the sim's phase
            // ladder (0xE27D30): the table's action for the state, at the sim's phase
            ActorContextId::Ladder if data.ladder.current().is_some_and(|(b, _)| sim_item(&lib, &b).is_some()) => {
                let (b, ph) = data.ladder.current().unwrap();
                p.sim_phase = Some(ph);
                sim_request(&mut p, &lib, &b, 7_700_000 + data.ladder.seq as u64, 0.1)
            }
            // swinging on a bar (0xDD24F0): landing, swing cycle, stops / impacts
            ActorContextId::Ledge if data.ledge.swing.is_some_and(|s| sim_item(&lib, &s.action).is_some()) => {
                let s = data.ledge.swing.unwrap();
                let (b, ph) = s.current();
                p.sim_phase = Some(ph);
                sim_request(&mut p, &lib, &b, 7_600_000 + s.seq as u64, 0.08)
            }
            // pass-over (0xE07D00 case 2 / 0xDDB800): the reception, then the vault, at the sim's phase
            ActorContextId::Ledge if data.ledge.pass_over.is_some_and(|p| sim_item(&lib, &p.action).is_some()) => {
                let po = data.ledge.pass_over.unwrap();
                p.sim_phase = Some((po.t / po.action.duration().max(1e-4)).min(1.0));
                sim_request(&mut p, &lib, &po.action, 7_500_000 + po.seq as u64, 0.06)
            }
            ActorContextId::Ledge if data.ledge.mv.and_then(|m| m.current()).is_some_and(|(b, _)| sim_item(&lib, &b).is_some()) => {
                let (b, ph) = data.ledge.mv.unwrap().current().unwrap();
                p.sim_phase = Some(ph);
                sim_request(&mut p, &lib, &b, 7_000_000 + data.ledge.step_seq as u64 * 4 + b.item as u64, 0.08)
            }
            ActorContextId::Ledge => {
                let l = &data.ledge;
                let wall = l.hang_type == LedgeHangType::Wall;
                let wi = if wall { 0 } else { 1 };
                let moving = matches!(l.sub_state, LedgeSubState::HandPlacement | LedgeSubState::Pullup);
                let token = 3_000_000 + l.step_seq as u64;
                match l.last_action {
                    // caught from the air (CheckAirCatch): the game's reception action
                    // (one reception per grab: once it has played the hang idle follows)
                    "grab" if loco.previous == ActorContextId::InAir && l.moving() && p.caught != Some(token) => {
                        p.caught = Some(token);
                        let r = match p.clip.as_deref() {
                            // jumped at the ledge: the reception that matches the jump-into-hang clip
                            Some("jump_hangwall") => Some(once("catch_jump_wall".into(), token, None, 0.06)),
                            Some("jump_hangfree") => Some(once("catch_jump_free".into(), token, None, 0.06)),
                            _ => {
                                let hi = data.air.long_catch as usize;
                                let mut r = action(&lib, &[if wall { ACT_CATCH_WALL[hi] } else { ACT_CATCH_FREE[hi] }], false, token, None, Some(if wall { "catch_wall" } else { "catch_free" }));
                                // CheckAirCatch snaps the 3-way wall catch (straight / 30° out / 45° in) to the
                                // dominant angle class; the port's ledges are straight
                                if let Some(r) = r.as_mut().filter(|_| wall) {
                                    for it in &mut r.items {
                                        for (k, l) in it.layers.iter_mut().enumerate() {
                                            l.1 = if k == 0 { 1.0 } else { 0.0 };
                                        }
                                    }
                                }
                                r
                            }
                        };
                        r.map(|r| Request { hold: true, ..r })
                    }
                    a if moving && a.starts_with("shimmy ") => {
                        // alt flag set after the step = the lead hand reached out (open)
                        let k = match (a.ends_with("left"), l.alt_flag) {
                            (true, true) => 0,
                            (true, false) => 1,
                            (false, true) => 2,
                            (false, false) => 3,
                        };
                        action(&lib, &[ACT_SHIMMY[wi][k]], false, token, Some(0.08), None)
                    }
                    "hand step up" | "hand step down" if moving => {
                        let (first_left, second) = l.vstep.unwrap_or((true, false));
                        let down = l.last_action == "hand step down";
                        let k = (down as usize) * 4 + (second as usize) * 2 + (!first_left) as usize;
                        action(&lib, &[ACT_VSTEP[wi][k]], false, token, Some(0.1), None)
                    }
                    "jump up" if moving => Some(once("hang_up".into(), token, None, 0.1)),
                    "pull-up" if moving => {
                        let ids: Vec<u32> = if wall {
                            vec![ACT_PULLUP_WALL, ACT_HANGKNEE_TO_WAIT]
                        } else {
                            let waist = lib.action_with_clip("HumanLedge", "xx_h_hangwaist_tr_hangknee_footl");
                            [Some(ACT_PULLUP_FREE), waist, Some(ACT_HANGKNEE_TO_WAIT)].into_iter().flatten().collect()
                        };
                        action(&lib, &ids, false, token, Some(0.1), None)
                    }
                    // free hang with a wall under it = LedgeHangType WallFree: `xx_h_hangwallfree_wait` (0x0106F2E9,
                    // played after the straight jump's wall-free reception, 0xE07D00 → 0xE09055; the wait update
                    // 0xDE1FE0 plays it for type 2), legs held clear of the wall
                    _ if !wall && crate::player::ledge::wall_below_hands((l.hand_l + l.hand_r) * 0.5, l.normal, &collision) => {
                        action(&lib, &[ACT_HANG_WALLFREE], true, 0, Some(0.15), Some("hang_free"))
                    }
                    _ => action(&lib, &[if wall { ACT_HANG_WALL } else { ACT_HANG_FREE }], true, 0, Some(0.15), Some(if wall { "hang_wall" } else { "hang_free" })),
                }
            }
            ActorContextId::Climb => {
                let c = &data.climb;
                match (c.moving, c.move_action) {
                    // a grid move plays the action of its SHORT/LONG table entry
                    (Some(_), Some(id)) => action(&lib, &[id], false, 6_000_000 + c.move_seq as u64, Some(0.1), None),
                    // waiting (or entering): the pose's wait action (pose table animStateId)
                    _ => action(&lib, &[crate::player::climb::POSE_ACTIONS[c.pose]], true, 0, Some(crate::tuning::CLIMB_MOVE_TIME), None),
                }
            }
            _ => Some(looped("idle_high", CROSSFADE)),
        };
        let Some(req) = req else { continue };
        if req.items.iter().any(|it| it.layers.iter().any(|(n, _)| !lib.clips.contains_key(n))) {
            continue;
        }
        let same = p.clip.as_deref() == Some(req.key.as_str()) && p.token == req.token;
        if same {
            continue;
        }
        if p.clip.take().is_some() {
            let cur = p.items.get(p.item).map(|i| i.layers.clone()).unwrap_or_default();
            p.prev = Some((cur, p.phase));
            p.fade = 0.0;
        }
        // locomotion cycles all start on the left foot: keep the phase between gaits
        let cyclic = |n: &str| matches!(n, "walk" | "jog" | "run" | "sprint");
        let prev_cyclic = p.prev.as_ref().is_some_and(|(l, _)| l.first().is_some_and(|(n, _)| cyclic(n)));
        if !(cyclic(&req.key) && prev_cyclic) {
            p.phase = 0.0;
        }
        if std::env::var_os("AC_ANIM_LOG").is_some() {
            let layers: Vec<String> = req.items.iter().map(|i| i.layers.iter().map(|(n, w)| format!("{n}*{w:.2}")).collect::<Vec<_>>().join("+")).collect();
            info!("anim t={:.2} {:?} -> {} [{}] fade {:.2}", time.elapsed_secs(), loco.current, req.key, layers.join(" | "), req.fade);
        }
        p.clip = Some(req.key);
        p.items = req.items;
        p.item = 0;
        p.looping = req.looping;
        p.fit = req.fit;
        p.token = req.token;
        p.fade_time = req.fade.max(0.01);
        p.hold = req.hold.then_some(loco.current);
    }
}

/// Animation space (Z-up, facing +Y, feet at origin) → player-local Bevy space (Y-up, facing -Z).
fn anim_root() -> Transform {
    let m = Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y);
    Transform::from_rotation(Quat::from_mat3(&m))
}

/// Visual root offset of a root-motion clip at phase `phase` (animation space): the clip's displacement
/// minus the linear path the logical root follows.
pub fn root_motion_offset(clip: &AnimClip, phase: f32) -> Vec3 {
    let Some(keys) = clip.translations.get(&crate::assets::ac_anim::TRACK_DISPLACEMENT) else { return Vec3::ZERO };
    let start = keys.first().map(|k| k.1).unwrap_or(Vec3::ZERO);
    let end = sample_vec(keys, clip.duration) - start;
    (sample_vec(keys, phase * clip.duration) - start) - end * phase
}

/// Weighted pose of a layer set at normalised phase `phase`: per bone (rotation, translation).
fn sample_layers(lib: &AnimLibrary, rig: &Rig, layers: &[(String, f32)], phase: f32, out: &mut Vec<(Quat, Vec3)>) {
    out.clear();
    out.extend(rig.rest.iter().map(|_| (Quat::from_xyzw(0.0, 0.0, 0.0, 0.0), Vec3::ZERO)));
    let mut total = vec![0.0f32; rig.bone_ids.len()];
    let mut first: Vec<Option<Quat>> = vec![None; rig.bone_ids.len()];
    for (name, w) in layers {
        if *w <= 0.0 {
            continue;
        }
        let Some(c) = lib.clips.get(name) else { continue };
        let t = phase * c.duration;
        for (i, bone) in rig.bone_ids.iter().enumerate() {
            let r = c.rotations.get(bone).map(|k| sample_rot(k, t)).unwrap_or(rig.rest[i].rotation);
            let p = c.translations.get(bone).map(|k| sample_vec(k, t)).unwrap_or(rig.rest[i].translation);
            let r0 = *first[i].get_or_insert(r);
            let r = if r0.dot(r) < 0.0 { -r } else { r };
            let acc = &mut out[i];
            acc.0 = Quat::from_vec4(Vec4::from(acc.0) + Vec4::from(r) * *w);
            acc.1 += p * *w;
            total[i] += *w;
        }
    }
    for (i, acc) in out.iter_mut().enumerate() {
        if total[i] > 0.0 {
            acc.0 = acc.0.normalize();
            acc.1 /= total[i];
        } else {
            *acc = (rig.rest[i].rotation, rig.rest[i].translation);
        }
    }
}

pub fn apply_clip(
    time: Res<Time>,
    lib: Res<AnimLibrary>,
    mut q: Query<(&Rig, &mut AnimPlayer, &crate::player::Body)>,
    mut joints: Query<&mut Transform>,
    mut cur: Local<Vec<(Quat, Vec3)>>,
    mut prev_pose: Local<Vec<(Quat, Vec3)>>,
) {
    let dt = time.delta_secs();
    for (rig, mut p, body) in &mut q {
        if p.clip.is_none() || p.items.is_empty() {
            continue;
        }
        let item = p.items[p.item.min(p.items.len() - 1)].clone();
        let duration = lib.item_duration(&item).max(1e-3);
        let root_speed = item.layers.first().and_then(|(n, _)| lib.clips.get(n)).map(|c| c.root_speed).unwrap_or(0.0);
        // playback rate: locomotion cycles match their root motion to the actual speed (no foot
        // sliding); one-shots play at their own rate or are stretched to `fit`
        let rate = if p.looping && root_speed > 0.1 {
            let speed = Vec2::new(body.velocity.x, body.velocity.z).length();
            (speed / root_speed).clamp(0.3, 2.0)
        } else if let Some(fit) = p.fit {
            let total: f32 = p.items.iter().map(|i| lib.item_duration(i)).sum();
            total / fit.max(1e-3)
        } else {
            1.0
        };
        let next = p.phase + dt * rate / duration;
        if let Some(ph) = p.sim_phase {
            p.phase = ph;
        } else if p.looping {
            p.phase = next.fract();
        } else if next >= 1.0 && p.item + 1 < p.items.len() {
            // next item of the action's sequence, blended over its authored blend time
            p.prev = Some((item.layers.clone(), 1.0));
            p.fade = 0.0;
            p.item += 1;
            p.fade_time = p.items[p.item].blend.max(0.01);
            p.phase = 0.0;
        } else {
            p.phase = next.min(1.0);
        }
        p.fade = (p.fade + dt / p.fade_time.max(0.01)).min(1.0);
        let item = p.items[p.item].clone();

        // contact tags of the dominant clip (for the limb IK)
        p.contacts = lib.dominant(&item).map(|c| contact_state(c, p.phase * c.duration)).unwrap_or_default();

        if let Ok(mut root) = joints.get_mut(rig.root) {
            *root = anim_root();
            if !p.looping && item.root_motion {
                let mut off = Vec3::ZERO;
                let mut wsum = 0.0;
                for (n, w) in &item.layers {
                    if let Some(c) = lib.clips.get(n) {
                        off += root_motion_offset(c, p.phase) * *w;
                        wsum += *w;
                    }
                }
                if wsum > 0.0 {
                    root.translation = root.rotation * (off / wsum);
                }
            }
        }
        sample_layers(&lib, rig, &item.layers, p.phase, &mut cur);
        let fading = p.fade < 1.0 && p.prev.is_some();
        if fading {
            let (pl, pph) = p.prev.clone().unwrap();
            sample_layers(&lib, rig, &pl, pph, &mut prev_pose);
        }
        for (i, e) in rig.joints.iter().enumerate() {
            let Ok(mut tr) = joints.get_mut(*e) else { continue };
            let (mut rot, mut pos) = cur[i];
            if fading {
                rot = qinterp(prev_pose[i].0, rot, p.fade);
                pos = prev_pose[i].1.lerp(pos, p.fade);
            }
            if !(rot.is_finite() && pos.is_finite()) && std::env::var_os("AC_NAN_LOG").is_some() {
                warn!("non-finite joint {i} (bone {:08x}) clip {:?} item {} phase {:.3} fade {:.3} layers {:?}", rig.bone_ids[i], p.clip, p.item, p.phase, p.fade, item.layers);
            }
            tr.rotation = rot;
            tr.translation = pos;
        }
        if p.fade >= 1.0 {
            p.prev = None;
        }
    }
}

fn load_library(mut lib: ResMut<AnimLibrary>) {
    if std::env::var_os("AC_NO_MODEL").is_some() {
        return;
    }
    match load_locomotion(&game_dir()) {
        Ok((raw, graph, names)) => {
            for c in raw {
                let rotations = c.rotations.into_iter().map(|(k, v)| (k, v.into_iter().map(|(t, q)| (t, Quat::from_array(q).normalize())).collect())).collect();
                let translations: HashMap<u32, Vec<(f32, Vec3)>> =
                    c.translations.into_iter().map(|(k, v)| (k, v.into_iter().map(|(t, p)| (t, Vec3::from_array(p))).collect())).collect();
                let root_speed = translations
                    .get(&crate::assets::ac_anim::TRACK_DISPLACEMENT)
                    .and_then(|k| Some((k.first()?.1, k.last()?.1)))
                    .map(|(a, b)| (b - a).length() / c.duration.max(1e-3))
                    .unwrap_or(0.0);
                lib.clips.insert(c.name.clone(), AnimClip { name: c.name, duration: c.duration, rotations, translations, root_speed, contacts: c.contacts });
            }
            lib.status = format!("anims: {} clips, {} graph actions from your install", lib.clips.len(), graph.actions.len());
            lib.graph = graph;
            lib.anim_names = names;
            info!("{}", lib.status);
        }
        Err(e) => {
            lib.status = format!("anims: not loaded ({e})");
            warn!("{}", lib.status);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fall_grasp_weights_follow_check_air_catch() {
        let f = Vec3::NEG_Z;
        // no reach → the static fall pose only
        assert_eq!(fall_grasp_weights(Vec3::ZERO, f), [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        // full reach straight ahead → grasp front
        let w = fall_grasp_weights(f, f);
        assert!(w[0].abs() < 1e-5 && (w[1] - 1.0).abs() < 1e-5);
        // 45° to the right → half front, half right
        let d = (f + crate::player::right_of(f)).normalize();
        let w = fall_grasp_weights(d, f);
        assert!((w[1] - 0.5).abs() < 1e-4 && (w[3] - 0.5).abs() < 1e-4);
        // half-length reach behind-left → half static, rest back-left
        let w = fall_grasp_weights(-f * 0.5, f);
        assert!((w[0] - 0.5).abs() < 1e-4);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn contact_state_finds_travel_windows() {
        // xx_l_climb_1m_u_1lu: L hand + L toe off at 0.067, L hand back at 0.333, L toe at 0.533
        let c = AnimClip { duration: 0.533, contacts: vec![(0.0, 0b111100), (0.067, 0b101000), (0.333, 0b111000), (0.533, 0b111100)], ..default() };
        let s = contact_state(&c, 0.2);
        assert_eq!(s.bits >> 4 & 1, 0);
        assert!((s.off_since[4].unwrap() - 0.067).abs() < 1e-4);
        assert!((s.next_on[4].unwrap() - 0.333).abs() < 1e-4);
        assert!((s.next_on[2].unwrap() - 0.533).abs() < 1e-4);
        assert_eq!(contact_state(&c, 0.4).bits >> 4 & 1, 1);
    }
}
