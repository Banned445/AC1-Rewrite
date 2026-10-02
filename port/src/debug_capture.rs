//! Debug helpers for automated verification (no effect unless the env vars are set):
//! - `AC_AUTOPILOT=roofs|climb|ledge` (or `1` = roofs): scripted input for a scenario.
//!   roofs: sprint across the rooftops · climb: climb the tower · ledge: jump to the balcony and shimmy.
//! - `AC_SCREENSHOT=path.png` (+ optional `AC_SCREENSHOT_AT=seconds`, default 3): save a screenshot
//!   of the primary window at that time, then exit.

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use crate::input::PadInput;
use crate::player::{Body, Player};

pub struct DebugCapturePlugin;

#[derive(Resource, Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Roofs,
    Climb,
    Ledge,
    /// Stand still; camera in front (model check).
    Pose,
    /// Stand still; camera behind at an angle.
    Back,
    /// Locomotion on flat ground toward +X, camera from the side.
    Walk,
    Run,
    Sprint,
    /// Jump up to the 2.6 m wall ledge (wall hang), hold, then pull up.
    WallHang,
    /// Grab wall C, side-jump to D, shimmy to D's end and turn its outer corner (RE/03 §7.6).
    LedgeMoves,
    /// Jump at wall F (free arrival), shimmy left: free → wall, then wall → free at the overhang (0xDE1060).
    HangSwitch,
    /// Stand at roof A's +X edge, press Legs in low profile: pull-down into a wall hang (0xDDE4D0).
    PullDown,
    /// Walk (low profile) into roof A's edge: ledge stop, step back.
    LedgeStop,
    /// Walk across the beam between the two 4 m platforms.
    Beam,
    /// Wall run at the 3.8 m wall: entry, vertical step, hang from the top edge.
    WallRun,
    /// Run off the high block's +X edge into the haystack (Leap of Faith), wait, hop out.
    Faith,
    /// Walk off the 6 m block and fall.
    Drop,
    /// As Drop, holding grab (Legs) and the stick to the left while falling (the game's fall-grasp blend).
    DropGrab,
}

impl Plugin for DebugCapturePlugin {
    fn build(&self, app: &mut App) {
        if let Ok(v) = std::env::var("AC_AUTOPILOT") {
            let sc = match v.as_str() {
                "climb" => Scenario::Climb,
                "ledge" => Scenario::Ledge,
                "pose" => Scenario::Pose,
                "run" => Scenario::Run,
                "sprint" => Scenario::Sprint,
                "walk" => Scenario::Walk,
                "back" => Scenario::Back,
                "wallhang" => Scenario::WallHang,
                "ledgemoves" => Scenario::LedgeMoves,
                "hangswitch" => Scenario::HangSwitch,
                "pulldown" => Scenario::PullDown,
                "ledgestop" => Scenario::LedgeStop,
                "faith" => Scenario::Faith,
                "wallrun" => Scenario::WallRun,
                "beam" => Scenario::Beam,
                "drop" => Scenario::Drop,
                "dropgrab" => Scenario::DropGrab,
                _ => Scenario::Roofs,
            };
            app.insert_resource(sc)
                .add_systems(PostStartup, place)
                .add_systems(PreUpdate, autopilot.after(crate::input::read_pad));
        }
        if std::env::var("AC_TESTPOSE").is_ok() {
            app.add_systems(Update, test_pose);
        }
        if let Ok(path) = std::env::var("AC_SCREENSHOT") {
            let at: f32 = std::env::var("AC_SCREENSHOT_AT").ok().and_then(|s| s.parse().ok()).unwrap_or(3.0);
            app.insert_resource(Capture { path, at, taken: None }).add_systems(Update, capture);
        }
    }
}

#[derive(Resource)]
struct Capture {
    path: String,
    at: f32,
    taken: Option<f32>,
}

fn place(sc: Res<Scenario>, mut q: Query<&mut Body, With<Player>>, mut rig: ResMut<crate::camera::CameraRig>) {
    for mut b in &mut q {
        match *sc {
            Scenario::Roofs => {
                b.feet = Vec3::new(7.0, 3.5, 12.0);
                b.heading = -std::f32::consts::FRAC_PI_2;
                rig.yaw = -0.9;
                rig.distance = 9.0;
                rig.pitch = -0.25;
            }
            Scenario::Climb => {
                b.feet = Vec3::new(-20.5, 0.0, 26.5);
                b.heading = std::f32::consts::PI;
                rig.yaw = std::f32::consts::PI + 0.6;
                rig.distance = 7.0;
                rig.pitch = 0.15;
            }
            Scenario::Ledge => {
                b.feet = Vec3::new(2.0, 0.0, 33.0);
                b.heading = std::f32::consts::PI;
                rig.yaw = std::f32::consts::PI - 0.5;
                rig.distance = 6.0;
                rig.pitch = 0.05;
            }
            Scenario::PullDown => {
                b.feet = Vec3::new(10.6, 3.5, 12.0);
                b.heading = -std::f32::consts::FRAC_PI_2; // facing +X, over the drop
                rig.yaw = 0.0;
                rig.distance = 6.0;
            }
            Scenario::Beam => {
                b.feet = Vec3::new(71.0, 4.0, 70.0);
                b.heading = -std::f32::consts::FRAC_PI_2; // facing +X, toward the beam
                rig.yaw = 0.0;
                rig.distance = 6.0;
            }
            Scenario::WallRun => {
                b.feet = Vec3::new(76.0, 0.0, 57.9);
                b.heading = std::f32::consts::PI; // facing +Z, at the wall
                rig.yaw = std::f32::consts::FRAC_PI_2;
                rig.distance = 6.0;
            }
            Scenario::Faith => {
                b.feet = Vec3::new(30.5, 9.5, 26.0);
                b.heading = -std::f32::consts::FRAC_PI_2;
                rig.yaw = 0.0;
                rig.distance = 7.0;
            }
            Scenario::LedgeStop => {
                b.feet = Vec3::new(8.5, 3.5, 12.0);
                b.heading = -std::f32::consts::FRAC_PI_2;
                rig.yaw = 0.0;
                rig.distance = 6.0;
            }
            Scenario::HangSwitch => {
                b.feet = Vec3::new(61.5, 0.0, 48.6);
                b.heading = std::f32::consts::PI;
                rig.yaw = std::f32::consts::PI;
                rig.distance = 6.0;
            }
            Scenario::LedgeMoves => {
                b.feet = Vec3::new(22.0, 0.0, 48.6);
                b.heading = std::f32::consts::PI; // facing +Z, into wall C
                rig.yaw = std::f32::consts::PI;
                rig.distance = 6.0;
            }
            Scenario::WallHang => {
                b.feet = Vec3::new(12.0, 0.0, 40.6);
                b.heading = std::f32::consts::PI; // facing +Z, into the wall
            }
            Scenario::Drop | Scenario::DropGrab => {
                b.feet = Vec3::new(-11.5, 6.0, 4.0);
                b.heading = -std::f32::consts::FRAC_PI_2; // facing +X, off the edge
            }
            Scenario::Walk | Scenario::Run | Scenario::Sprint => {
                b.feet = Vec3::new(-60.0, 0.0, -40.0);
                b.heading = -std::f32::consts::FRAC_PI_2; // facing +X
                rig.yaw = 0.0; // camera on the +Z side → side view
                rig.distance = 4.0;
                rig.pitch = -0.05;
            }
            Scenario::Pose | Scenario::Back => {
                b.feet = Vec3::new(-30.0, 0.0, -20.0);
                b.heading = std::f32::consts::PI; // facing +Z
                rig.yaw = if *sc == Scenario::Pose { 0.35 } else { std::f32::consts::PI + 0.6 };
                rig.distance = 3.2;
                rig.pitch = -0.08;
            }
        }
    }
}

fn autopilot(time: Res<Time>, sc: Res<Scenario>, mut pad: ResMut<PadInput>) {
    let t = time.elapsed_secs();
    pad.magnitude = 1.0;
    pad.speed01 = 1.0;
    match *sc {
        Scenario::Pose | Scenario::Back => {
            pad.magnitude = 0.0;
            pad.speed01 = 0.0;
        }
        Scenario::Walk | Scenario::Run | Scenario::Sprint => {
            pad.dir = Vec3::X;
            pad.high_profile = *sc != Scenario::Walk;
            pad.legs_held = *sc == Scenario::Sprint;
        }
        Scenario::Roofs => {
            pad.dir = Vec3::X;
            pad.high_profile = true;
            pad.legs_held = true;
        }
        Scenario::Climb => {
            pad.dir = Vec3::Z;
            pad.high_profile = true;
            pad.legs_held = true;
        }
        Scenario::PullDown => {
            pad.magnitude = 0.0;
            pad.speed01 = 0.0;
            pad.high_profile = false;
            if (0.5..0.52).contains(&t) {
                pad.legs_pressed_ago = 0.0;
            }
        }
        Scenario::Beam => {
            pad.dir = Vec3::X;
            pad.high_profile = false;
            if t < 1.0 {
                pad.magnitude = 0.0;
                pad.speed01 = 0.0;
            }
        }
        Scenario::WallRun => {
            // stand for 1 s first (the renderer's pipelines finish compiling), then go
            let t = t - 1.0;
            pad.dir = Vec3::Z;
            pad.high_profile = true;
            pad.legs_held = t > 0.2;
            if (0.2..0.22).contains(&t) {
                pad.legs_pressed_ago = 0.0;
            }
            if !(0.0..=0.85).contains(&t) {
                pad.magnitude = 0.0;
                pad.speed01 = 0.0;
            }
        }
        Scenario::Faith => {
            pad.dir = Vec3::X;
            let run = t < 1.2;
            let hop = t > 6.0;
            pad.magnitude = if run || hop { 1.0 } else { 0.0 };
            pad.speed01 = pad.magnitude;
            pad.high_profile = run;
            pad.legs_held = run;
        }
        Scenario::LedgeStop => {
            pad.dir = Vec3::X;
            pad.magnitude = if t < 2.5 { 1.0 } else { 0.0 };
            pad.speed01 = pad.magnitude;
            pad.high_profile = false;
        }
        Scenario::HangSwitch | Scenario::LedgeMoves => {
            // grab C (jump up), then hold the stick toward +X (the player's left): side jump to D, shimmy
            // along D, outer corner at D's end
            pad.high_profile = t < 0.6;
            pad.legs_held = (0.3..0.6).contains(&t);
            pad.dir = if t < 2.0 { Vec3::Z } else { Vec3::X };
            if (0.6..2.0).contains(&t) {
                pad.magnitude = 0.0;
                pad.speed01 = 0.0;
            }
        }
        Scenario::WallHang => {
            // into the wall with high profile, Legs at 0.4 s (jump up to the ledge), hang until 3.5 s,
            // then push up (pull-up)
            pad.dir = Vec3::Z;
            pad.high_profile = true;
            pad.legs_held = (0.3..0.6).contains(&t);
            if (0.6..3.5).contains(&t) {
                pad.magnitude = 0.0;
                pad.speed01 = 0.0;
            }
        }
        Scenario::Drop => {
            pad.dir = Vec3::X;
            pad.high_profile = false;
            pad.legs_held = false;
        }
        Scenario::DropGrab => {
            // walk off, then hold grab and push the stick to the left (-Z is left of +X facing)
            pad.dir = if t < 1.2 { Vec3::X } else { Vec3::NEG_Z };
            pad.high_profile = false;
            pad.legs_held = t >= 1.2;
        }
        Scenario::Ledge => {
            // run at the balcony, jump (Legs press at 0.3 s), let go of the stick, then shimmy right (-X)
            pad.dir = if t < 1.0 { Vec3::Z } else { Vec3::NEG_X };
            if (1.0..1.6).contains(&t) {
                pad.magnitude = 0.0;
                pad.speed01 = 0.0;
            }
            pad.high_profile = t < 1.0;
            pad.legs_held = false;
            if (0.3..0.32).contains(&t) {
                pad.legs_pressed_ago = 0.0;
            }
        }
    }
    // `AC_STICK_OFF=a-b,c-d`: release the stick in these time windows (to hold still for inspection)
    if let Ok(v) = std::env::var("AC_STICK_OFF") {
        for w in v.split(',') {
            if let Some((a, b)) = w.split_once('-') {
                if let (Ok(a), Ok(b)) = (a.parse::<f32>(), b.parse::<f32>()) {
                    if (a..b).contains(&t) {
                        pad.magnitude = 0.0;
                        pad.speed01 = 0.0;
                    }
                }
            }
        }
    }
}

/// Skin-binding check: bend LeftArm, RightForeArm and LeftLeg away from their rest rotation.
fn test_pose(rig: Query<&crate::model::Rig>, mut joints: Query<&mut Transform>) {
    let Ok(rig) = rig.single() else { return };
    for (bone, angle) in [(0xeb83_0adau32, 1.0f32), (0x7257_a1aa, 1.2), (0x060d_f401, -1.1)] {
        if let Some(i) = rig.bone_ids.iter().position(|&b| b == bone) {
            if let Ok(mut t) = joints.get_mut(rig.joints[i]) {
                t.rotation = rig.rest[i].rotation * Quat::from_rotation_z(angle);
            }
        }
    }
}

fn capture(mut commands: Commands, time: Res<Time>, mut cap: ResMut<Capture>, mut exit: MessageWriter<AppExit>) {
    let t = time.elapsed_secs();
    match cap.taken {
        None if t >= cap.at => {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(cap.path.clone()));
            cap.taken = Some(t);
        }
        Some(t0) if t - t0 > 1.0 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- multi-angle shots
// `AC_SHOTS=t1,t2,…` (+ `AC_SHOT_DIR=folder`, `AC_VIEWS=back,left,right,high,hands`): at each time the
// simulation freezes (virtual time paused), the camera visits every view (one frame each, positioned
// relative to the player's facing) and a screenshot `<dir>/<t>_<view>.png` is saved; then time resumes.
// Time advances in fixed 1/60 s steps so runs are reproducible.

#[derive(Resource)]
struct Shots {
    times: Vec<f32>,
    views: Vec<String>,
    dir: String,
    next: usize,
    view: Option<usize>,
    done_at: Option<u32>,
    frames: u32,
}

pub struct ShotsPlugin;

impl Plugin for ShotsPlugin {
    fn build(&self, app: &mut App) {
        let Ok(list) = std::env::var("AC_SHOTS") else { return };
        let mut times: Vec<f32> = list.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let views = std::env::var("AC_VIEWS").unwrap_or("back,left,right,high,hands".into()).split(',').map(String::from).collect();
        let dir = std::env::var("AC_SHOT_DIR").unwrap_or(".".into());
        let _ = std::fs::create_dir_all(&dir);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)))
            .insert_resource(Shots { times, views, dir, next: 0, view: None, done_at: None, frames: 0 })
            .add_systems(Startup, fill_light)
            .add_systems(PostUpdate, shots.before(bevy::transform::TransformSystems::Propagate));
    }
}

#[allow(clippy::too_many_arguments)]
fn shots(
    mut commands: Commands,
    mut shots: ResMut<Shots>,
    mut vtime: ResMut<Time<Virtual>>,
    player: Query<(&Body, &crate::player::LimbTargets), With<Player>>,
    rig: Query<&crate::model::Rig>,
    globals: Query<&GlobalTransform>,
    mut cam: Query<&mut Transform, With<crate::camera::MainCamera>>,
    mut exit: MessageWriter<AppExit>,
) {
    shots.frames += 1;
    if let Some(f) = shots.done_at {
        if shots.frames > f + 30 {
            exit.write(AppExit::Success);
        }
        return;
    }
    let t = vtime.elapsed_secs();
    if shots.view.is_none() {
        if shots.next >= shots.times.len() {
            shots.done_at = Some(shots.frames);
            return;
        }
        if t + 1e-4 < shots.times[shots.next] {
            return;
        }
        vtime.pause();
        shots.view = Some(0);
    }
    let vi = shots.view.unwrap();
    let Ok((body, limbs)) = player.single() else { return };
    let fwd = body.forward();
    let right = crate::player::right_of(fwd);
    // focus: the hips joint (falls back to the body root)
    let hips = rig.single().ok().and_then(|r| r.bone_ids.iter().position(|b| *b == 0xded1_0611).and_then(|i| globals.get(r.joints[i]).ok()));
    let focus = hips.map(|g| g.translation()).unwrap_or(body.feet + Vec3::Y * 1.0);
    if std::env::var_os("AC_SHOT_LOG").is_some() {
        info!("shot t={t:.2} feet {:?} hips {:?}", body.feet, hips.map(|g| g.translation()));
    }
    let name = shots.views[vi].clone();
    let (target, eye) = match name.as_str() {
        "left" => (focus, focus - right * 3.0 + Vec3::Y * 0.3),
        "right" => (focus, focus + right * 3.0 + Vec3::Y * 0.3),
        "high" => (focus, focus - fwd * 2.2 - right * 1.6 + Vec3::Y * 2.2),
        "front" => (focus, focus + fwd * 3.0 + Vec3::Y * 0.3),
        "fingers" | "fingers_side" => {
            let h = limbs.hands.map(|(l, r)| if l.y >= r.y { l } else { r }).unwrap_or(focus + Vec3::Y * 0.8);
            let side = if name == "fingers" { -fwd * 0.6 - right * 0.45 + Vec3::Y * 0.35 } else { -fwd * 0.3 - right * 0.7 + Vec3::Y * 0.1 };
            (h, h + side)
        }
        "hands" => {
            let h = limbs.hands.map(|(l, r)| (l + r) * 0.5).unwrap_or(focus + Vec3::Y * 0.8);
            (h, h - fwd * 1.5 - right * 0.8 + Vec3::Y * 0.6)
        }
        _ => (focus, focus - fwd * 3.2 + Vec3::Y * 0.6),
    };
    if let Ok(mut c) = cam.single_mut() {
        *c = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    }
    let path = format!("{}/{:05.2}_{}.png", shots.dir, shots.times[shots.next], name);
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    if vi + 1 >= shots.views.len() {
        shots.view = None;
        shots.next += 1;
        vtime.unpause();
    } else {
        shots.view = Some(vi + 1);
    }
}

/// Capture runs only: a shadowless fill light from the -Z side so walls facing away from the sun
/// (the climb tower face) are readable.
fn fill_light(mut commands: Commands) {
    commands.spawn((
        DirectionalLight { illuminance: 5_000.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(0.0, 10.0, -10.0).looking_at(Vec3::new(0.0, 0.0, 6.0), Vec3::Y),
    ));
}
