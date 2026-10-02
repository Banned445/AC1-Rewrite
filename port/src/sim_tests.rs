//! Headless simulation tests: run the real ground/air systems on the greybox level with scripted
//! pad input at a fixed 60 Hz, and check behaviour against the reverse-engineered rules.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use crate::camera::CameraRig;
use crate::input::PadInput;
use crate::level;
use crate::player::air::LandingType;
use crate::player::ground::{speed_band, HumanGroundData, SpeedBand};
use crate::player::{air, climb, ground, ledge, player_components, ActorContextId, Body, HumanDataBundle, Locomotion, SpawnPoint};

const SPAWN: Vec3 = Vec3::new(0.0, 0.0, 4.0);

struct Sim {
    app: App,
    player: Entity,
    saw_air: bool,
}

impl Sim {
    fn new(feet: Vec3, heading: f32) -> Self {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)))
            .insert_resource(PadInput { legs_pressed_ago: f32::INFINITY, ..default() })
            .insert_resource(SpawnPoint(SPAWN))
            .init_resource::<CameraRig>()
            .add_systems(Update, (ground::update_ground, air::update_air, ledge::update_ledge, climb::update_climb, crate::player::hay::update_hay, crate::player::walling::update_walling, crate::player::narrow::update_narrow).chain());
        let (c, g) = level::geometry();
        app.insert_resource(c).insert_resource(g);
        let player = app.world_mut().spawn(player_components(feet, heading)).id();
        app.update(); // first frame has dt = 0
        Sim { app, player, saw_air: false }
    }

    fn pad(&mut self, dir: Vec3, stick: f32, high: bool, legs: bool) {
        let mut p = self.app.world_mut().resource_mut::<PadInput>();
        p.dir = dir.normalize_or_zero();
        p.magnitude = stick;
        p.speed01 = if stick <= 0.35 { 0.0 } else { ((stick - 0.35) / 0.65).min(1.0) };
        p.high_profile = high;
        p.legs_held = legs;
    }

    fn run(&mut self, seconds: f32) {
        let frames = (seconds * 60.0) as usize;
        for _ in 0..frames {
            // Legs buffer bookkeeping normally done by read_pad
            {
                let mut p = self.app.world_mut().resource_mut::<PadInput>();
                p.legs_pressed_ago += 1.0 / 60.0;
            }
            self.app.update();
            if self.loco().current == ActorContextId::InAir {
                self.saw_air = true;
            }
        }
    }

    fn loco(&self) -> &Locomotion {
        self.app.world().get::<Locomotion>(self.player).unwrap()
    }
    fn body(&self) -> &Body {
        self.app.world().get::<Body>(self.player).unwrap()
    }
    fn data(&self) -> &HumanDataBundle {
        self.app.world().get::<HumanDataBundle>(self.player).unwrap()
    }
    fn ground(&self) -> &HumanGroundData {
        &self.data().ground
    }
    /// Press Legs (fills the jump buffer, like read_pad on a fresh press).
    fn press_legs(&mut self) {
        self.app.world_mut().resource_mut::<PadInput>().legs_pressed_ago = 0.0;
    }
    /// Run until `pred` holds or `max_s` elapses; returns whether it held.
    fn run_until(&mut self, max_s: f32, pred: impl Fn(&Sim) -> bool) -> bool {
        for _ in 0..(max_s * 60.0) as usize {
            self.run(1.0 / 60.0 + 1e-4);
            if pred(self) {
                return true;
            }
        }
        false
    }
}

#[test]
fn low_profile_full_stick_is_walk_band() {
    let mut s = Sim::new(SPAWN, std::f32::consts::PI);
    s.pad(Vec3::Z, 1.0, false, false);
    s.run(2.0);
    assert_eq!(speed_band(s.ground().speed_param), SpeedBand::Walk);
    assert!((s.ground().speed_param - 0.25).abs() < 1e-3, "param {}", s.ground().speed_param);
    assert!(s.body().feet.z > SPAWN.z + 2.0, "walked forward: {:?}", s.body().feet);
}

#[test]
fn high_profile_runs_and_legs_sprints() {
    let mut s = Sim::new(Vec3::new(-30.0, 0.0, -30.0), 0.0);
    s.pad(Vec3::NEG_Z, 1.0, true, false);
    s.run(1.5);
    assert_eq!(speed_band(s.ground().speed_param), SpeedBand::Run);
    s.pad(Vec3::NEG_Z, 1.0, true, true);
    s.run(1.5);
    assert_eq!(speed_band(s.ground().speed_param), SpeedBand::Sprint);
    assert!(s.ground().speed_param > 0.99);
}

#[test]
fn speed_param_rises_at_one_per_second() {
    let mut s = Sim::new(Vec3::new(-30.0, 0.0, -30.0), 0.0);
    s.pad(Vec3::NEG_Z, 1.0, true, true);
    s.run(0.5);
    let p = s.ground().speed_param;
    assert!((p - 0.5).abs() < 0.05, "after 0.5 s param = {p}");
}

#[test]
fn ground_moves_by_the_blended_clip_root_motion() {
    // run band top (param 0.75 = pure xx_h_run_hipm, 1.707 m per 0.3333 s step)
    let mut s = Sim::new(Vec3::new(-30.0, 0.0, -30.0), 0.0);
    s.pad(Vec3::NEG_Z, 1.0, true, false);
    s.run(1.5);
    let z0 = s.body().feet.z;
    s.run(1.0);
    let v = z0 - s.body().feet.z;
    assert!((v - 5.121).abs() < 0.05, "run speed {v} m/s");
    // sprint top settles on xx_h_sprint_hipm (1.674 m per 0.2667 s)
    s.pad(Vec3::NEG_Z, 1.0, true, true);
    s.run(2.5);
    let z0 = s.body().feet.z;
    s.run(1.0);
    let v = z0 - s.body().feet.z;
    assert!((v - 6.277).abs() < 0.05, "sprint speed {v} m/s");
    assert!(s.ground().blend.weights[13] > 0.99);
}

#[test]
fn releasing_sprint_decelerates_through_the_curve() {
    let mut s = Sim::new(Vec3::new(-30.0, 0.0, -30.0), 0.0);
    s.pad(Vec3::NEG_Z, 1.0, true, true);
    s.run(1.5);
    // back to high-profile run (target 0.75). On the curve's last segment ds/dt = −(0.2 + 2.395·(s − 0.666)),
    // so from 1.0: s(0.2 s) = 0.4175·e^(−0.479) + 0.5825 = 0.841
    s.pad(Vec3::NEG_Z, 1.0, true, false);
    s.run(0.2);
    let p = s.ground().speed_param;
    assert!((p - 0.841).abs() < 0.01, "param after 0.2 s = {p}");
    s.run(1.0);
    assert!((s.ground().speed_param - 0.75).abs() < 1e-3);
}

#[test]
fn free_run_jumps_a_rooftop_gap_and_lands_on_target() {
    // roof A: x 5..11, h 3.5 ; roof B: x 14.5..20.5, h 3.0  (3.5 m gap)
    let mut s = Sim::new(Vec3::new(7.0, 3.5, 12.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, true, true); // sprint toward +X
    for _ in 0..240 {
        s.run(1.0 / 60.0 + 1e-4);
        if s.saw_air && s.loco().current == ActorContextId::Ground {
            s.pad(Vec3::X, 0.0, false, false); // let go once landed
            s.run(1.0);
            break;
        }
    }
    assert!(s.saw_air, "never jumped");
    assert_eq!(s.loco().current, ActorContextId::Ground);
    let f = s.body().feet;
    assert!(f.x > 14.5 && (f.y - 3.0).abs() < 0.05, "should be standing on roof B, feet {f:?}");
    let l = s.ground().last_landing.expect("landing recorded");
    assert_eq!(l.kind, LandingType::Safe);
}

#[test]
fn chained_free_run_crosses_several_roofs() {
    // holding sprint keeps jumping: roof A (h 3.5) → B (h 3.0, 3.5 m gap) → C (h 4.0, 5 m gap, +1 m up)
    let mut s = Sim::new(Vec3::new(7.0, 3.5, 12.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, true, true);
    let mut on_b = false;
    let mut on_c = false;
    for _ in 0..360 {
        s.run(1.0 / 60.0 + 1e-4);
        let f = s.body().feet;
        let grounded = s.loco().current == ActorContextId::Ground;
        on_b |= grounded && (14.5..20.5).contains(&f.x) && (f.y - 3.0).abs() < 0.05;
        on_c |= grounded && (25.5..31.5).contains(&f.x) && (f.y - 4.0).abs() < 0.05;
    }
    assert!(on_b && on_c, "landed on B: {on_b}, landed on C: {on_c}");
}

#[test]
fn eight_metre_drop_is_fatal_and_respawns() {
    // tower at (-12, 14), 5x5, h 8: run off its +X edge
    let mut s = Sim::new(Vec3::new(-11.0, 8.0, 14.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, true, false); // high profile: walking stops at the edge (ledge stop)
    let landed = s.run_until(4.0, |s| s.saw_air && s.ground().last_landing.is_some());
    assert!(landed && s.saw_air);
    s.pad(Vec3::X, 0.0, false, false);
    s.run(0.2);
    let l = s.ground().last_landing.expect("landed");
    assert_eq!(l.kind, LandingType::Fatal, "{l:?}");
    assert!((s.body().feet - SPAWN).length() < 3.0, "respawned near spawn: {:?}", s.body().feet);
}

#[test]
fn six_metre_drop_rolls_without_heavy_damage() {
    // 6 m is below the 6.3 m heavy threshold but above the 3 m roll threshold
    let mut s = Sim::new(Vec3::new(-11.0, 6.0, 4.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, true, false); // high profile: walking stops at the edge (ledge stop)
    s.run(4.0);
    let l = s.ground().last_landing.expect("landed");
    assert_eq!(l.kind, LandingType::Safe, "{l:?}");
    assert!(l.roll, "drop > 3 m should roll: {l:?}");
}

#[test]
fn step_up_low_obstacle() {
    // 0.3 m block at (6, 0): STEP_HEIGHT 0.35 lets the capsule walk over it
    let mut s = Sim::new(Vec3::new(3.0, 0.0, 0.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, true, false);
    s.run(2.0);
    assert!(s.body().feet.x > 7.0, "blocked by 0.3 m step: {:?}", s.body().feet);
    assert_eq!(s.loco().current, ActorContextId::Ground);
}

#[test]
fn wall_blocks_movement() {
    // wall: x -7..7, z -8.3..-7.7, h 2.4
    let mut s = Sim::new(Vec3::new(0.0, 0.0, -5.0), 0.0);
    s.pad(Vec3::NEG_Z, 1.0, true, false);
    s.run(2.0);
    // wall face at z = -7.7, capsule radius 0.3 → should stop at about z = -7.4
    assert!(s.body().feet.z > -7.45, "went through wall: {:?}", s.body().feet);
}

// ============================================================== stage 2: ledges and climbing

const FACE_PZ: f32 = std::f32::consts::PI; // heading that faces +Z

#[test]
fn climb_tower_to_the_top_and_pull_up() {
    // tower face at z = 27 (normal -Z), roof at 9.6 m
    let mut s = Sim::new(Vec3::new(-20.5, 0.0, 26.5), FACE_PZ);
    s.pad(Vec3::Z, 1.0, true, true);
    assert!(s.run_until(1.0, |s| s.loco().current == ActorContextId::Climb), "did not start climbing");
    assert_eq!(s.data().climb.entry_type, climb::ClimbEntryType::FromGround);
    let reached = s.run_until(30.0, |s| s.loco().current == ActorContextId::Ground && s.body().feet.y > 9.5);
    let f = s.body().feet;
    assert!(reached, "never stood on the roof: ctx {:?} feet {f:?} climb {:?} ledge {:?}", s.loco().current, s.data().climb.last_action, s.data().ledge.last_action);
    assert!(f.z > 27.0, "should be on the roof, feet {f:?}");
}

#[test]
fn climb_is_blocked_by_missing_holds() {
    // column x = -18.0 has no holds between 2.9 and 4.3 m
    let mut s = Sim::new(Vec3::new(-18.0, 0.0, 26.5), FACE_PZ);
    s.pad(Vec3::Z, 1.0, true, true);
    assert!(s.run_until(1.0, |s| s.loco().current == ActorContextId::Climb));
    s.run(8.0);
    assert_eq!(s.loco().current, ActorContextId::Climb);
    let top_hand = s.data().climb.hand_l.y.max(s.data().climb.hand_r.y);
    assert!(top_hand < 2.95, "climbed into the gap: hands at {top_hand}");
    assert_eq!(s.data().climb.last_action, "blocked");
}

/// Jump-up wall: top edge 2.6 m, face z = 41.7 (normal -Z), x 8..16, pillar at x 15.5..16.5.
fn hang_on_jump_up_wall() -> Sim {
    let mut s = Sim::new(Vec3::new(12.0, 0.0, 40.6), FACE_PZ);
    s.pad(Vec3::Z, 1.0, true, true);
    let hung = s.run_until(3.0, |s| s.loco().current == ActorContextId::Ledge);
    assert!(hung, "never hung: ctx {:?} feet {:?}", s.loco().current, s.body().feet);
    s.pad(Vec3::Z, 0.0, false, false);
    s.run(0.5);
    s
}

#[test]
fn jump_up_to_a_ledge_then_pull_up() {
    let mut s = hang_on_jump_up_wall();
    assert!(s.saw_air, "should have jumped up to the 2.6 m ledge");
    // hands 2.6 m above the feet with a wall below: the straight jump's top band, `jumpstraight_to_hangwallfree`,
    // which the arrival turns into a free hang (0xB21DA0 / 0xE07D00: LedgeHangType 1)
    assert!(s.run_until(3.0, |s| s.data().ledge.mv.is_none()), "reception never ended");
    assert_eq!(s.data().ledge.hang_type, ledge::LedgeHangType::Free);
    let hands = s.data().ledge.hand_l.y;
    assert!((hands - 2.6).abs() < 0.05, "hands on the edge: {hands}");
    let f = s.body().feet;
    assert!((f.y - 0.2).abs() < 0.05, "free-hang root 2.4 m below the hands: {f:?}");
    s.pad(Vec3::Z, 1.0, false, false); // hold up at the top edge → pull-up
    let on_top = s.run_until(3.0, |s| s.loco().current == ActorContextId::Ground);
    assert!(on_top && (s.body().feet.y - 2.6).abs() < 0.05, "pull-up failed: {:?} ledge {}", s.body().feet, s.data().ledge.last_action);
}

#[test]
fn shimmy_is_stopped_by_the_pillar() {
    let mut s = hang_on_jump_up_wall();
    // facing +Z the player's left is +X, toward the pillar
    s.pad(Vec3::X, 1.0, false, false);
    s.run(8.0);
    assert_eq!(s.loco().current, ActorContextId::Ledge);
    let max_x = s.data().ledge.hand_l.x.max(s.data().ledge.hand_r.x);
    assert!(max_x > 13.5, "should have shimmied toward the pillar, hands at x {max_x}");
    assert!(max_x < 15.5, "went into the pillar: x {max_x}");
    assert!(s.data().ledge.last_action.contains("blocked"), "{}", s.data().ledge.last_action);
}

/// Free-hang balcony slab: top 3.0 m, x -1..5, front edge z 35.4 (normal -Z), nothing below.
fn hang_on_balcony() -> Sim {
    let mut s = Sim::new(Vec3::new(2.0, 0.0, 33.0), FACE_PZ);
    s.pad(Vec3::Z, 1.0, true, false);
    s.run(0.3);
    s.press_legs();
    let hung = s.run_until(3.0, |s| s.loco().current == ActorContextId::Ledge);
    assert!(hung, "never reached the balcony: ctx {:?} feet {:?}", s.loco().current, s.body().feet);
    s.pad(Vec3::Z, 0.0, false, false);
    // the arrival's reception (swing) plays before the hang takes input
    assert!(s.run_until(4.0, |s| s.data().ledge.mv.is_none() && s.data().ledge.swing.is_none()), "reception never ended");
    s.run(0.2);
    s
}

#[test]
fn free_hang_shimmy_ends_with_the_outer_corner() {
    let mut s = hang_on_balcony();
    assert_eq!(s.data().ledge.hang_type, ledge::LedgeHangType::Free);
    let f = s.body().feet;
    assert!((f.y - 0.6).abs() < 0.05, "free hang root 2.4 m below the hands: {f:?}");
    // the player's right (facing +Z) is -X; the slab ends at x = -1
    s.pad(Vec3::NEG_X, 1.0, false, false);
    s.run(10.0);
    let min_x = s.data().ledge.hand_l.x.min(s.data().ledge.hand_r.x);
    assert_eq!(s.loco().current, ActorContextId::Ledge);
    assert!(min_x >= -1.01 && min_x < 0.2, "hands should stop at the slab end: {min_x}");
    // at the end the free hang turns the outer corner onto the slab's -X side (0xDD3BB0, `corner_090_out`)
    assert!(s.data().ledge.normal.dot(Vec3::NEG_X) > 0.99, "no outer corner: normal {:?}", s.data().ledge.normal);
}

#[test]
fn let_go_from_a_hang_falls_and_lands() {
    let mut s = hang_on_balcony();
    s.press_legs();
    assert!(s.run_until(0.5, |s| s.loco().current == ActorContextId::InAir), "did not let go");
    assert_eq!(s.data().air.fall_origin, air::FallOrigin::HangFree);
    assert!(s.run_until(3.0, |s| s.loco().current == ActorContextId::Ground));
    assert_eq!(s.ground().last_landing.map(|l| l.kind), Some(LandingType::Safe));
}

#[test]
#[ignore]
fn trace_climb() {
    let mut s = Sim::new(Vec3::new(-20.5, 0.0, 26.5), FACE_PZ);
    s.pad(Vec3::Z, 1.0, true, true);
    let mut last = String::new();
    for i in 0..(30 * 60) {
        s.run(1.0 / 60.0 + 1e-4);
        let d = s.data();
        let line = format!("{:?} pose {} climb:{} ledge:{} {:?}", s.loco().current, d.climb.pose, d.climb.last_action, d.ledge.last_action, d.ledge.sub_state);
        if line != last {
            eprintln!("t={:5.2} feet y {:5.2} hands {:.2}/{:.2} | {line}", i as f32 / 60.0, s.body().feet.y, d.climb.hand_l.y, d.climb.hand_r.y);
            last = line;
        }
        if s.loco().current == ActorContextId::Ground && s.body().feet.y > 9.5 { break; }
    }
}

#[test]
fn rooftop_jump_plays_the_game_items_and_free_step_reception() {
    use crate::player::air::AirMode;
    use crate::player::jump_blend::{RECEPTION_FREESTEP, TAKEOFF_RUN};
    // roof A: x 5..11, h 3.5 ; roof B: x 14.5..20.5, h 3.0
    let mut s = Sim::new(Vec3::new(7.0, 3.5, 12.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, true, true);
    assert!(s.run_until(4.0, |s| s.loco().current == ActorContextId::InAir), "never jumped");
    let air = &s.data().air;
    let AirMode::Jump { real, duration, t_takeoff, .. } = air.mode else { panic!("not a jump: {:?}", air.mode) };
    assert!(real);
    let (to, fl) = (air.takeoff.unwrap(), air.flight.unwrap());
    assert!(TAKEOFF_RUN.contains(&to.id));
    // durations are the items' Σw·T (takeoff 0.13–0.33 s, flight 0.2–0.6 s for these clips)
    assert!((t_takeoff - to.duration()).abs() < 1e-5 && (duration - t_takeoff - fl.duration()).abs() < 1e-5);
    assert!(duration > 0.3 && duration < 1.2, "jump lasts {duration}");
    let sum: f32 = fl.weights().iter().sum();
    assert!((sum - 1.0).abs() < 1e-4);
    // arrival → Ground with the free-step reception playing, then free movement
    assert!(s.run_until(2.0, |s| s.loco().current == ActorContextId::Ground), "never landed");
    let os = s.ground().oneshot.expect("reception playing");
    assert!(RECEPTION_FREESTEP.contains(&os.blend.id));
    assert!(os.duration > 0.1);
    assert!(s.run_until(2.0, |s| s.ground().oneshot.is_none()), "reception never ended");
    assert!(s.ground().speed_param > 0.75, "kept the sprint speed: {}", s.ground().speed_param);
}

#[test]
fn small_drop_lands_with_the_forward_landing_blend() {
    use crate::player::jump_blend::{LAND_FORWARD_MOVE, LAND_STRAIGHT_MOVE};
    // walk off roof B's far edge (3.0 m high, x 14.5..20.5) at jog speed: drop 3.0 m ≤ 3 → landing, b = 1 (hard)
    let mut s = Sim::new(Vec3::new(19.0, 3.0, 12.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, true, false);
    assert!(s.run_until(4.0, |s| s.saw_air && s.loco().current == ActorContextId::Ground), "never landed");
    let os = s.ground().oneshot.expect("landing action");
    assert!(LAND_FORWARD_MOVE.contains(&os.blend.id) || os.blend.id == LAND_STRAIGHT_MOVE, "{:#x}", os.blend.id);
    // speed ratio ~0.75 → bucket 2 (jog exit): weights in slots 1 (soft) and 4 (hard)
    let w = os.blend.weights();
    assert!(w[1] + w[4] > 0.99, "{w:?}");
}

/// Start hanging (wall hang) with the hands centred on `mid`, facing into the wall whose normal is `n`.
fn hang_at(mid: Vec3, n: Vec3) -> Sim {
    use crate::player::ledge::{LedgeEntry, LedgeSubState};
    use crate::player::{switch_context, TransitionSetup};
    let feet = mid - Vec3::Y * 1.1 + n * 0.5;
    let mut s = Sim::new(feet, crate::player::heading_of(-n));
    {
        let player = s.player;
        let w = s.app.world_mut();
        let mut q = w.query::<(&mut Locomotion, &mut HumanDataBundle)>();
        let (mut loco, mut data) = q.get_mut(w, player).unwrap();
        switch_context(&mut loco, &mut data, TransitionSetup::ToLedge(LedgeEntry::at(mid, n, feet, LedgeSubState::Movement)));
    }
    s.run(1.0);
    assert_eq!(s.loco().current, ActorContextId::Ledge);
    s
}

#[test]
fn inner_corner_turns_onto_the_perpendicular_wall() {
    // L-wall: A's -Z face (z 49.7), B sticks out toward -Z at x 36 (its -X face is the inner corner)
    let mut s = hang_at(Vec3::new(34.0, 2.6, 49.7), Vec3::NEG_Z);
    s.pad(Vec3::X, 1.0, false, false); // facing +Z the player's left is +X
    let turned = s.run_until(8.0, |s| s.data().ledge.normal.dot(Vec3::NEG_X) > 0.99 && s.data().ledge.mv.is_none());
    assert!(turned, "no corner: {} hands {:?}", s.data().ledge.last_action, s.data().ledge.hand_l);
    let l = &s.data().ledge;
    assert!((l.hand_l.x - 35.92).abs() < 0.05 && (l.hand_l.y - 2.6).abs() < 0.05, "hands on B's stone ledge: {:?}", l.hand_l);
    assert_eq!(s.loco().current, ActorContextId::Ledge);
}

#[test]
fn side_jump_crosses_the_gap_between_ledges() {
    // C (x 17..23) and D (x 24..27): same edge line, 1 m gap
    let mut s = hang_at(Vec3::new(22.0, 2.6, 49.7), Vec3::NEG_Z);
    s.pad(Vec3::X, 1.0, false, false);
    let jumped = s.run_until(6.0, |s| s.data().ledge.last_action == "side jump");
    assert!(jumped, "no side jump: {}", s.data().ledge.last_action);
    assert!(s.run_until(3.0, |s| s.data().ledge.mv.is_none()));
    let x = s.data().ledge.hand_l.x.min(s.data().ledge.hand_r.x);
    assert!(x > 23.9, "should hang on D: hands x {x}");
    assert_eq!(s.loco().current, ActorContextId::Ledge);
}

#[test]
fn outer_corner_wraps_around_the_wall_end() {
    // C's -X end (x 17): moving -X (the player's right) wraps onto C's -X face
    let mut s = hang_at(Vec3::new(18.0, 2.6, 49.7), Vec3::NEG_Z);
    s.pad(Vec3::NEG_X, 1.0, false, false);
    let turned = s.run_until(8.0, |s| s.data().ledge.normal.dot(Vec3::NEG_X) > 0.99 && s.data().ledge.mv.is_none());
    assert!(turned, "no outer corner: {} hands {:?}", s.data().ledge.last_action, s.data().ledge.hand_l);
    assert!((s.data().ledge.hand_l.x - 17.0).abs() < 0.05);
}

#[test]
fn hop_up_reaches_the_ledge_above_and_swings_into_a_free_hang() {
    use crate::player::ledge_moves::{MoveKind, HOP_UP};
    let mut s = hang_at(Vec3::new(42.0, 2.6, 49.7), Vec3::NEG_Z);
    s.pad(Vec3::Z, 1.0, false, false); // up
    let hopped = s.run_until(2.0, |s| s.data().ledge.mv.is_some_and(|m| m.kind == MoveKind::HopUp));
    assert!(hopped, "no hop: {}", s.data().ledge.last_action);
    let mv = s.data().ledge.mv.unwrap();
    assert_eq!(mv.seq[0].map(|a| a.id), Some(HOP_UP));
    s.pad(Vec3::Z, 0.0, false, false);
    assert!(s.run_until(3.0, |s| s.data().ledge.mv.is_none()));
    let l = &s.data().ledge;
    assert!((l.hand_l.y - 4.2).abs() < 0.05, "hands on E2: {:?}", l.hand_l);
    assert_eq!(l.hang_type, ledge::LedgeHangType::Free);
    assert!((s.body().feet.y - (4.2 - 2.4)).abs() < 0.05, "free-hang root: {:?}", s.body().feet);
}

/// Walk into a wall (facing +Z) with high profile + Legs from `feet`; return once the straight jump started.
fn straight_jump_at(feet: Vec3) -> Sim {
    let mut s = Sim::new(feet, FACE_PZ);
    s.pad(Vec3::Z, 1.0, true, true);
    assert!(s.run_until(3.0, |s| s.loco().current == ActorContextId::InAir), "never jumped: {:?}", s.body().feet);
    s.pad(Vec3::Z, 0.0, false, false);
    s
}

#[test]
fn knee_height_block_jumps_and_stands_on_top() {
    use crate::player::ledge_moves::{HangEnd, ACT_KNEE_TO_WAIT};
    let mut s = straight_jump_at(Vec3::new(50.0, 0.0, 49.0));
    let j = s.data().air.target.and_then(|t| t.straight).expect("straight jump band");
    assert_eq!((j.flight, j.end), (0x0127_2A69, HangEnd::StandFromKnee), "1.6 m: jumpstraight_to_hangknee");
    assert!(s.run_until(2.0, |s| s.loco().current == ActorContextId::Ledge));
    let mv = s.data().ledge.mv.expect("reception");
    assert_eq!(mv.seq[2].map(|a| a.id), Some(ACT_KNEE_TO_WAIT));
    assert!(s.run_until(4.0, |s| s.loco().current == ActorContextId::Ground), "never stood up");
    assert!((s.body().feet.y - 1.6).abs() < 0.05, "on top of the block: {:?}", s.body().feet);
}

#[test]
fn two_metre_wall_jumps_into_a_wall_hang() {
    let mut s = straight_jump_at(Vec3::new(56.0, 0.0, 49.0));
    let j = s.data().air.target.and_then(|t| t.straight).expect("straight jump band");
    assert_eq!(j.flight, 0x0127_1631, "2.2 m with a wall below: jumpstraight_to_hangwall");
    assert!(s.run_until(2.0, |s| s.loco().current == ActorContextId::Ledge));
    assert!(s.run_until(3.0, |s| s.data().ledge.mv.is_none()));
    assert_eq!(s.data().ledge.hang_type, ledge::LedgeHangType::Wall);
    assert!((s.body().feet.y - 1.1).abs() < 0.05, "wall-hang root 1.1 m below the hands: {:?}", s.body().feet);
}


#[test]
fn shimmy_from_a_free_hang_switches_to_a_wall_hang() {
    use crate::player::ledge_moves::{MoveKind, TO_WALL};
    // the 2.6 m straight jump arrives in a free hang; the first step onto wall below plays hangfree_tr_hangwall
    let mut s = hang_on_jump_up_wall();
    assert!(s.run_until(3.0, |s| s.data().ledge.mv.is_none()));
    assert_eq!(s.data().ledge.hang_type, ledge::LedgeHangType::Free);
    s.pad(Vec3::X, 1.0, false, false);
    assert!(s.run_until(1.0, |s| s.data().ledge.mv.is_some()));
    let mv = s.data().ledge.mv.unwrap();
    assert_eq!(mv.kind, MoveKind::SwitchHang { to_wall: true });
    assert_eq!(mv.seq[0].map(|a| a.id), Some(TO_WALL[0]), "moving left: hangfree_tr_hangwall_left");
    s.pad(Vec3::X, 0.0, false, false);
    assert!(s.run_until(2.0, |s| s.data().ledge.mv.is_none()));
    assert_eq!(s.data().ledge.hang_type, ledge::LedgeHangType::Wall);
    assert!((s.body().feet.y - 1.5).abs() < 0.05, "wall-hang root: {:?}", s.body().feet);
}

#[test]
fn shimmy_onto_an_overhang_switches_to_a_free_hang() {
    use crate::player::ledge_moves::{MoveKind, TO_FREE};
    // wall F (x 60..63) continues as an overhang slab (x 63..66) without wall below
    let mut s = hang_at(Vec3::new(62.3, 2.6, 49.7), Vec3::NEG_Z);
    assert_eq!(s.data().ledge.hang_type, ledge::LedgeHangType::Wall);
    s.pad(Vec3::X, 1.0, false, false); // the player's left
    let switched = s.run_until(6.0, |s| s.data().ledge.mv.is_some_and(|m| m.kind == MoveKind::SwitchHang { to_wall: false }));
    assert!(switched, "no switch: {} hands {:?}", s.data().ledge.last_action, s.data().ledge.hand_l);
    assert_eq!(s.data().ledge.mv.unwrap().seq[0].map(|a| a.id), Some(TO_FREE[0]), "hangwall_tr_hangfree_left");
    s.pad(Vec3::X, 0.0, false, false);
    assert!(s.run_until(2.0, |s| s.data().ledge.mv.is_none()));
    assert_eq!(s.data().ledge.hang_type, ledge::LedgeHangType::Free);
    assert!((s.body().feet.y - 0.2).abs() < 0.05, "free-hang root: {:?}", s.body().feet);
}

#[test]
fn pull_down_from_a_roof_edge_into_a_wall_hang() {
    use crate::player::ledge_moves::{MoveKind, PULLDOWN_DESCENT, PULLDOWN_ORIENT};
    // roof A: x 5..11, h 3.5; stand near its +X edge facing +X (over the drop)
    let mut s = Sim::new(Vec3::new(10.6, 3.5, 12.0), -std::f32::consts::FRAC_PI_2);
    s.run(0.2);
    s.press_legs();
    assert!(s.run_until(0.5, |s| s.loco().current == ActorContextId::Ledge), "no pull-down: {:?}", s.loco().current);
    let mv = s.data().ledge.mv.expect("orientation");
    assert_eq!(mv.kind, MoveKind::PullDown { stage: 1 });
    assert_eq!(mv.seq[0].map(|a| a.id), Some(PULLDOWN_ORIENT[1]));
    assert!(s.run_until(3.0, |s| s.data().ledge.mv.is_some_and(|m| m.kind == MoveKind::PullDown { stage: 2 })));
    assert_eq!(s.data().ledge.mv.unwrap().seq[0].map(|a| a.id), Some(PULLDOWN_DESCENT));
    assert!(s.run_until(4.0, |s| s.data().ledge.mv.is_none()), "never settled: {:?}", s.data().ledge.mv.map(|m| m.kind));
    let l = &s.data().ledge;
    assert_eq!(l.hang_type, ledge::LedgeHangType::Wall);
    assert!(l.normal.dot(Vec3::X) > 0.99, "hanging on the +X face: {:?}", l.normal);
    assert!((l.hand_l.y - 3.5).abs() < 0.05);
    let f = s.body().feet;
    assert!((f.y - 2.4).abs() < 0.05 && (f.x - 11.5).abs() < 0.05, "wall-hang root: {f:?}");
    assert_eq!(s.loco().current, ActorContextId::Ledge);
}

#[test]
fn walking_into_a_roof_edge_stops_then_steps_back() {
    use crate::player::ledge_moves::{LEDGE_STOP_END, LEDGE_STOP_START};
    // roof A: x 5..11, h 3.5; walk (low profile) toward its +X edge
    let mut s = Sim::new(Vec3::new(9.0, 3.5, 12.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, false, false);
    assert!(s.run_until(3.0, |s| s.ground().ledge_stop.is_some()), "never stopped: {:?}", s.body().feet);
    assert_eq!(s.ground().oneshot.map(|o| o.blend.id), Some(LEDGE_STOP_START));
    assert!(s.run_until(1.0, |s| s.ground().ledge_stop.is_some_and(|l| l.ending)));
    assert_eq!(s.ground().oneshot.map(|o| o.blend.id), Some(LEDGE_STOP_END));
    let at_end = s.body().feet;
    assert!(at_end.x < 11.0 - 0.15, "feet stayed behind the edge: {at_end:?}");
    assert!(s.run_until(1.5, |s| s.ground().ledge_stop.is_none()));
    // the end action steps back (~0.5 m), still on the roof, no fall
    assert!(s.body().feet.x < at_end.x - 0.3, "stepped back: {:?}", s.body().feet);
    assert_eq!(s.loco().current, ActorContextId::Ground);
    // still pushing into the edge: no new stop until the stick lets go (PORT lock)
    s.run(1.0);
    assert!(s.ground().ledge_stop.is_none());
    assert_eq!(s.loco().current, ActorContextId::Ground, "held at the edge, no fall");
    assert!(s.body().feet.x < 11.0 - 0.15, "held behind the edge: {:?}", s.body().feet);
    s.pad(Vec3::X, 0.0, false, false);
    s.run(0.1);
    s.pad(Vec3::X, 1.0, false, false);
    assert!(s.run_until(1.0, |s| s.ground().ledge_stop.is_some()), "stops again after letting go");
}

#[test]
fn pull_down_from_the_ledge_stop_uses_the_edge_stop_orientation() {
    use crate::player::ledge_moves::{MoveKind, PULLDOWN_ORIENT};
    let mut s = Sim::new(Vec3::new(9.5, 3.5, 12.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, false, false);
    assert!(s.run_until(3.0, |s| s.ground().ledge_stop.is_some()));
    s.press_legs();
    assert!(s.run_until(0.3, |s| s.loco().current == ActorContextId::Ledge), "no pull-down: {:?}", s.loco().current);
    let mv = s.data().ledge.mv.expect("orientation");
    assert_eq!(mv.kind, MoveKind::PullDown { stage: 1 });
    assert_eq!(mv.seq[0].map(|a| a.id), Some(PULLDOWN_ORIENT[0]));
    assert!(s.run_until(4.0, |s| s.data().ledge.mv.is_none() && s.data().ledge.queue.is_empty()));
    assert_eq!(s.loco().current, ActorContextId::Ledge);
}

#[test]
fn leap_of_faith_into_the_haystack_then_hop_out() {
    use crate::player::hay::{HayPhase, HAYSTACK_FAITH_LANDING, HAYSTACK_HOP_OUT, HAYSTACK_WAIT};
    use crate::player::jump_blend::{FLIGHT_FAITH, TAKEOFF_FAITH};
    // high block: x 27..33, roof 9.5 m; haystack at (37.5, 26), 2.2 m wide, 1 m high
    let mut s = Sim::new(Vec3::new(30.5, 9.5, 26.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, true, true);
    assert!(s.run_until(3.0, |s| s.loco().current == ActorContextId::InAir), "never jumped");
    let air = &s.data().air;
    assert_eq!(air.target_flags, 0x800);
    assert_eq!(air.flight.map(|f| f.id), Some(FLIGHT_FAITH));
    assert!(air.takeoff.is_some_and(|t| TAKEOFF_FAITH.contains(&t.id)));
    s.pad(Vec3::X, 0.0, false, false);
    assert!(s.run_until(5.0, |s| s.loco().current == ActorContextId::HayStack), "never reached the haystack: {:?}", s.loco().current);
    assert_eq!(s.data().hay.action.map(|a| a.id), Some(HAYSTACK_FAITH_LANDING));
    assert!(s.run_until(2.0, |s| s.data().hay.phase == HayPhase::Waiting));
    assert_eq!(s.data().hay.action.map(|a| a.id), Some(HAYSTACK_WAIT));
    let f = s.body().feet;
    assert!((f - Vec3::new(37.5, 0.0, 26.0)).length() < 0.05, "inside the stack: {f:?}");
    // no fall damage: the haystack takes the landing
    assert!(s.ground().last_landing.is_none());
    s.run(0.3);
    s.pad(Vec3::X, 1.0, false, false);
    assert!(s.run_until(1.0, |s| s.loco().current == ActorContextId::Ground));
    assert_eq!(s.ground().oneshot.map(|o| o.blend.id), Some(HAYSTACK_HOP_OUT));
    s.run(0.6);
    assert!(s.body().feet.x > 38.0 && s.body().feet.y.abs() < 0.05, "hopped out: {:?}", s.body().feet);
}

#[test]
fn releasing_the_stick_stops_quickly_like_the_game() {
    use crate::player::jump_blend::{RUN_STOP, RUN_STOP_TO_WAIT};
    // run (high profile), release: RunStop action + its settle into the wait (~0.7 s), short slide
    let mut s = Sim::new(Vec3::new(0.0, 0.0, -30.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, true, false);
    s.run(2.0);
    let x0 = s.body().feet.x;
    s.pad(Vec3::X, 0.0, false, false);
    s.run(1.0 / 60.0 + 1e-4);
    assert!(s.ground().oneshot.is_some_and(|o| RUN_STOP.contains(&o.blend.id)), "run stop playing");
    assert_eq!(s.ground().speed_param, 0.0);
    assert!(s.run_until(1.2, |s| s.ground().oneshot.is_none()), "stop + settle within 1.2 s");
    let slide = s.body().feet.x - x0;
    assert!((0.2..2.0).contains(&slide), "run stop slide {slide}");
    let _ = RUN_STOP_TO_WAIT;
    // walk, release: stopped at once
    let mut s = Sim::new(Vec3::new(0.0, 0.0, -30.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, false, false);
    s.run(1.5);
    let x0 = s.body().feet.x;
    s.pad(Vec3::X, 0.0, false, false);
    s.run(0.3);
    assert!(s.body().feet.x - x0 < 0.05, "walk stop slid {}", s.body().feet.x - x0);
    assert!(s.ground().oneshot.is_none());
}

#[test]
fn pull_up_does_not_cut_through_the_wall() {
    // the root follows the pull-up clips (up, then in over the lip): body points stay outside the wall
    let mut s = hang_on_jump_up_wall();
    assert!(s.run_until(3.0, |s| s.data().ledge.mv.is_none()));
    let c = crate::level::geometry().0;
    s.pad(Vec3::Z, 1.0, false, false);
    let mut worst = 0usize;
    let mut frames = 0;
    for _ in 0..240 {
        s.run(1.0 / 60.0 + 1e-4);
        if s.loco().current != ActorContextId::Ledge {
            break;
        }
        frames += 1;
        let f = s.body().feet;
        worst += [0.3f32, 0.9, 1.5].iter().filter(|&&h| c.point_inside(f + Vec3::Y * h)).count();
    }
    assert!(frames > 10, "pull-up ran {frames} frames");
    assert_eq!(worst, 0, "body inside the wall on {worst} samples");
    assert_eq!(s.loco().current, ActorContextId::Ground);
    assert!((s.body().feet.y - 2.6).abs() < 0.05);
}

#[test]
fn free_hang_against_a_wall_keeps_the_body_out_of_it() {
    // the 2.6 m jump-up wall: free hang with wall under it = WallFree, root 0.5 m out (hangwallfree_wait)
    let mut s = hang_on_jump_up_wall();
    assert!(s.run_until(3.0, |s| s.data().ledge.mv.is_none()));
    let l = &s.data().ledge;
    let f = s.body().feet;
    let out = (f - (l.hand_l + l.hand_r) * 0.5).dot(l.normal);
    assert!((out - 0.5).abs() < 0.05, "root 0.5 m out from the edge: {out}");
    let c = crate::level::geometry().0;
    // body in the clip: feet 0.30-0.39 m and hips 0.36 m in front of the root, i.e. 0.11-0.20 m off the wall
    for h in [0.3f32, 1.2, 1.8] {
        assert!(!c.point_inside(f + Vec3::Y * h - l.normal * 0.3), "body point {h} m up inside the wall");
    }
}

/// Run at a wall face (z 59.25) with high profile, press Legs, keep holding it.
fn wall_run_at(x: f32) -> Sim {
    let mut s = Sim::new(Vec3::new(x, 0.0, 55.5), FACE_PZ);
    s.pad(Vec3::Z, 1.0, true, false);
    // the wall ray is 1.5 m long from the chest (0xE18390): press Legs once within reach
    assert!(s.run_until(2.0, |s| s.body().feet.z > 58.0));
    s.pad(Vec3::Z, 1.0, true, true);
    s.press_legs();
    assert!(s.run_until(1.5, |s| s.loco().current == ActorContextId::Walling), "no wall run: {:?} at {:?}", s.loco().current, s.body().feet);
    s
}

#[test]
fn wall_run_pulls_up_onto_a_low_wall() {
    use crate::player::walling::{WallingSubState, ENTRY_A};
    let mut s = wall_run_at(70.0);
    assert_eq!(s.data().walling.action.map(|a| a.id), Some(ENTRY_A));
    assert_eq!(s.data().walling.sub_state, WallingSubState::EntryA);
    // EntryA ends 0.5 m out from the wall, 1.0 m up (the entry clip's rise = the warp target, 0xE18390)
    assert!(s.run_until(0.5, |s| s.data().walling.sub_state == WallingSubState::EntryB));
    let f = s.body().feet;
    assert!((f.y - 1.0).abs() < 0.05 && (59.25 - f.z - 0.5).abs() < 0.05, "entry root: {f:?}");
    // probe A (ledge 0.8 m above the root): entry_footl_tr_hangknee → hangknee → wait, standing on top
    assert!(s.run_until(3.0, |s| s.loco().current == ActorContextId::Ground), "never on top: {:?}", s.loco().current);
    assert!((s.body().feet.y - 1.8).abs() < 0.05, "on the 1.8 m top: {:?}", s.body().feet);
}

#[test]
fn wall_run_steps_up_then_hangs_from_a_higher_edge() {
    use crate::player::walling::WallingSubState;
    let mut s = wall_run_at(76.0);
    assert!(s.run_until(1.0, |s| s.data().walling.sub_state == WallingSubState::Vertical), "no vertical step");
    assert!(s.run_until(2.0, |s| s.loco().current == ActorContextId::Ledge), "never hung: {:?}", s.loco().current);
    assert!(s.run_until(2.0, |s| s.data().ledge.mv.is_none()));
    let l = &s.data().ledge;
    assert!((l.hand_l.y - 3.8).abs() < 0.05, "hands on the 3.8 m edge: {:?}", l.hand_l);
}

#[test]
fn wall_run_without_a_ledge_drops_back() {
    use crate::player::walling::WallingSubState;
    let mut s = wall_run_at(82.0);
    assert!(s.run_until(1.0, |s| s.data().walling.sub_state == WallingSubState::VerticalEnd), "no vertical end");
    assert!(s.run_until(3.0, |s| s.loco().current == ActorContextId::Ground), "never landed: {:?}", s.loco().current);
    assert!(s.body().feet.y.abs() < 0.05);
}

#[test]
fn releasing_legs_on_the_wall_falls_off() {
    let mut s = wall_run_at(82.0);
    s.pad(Vec3::Z, 1.0, true, false);
    assert!(s.run_until(2.0, |s| s.loco().current == ActorContextId::Ground), "never landed: {:?}", s.loco().current);
}

#[test]
fn pushing_away_from_the_wall_rebounds() {
    let mut s = wall_run_at(82.0);
    s.run(0.15);
    s.pad(Vec3::NEG_Z, 1.0, true, true);
    assert!(s.run_until(0.3, |s| s.loco().current == ActorContextId::InAir), "no rebound: {:?}", s.loco().current);
    let z0 = s.body().feet.z;
    assert!(s.run_until(3.0, |s| s.loco().current == ActorContextId::Ground));
    assert!(s.body().feet.z < z0 - 2.0, "pushed off away from the wall: {:?}", s.body().feet);
}

#[test]
fn walk_across_a_beam_onto_the_far_platform() {
    use crate::player::narrow::{BeamState, BEAM_WALK};
    // platform A x 68..72 (top 4 m), beam x 72..78 at 4 m, platform B x 78..82
    let mut s = Sim::new(Vec3::new(71.0, 4.0, 70.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, false, false);
    assert!(s.run_until(2.0, |s| s.loco().current == ActorContextId::NarrowObject), "never mounted: {:?} {:?}", s.loco().current, s.body().feet);
    assert!(s.run_until(2.0, |s| s.data().narrow.action.is_some_and(|a| a.id == BEAM_WALK)), "never walked: {:?}", s.data().narrow.state);
    // on the line, at the beam's height
    let f = s.body().feet;
    assert!((f.z - 70.0).abs() < 0.01 && (f.y - 4.0).abs() < 0.01, "on the beam: {f:?}");
    assert!(s.run_until(8.0, |s| s.loco().current == ActorContextId::Ground), "never stepped off: {:?} {:?} {:?}", s.loco().current, s.data().narrow.state, s.body().feet);
    s.run(0.5);
    let f = s.body().feet;
    assert!(f.x > 78.0 && (f.y - 4.0).abs() < 0.05, "on platform B: {f:?}");
    let _ = BeamState::Walk;
}

#[test]
fn turn_around_on_a_beam() {
    use crate::player::narrow::BeamState;
    let mut s = Sim::new(Vec3::new(71.0, 4.0, 70.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, false, false);
    assert!(s.run_until(2.0, |s| s.loco().current == ActorContextId::NarrowObject));
    s.run(1.5);
    s.pad(Vec3::X, 0.0, false, false);
    assert!(s.run_until(1.0, |s| s.data().narrow.state == BeamState::Wait));
    s.pad(Vec3::NEG_X, 1.0, false, false);
    assert!(s.run_until(0.5, |s| s.data().narrow.state == BeamState::Turn), "no turn: {:?}", s.data().narrow.state);
    assert!(s.run_until(1.0, |s| s.data().narrow.state != BeamState::Turn));
    assert!(!s.data().narrow.toward_p1, "now facing back toward p0");
    assert!(s.body().forward().dot(Vec3::NEG_X) > 0.99);
}

// ---------------------------------------------------------------- beams from the air, beam jumps, pilotis (RE/05 §2.8)

/// Free-run off platform P (x 68..72, top 3 m) toward the posts at x 74.5 / 77 / 79.5 (z 80).
fn on_pilotis_row() -> Sim {
    let mut s = Sim::new(Vec3::new(69.0, 3.0, 80.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, true, true);
    s
}

/// Switch the player straight into a context (test setup).
fn force(s: &mut Sim, setup: crate::player::TransitionSetup) {
    let w = s.app.world_mut();
    let mut e = w.entity_mut(s.player);
    let (mut loco, mut data) = (e.take::<Locomotion>().unwrap(), e.take::<HumanDataBundle>().unwrap());
    crate::player::switch_context(&mut loco, &mut data, setup);
    e.insert((loco, data));
}

#[test]
fn free_run_onto_a_pilotis_and_hop_along_the_posts() {
    use crate::player::narrow::{BeamState, NarrowKind};
    let mut s = on_pilotis_row();
    assert!(s.run_until(4.0, |s| s.loco().current == ActorContextId::NarrowObject), "never reached a post: {:?} {:?}", s.loco().current, s.body().feet);
    assert_eq!(s.data().narrow.kind, NarrowKind::Pilotis);
    s.pad(Vec3::X, 0.0, true, false);
    assert!(s.run_until(2.0, |s| s.data().narrow.state == BeamState::PilotisWait), "no wait: {:?}", s.data().narrow.state);
    let f = s.body().feet;
    assert!((f - Vec3::new(74.5, 3.0, 80.0)).length() < 0.05, "on the first post top: {f:?}");
    // hop to the next posts with high profile + Legs + the stick
    for x in [77.0f32, 79.5] {
        s.pad(Vec3::X, 1.0, true, false);
        s.press_legs();
        assert!(s.run_until(0.3, |s| s.loco().current == ActorContextId::InAir), "no jump from the post");
        s.pad(Vec3::X, 0.0, true, false);
        assert!(
            s.run_until(3.0, |s| s.data().narrow.state == BeamState::PilotisWait && s.loco().current == ActorContextId::NarrowObject),
            "no arrival at {x}: {:?} {:?}",
            s.loco().current,
            s.body().feet
        );
        let f = s.body().feet;
        assert!((f - Vec3::new(x, 3.0, 80.0)).length() < 0.05, "on the post at {x}: {f:?}");
    }
    // and off onto platform Q
    s.pad(Vec3::X, 1.0, true, false);
    s.press_legs();
    assert!(s.run_until(3.0, |s| s.loco().current == ActorContextId::Ground), "never landed on Q: {:?}", s.loco().current);
    let f = s.body().feet;
    assert!(f.x > 82.0 && (f.y - 3.0).abs() < 0.05, "on platform Q: {f:?}");
}

#[test]
fn pilotis_impulsion_and_jump_on_the_spot() {
    use crate::player::narrow::{BeamState, PilotisEntry, PilotisEntryType};
    let top = Vec3::new(74.5, 3.0, 80.0);
    let mut s = Sim::new(top, -std::f32::consts::FRAC_PI_2);
    force(&mut s, crate::player::TransitionSetup::ToPilotis(PilotisEntry { top, from: top, facing: Vec3::X, kind: PilotisEntryType::FromInAir, foot: 0 }));
    assert!(s.run_until(1.5, |s| s.data().narrow.state == BeamState::PilotisWait));
    s.pad(Vec3::ZERO, 0.0, true, false);
    s.press_legs();
    assert!(s.run_until(0.3, |s| s.data().narrow.state == BeamState::ImpulseIn), "no impulsion: {:?}", s.data().narrow.state);
    assert!(s.run_until(1.0, |s| s.data().narrow.state == BeamState::ImpulseWait));
    s.press_legs();
    assert!(s.run_until(0.5, |s| s.loco().current == ActorContextId::InAir), "no jump on the spot: {:?}", s.data().narrow.state);
    let mut top_y: f32 = 0.0;
    for _ in 0..120 {
        s.run(1.0 / 60.0 + 1e-4);
        top_y = top_y.max(s.body().feet.y);
        if s.loco().current == ActorContextId::NarrowObject {
            break;
        }
    }
    assert!((top_y - 4.0).abs() < 0.05, "rose 1.0 m (beam_jumpstraight_clear): {top_y}");
    assert_eq!(s.loco().current, ActorContextId::NarrowObject, "caught back on the post");
    assert!(s.run_until(1.5, |s| s.data().narrow.state == BeamState::PilotisWait));
    assert!((s.body().feet - top).length() < 0.05);
}

#[test]
fn running_jump_onto_a_beam_mounts_it_straight() {
    use crate::player::narrow::{BeamEntryMode, BeamState, NarrowKind};
    // platform B (x 78..82, top 4) → the free beam x 84.5..90.5 at z 70
    let mut s = Sim::new(Vec3::new(79.0, 4.0, 70.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, true, true);
    assert!(s.run_until(4.0, |s| s.loco().current == ActorContextId::NarrowObject), "never reached the beam: {:?} {:?}", s.loco().current, s.body().feet);
    let n = &s.data().narrow;
    assert_eq!(n.kind, NarrowKind::Beam);
    assert_eq!(n.entry_mode, BeamEntryMode::Straight);
    assert!(n.toward_p1);
    let f = s.body().feet;
    assert!((f.z - 70.0).abs() < 0.01 && (f.y - 4.0).abs() < 0.01 && f.x > 84.5, "on the beam line: {f:?}");
    // keeps walking along it while the stick is held
    s.pad(Vec3::X, 1.0, false, false);
    assert!(s.run_until(2.0, |s| s.data().narrow.state == BeamState::Walk));
}

#[test]
fn falling_onto_a_beam_is_caught() {
    use crate::player::narrow::{BeamEntryMode, BeamState};
    // drop from 1.5 m above the free beam (5 cm off its line)
    let from = Vec3::new(86.0, 5.5, 70.05);
    let mut s = Sim::new(from, -std::f32::consts::FRAC_PI_2);
    force(&mut s, crate::player::TransitionSetup::ToInAir(air::InAirEntry::Fall { from, velocity: Vec3::ZERO, origin: air::FallOrigin::Ground, speed_param: 0.0 }));
    assert!(s.run_until(2.0, |s| s.loco().current != ActorContextId::InAir));
    assert_eq!(s.loco().current, ActorContextId::NarrowObject, "caught on the beam, not a ground landing");
    assert_eq!(s.data().narrow.entry_mode, BeamEntryMode::Reception);
    assert!(s.run_until(2.0, |s| s.data().narrow.state == BeamState::Wait));
    let f = s.body().feet;
    assert!((f.z - 70.0).abs() < 0.01 && (f.y - 4.0).abs() < 0.01, "on the line: {f:?}");
}

#[test]
fn beam_jump_at_a_ledge_above() {
    use crate::player::narrow::{BeamEntry, BeamEntryMode, BeamState, BEAM_IMPULSE_TO_JUMP};
    // on the free beam at x 87.4 facing +X; the slab edge (x 88, top 6.3) is 2.3 m above
    let point = Vec3::new(87.4, 4.0, 70.0);
    let mut s = Sim::new(point, -std::f32::consts::FRAC_PI_2);
    let (p0, p1) = (Vec3::new(84.5, 4.0, 70.0), Vec3::new(90.5, 4.0, 70.0));
    force(&mut s, crate::player::TransitionSetup::ToBeam(BeamEntry { p0, p1, point, from: point, toward_p1: true, mode: BeamEntryMode::Straight, foot: 0, action: None, facing: Vec3::X }));
    s.run(0.2);
    s.pad(Vec3::ZERO, 0.0, true, false);
    s.press_legs();
    assert!(s.run_until(1.0, |s| s.data().narrow.state == BeamState::ImpulseWait), "no impulsion: {:?}", s.data().narrow.state);
    s.press_legs();
    assert!(s.run_until(0.3, |s| s.data().narrow.state == BeamState::JumpOnPlace));
    assert!(s.data().narrow.action.is_some_and(|a| a.id == BEAM_IMPULSE_TO_JUMP), "hand target found");
    assert!(s.run_until(1.0, |s| s.loco().current == ActorContextId::InAir));
    let flight = s.data().air.flight.unwrap().id;
    assert_eq!(flight, 0x516D_52DE, "beam_jumpstraight_to_hangwaist");
    assert!(s.run_until(5.0, |s| s.loco().current == ActorContextId::Ground), "never stood on the slab: {:?} {:?}", s.loco().current, s.body().feet);
    let f = s.body().feet;
    assert!(f.x > 88.0 && (f.y - 6.3).abs() < 0.05, "on top of the slab: {f:?}");
}

// ---------------------------------------------------------------- obstacle collision / lean, look-down (RE/02 §4.2)

#[test]
fn running_into_a_low_wall_leans_on_it_and_releasing_stands_up() {
    use crate::player::collide::{CollideKind, CollidePhase, LEAN_TO_WAIT};
    // the 1.1 m wall at z -4 (x 28..32, 0.4 thick): walk at it along -Z
    let mut s = Sim::new(Vec3::new(30.0, 0.0, -1.0), 0.0);
    s.pad(Vec3::NEG_Z, 1.0, false, false);
    assert!(s.run_until(3.0, |s| s.ground().collide.is_some()), "never collided: {:?}", s.body().feet);
    let c = s.ground().collide.unwrap();
    assert_eq!(c.kind, CollideKind::Hand);
    assert!((c.h - 0.5).abs() < 0.02, "1.1 m → halfway between the 70 and 150 cm clips: {}", c.h);
    // keeps leaning while pushing into the wall
    assert!(s.run_until(3.0, |s| s.ground().collide.is_some_and(|c| c.phase == CollidePhase::Wait)), "no lean wait");
    s.run(1.0);
    assert!(s.ground().collide.is_some(), "still leaning");
    let f = s.body().feet;
    assert!((f.z - (-3.8 + 0.4)).abs() < 0.02, "root 0.4 m out of the face: {f:?}");
    assert!(s.body().forward().dot(Vec3::NEG_Z) > 0.99, "facing the wall");
    // release: lean → wait, then standing
    s.pad(Vec3::ZERO, 0.0, false, false);
    s.run(2.0 / 60.0);
    assert!(s.ground().collide.is_none());
    assert!(s.ground().oneshot.is_some_and(|o| o.blend.id == LEAN_TO_WAIT[0]), "lean_*_wait_tr_l_wait");
}

#[test]
fn leaning_then_pushing_sideways_walks_off_through_the_exit() {
    use crate::player::collide::{CollidePhase, LEAN_EXIT, LEAN_EXIT_TR};
    let mut s = Sim::new(Vec3::new(30.0, 0.0, -1.0), 0.0);
    s.pad(Vec3::NEG_Z, 1.0, false, false);
    assert!(s.run_until(4.0, |s| s.ground().collide.is_some_and(|c| c.phase == CollidePhase::Wait)));
    // stick to the character's left (facing -Z, left = -X)
    s.pad(Vec3::NEG_X, 1.0, false, false);
    s.run(2.0 / 60.0);
    let os = s.ground().oneshot.expect("exit action");
    assert_eq!(os.blend.id, LEAN_EXIT[0], "the left exit");
    let w = os.blend.weights();
    assert!(w[2] + w[3] > 0.99 && w[4..].iter().all(|x| *x == 0.0), "side walk clips: {w:?}");
    assert!(s.run_until(3.0, |s| s.ground().oneshot.is_some_and(|o| o.blend.id == LEAN_EXIT_TR[0])), "exit transition");
    assert!(s.run_until(3.0, |s| s.ground().oneshot.is_none()));
    s.run(1.0);
    assert!(s.body().feet.x < 29.0, "walked off to the left: {:?}", s.body().feet);
}

#[test]
fn bumping_a_knee_high_obstacle_is_the_foot_collide() {
    use crate::player::collide::CollideKind;
    // the 0.6 m box at (9, 0): too high to step onto (0.35 m)
    let mut s = Sim::new(Vec3::new(9.0, 0.0, 3.0), 0.0);
    s.pad(Vec3::NEG_Z, 1.0, false, false);
    assert!(s.run_until(3.0, |s| s.ground().collide.is_some()), "never collided: {:?}", s.body().feet);
    let c = s.ground().collide.unwrap();
    assert_eq!(c.kind, CollideKind::Foot);
    assert!((c.h - 0.5).abs() < 0.02, "0.6 m → halfway between the 50 and 70 cm clips: {}", c.h);
}

#[test]
fn standing_at_a_roof_edge_looks_down_toward_it() {
    // roof A (x -3..3, top 3), standing 0.3 m from its +X edge facing -Z: the edge is on the right
    let mut s = Sim::new(Vec3::new(2.7, 3.0, 12.0), 0.0);
    s.run(0.3);
    let ld = s.ground().look_down.expect("look-down");
    let w = ld.action.weights();
    assert!(w[2] > 0.95 && w[1] == 0.0, "right look-down: {w:?}");
    s.pad(Vec3::NEG_X, 1.0, false, false);
    s.run(0.1);
    assert!(s.ground().look_down.is_none(), "the stick ends it");
}

// ---------------------------------------------------------------- pass-over (RE/04 §4.1.13)

#[test]
fn passover_weights_follow_the_game() {
    use crate::player::jump_blend::{passover_flight_weights, passover_takeoff_weights};
    let mut f = [0.0f32; 5];
    passover_flight_weights(&mut f, 0.4, 0.5, 0);
    assert_eq!(f, [0.3, 0.2, 0.0, 0.3, 0.2]);
    let mut t = [0.0f32; 40];
    passover_takeoff_weights(&mut t, &f, 0);
    assert_eq!((t[0], t[1], t[6], t[7]), (0.3, 0.2, 0.3, 0.2));
    let mut f = [0.0f32; 5];
    passover_flight_weights(&mut f, 0.5, 0.2, 1);
    assert!((f[1] - 0.4).abs() < 1e-6 && (f[2] - 0.4).abs() < 1e-6 && (f[4] - 0.2).abs() < 1e-6);
}

#[test]
fn running_jump_at_a_railing_vaults_it() {
    use crate::player::passover::{PassOverPhase, FLIGHT_PASSOVER, VAULT};
    // the 1 m railing at z 4 (x 38..42, z 3.85..4.15): run along -Z, jump
    let mut s = Sim::new(Vec3::new(40.0, 0.0, 8.5), 0.0);
    s.pad(Vec3::NEG_Z, 1.0, true, false);
    s.run(0.4);
    s.press_legs();
    assert!(s.run_until(0.5, |s| s.loco().current == ActorContextId::InAir), "no jump");
    let t = s.data().air.target.expect("a target");
    assert_eq!(t.type_flags, 2, "pass-over target");
    assert!(FLIGHT_PASSOVER.contains(&s.data().air.flight.unwrap().id));
    assert!(s.run_until(2.0, |s| s.data().ledge.pass_over.is_some_and(|p| p.phase == PassOverPhase::Vault)), "no vault: {:?} {:?}", s.loco().current, s.body().feet);
    let p = s.data().ledge.pass_over.unwrap();
    assert!(VAULT.contains(&p.action.id));
    assert!((p.w - 0.3).abs() < 0.02, "0.3 m thick → w 0.3: {}", p.w);
    assert!((s.body().feet.y - 1.0).abs() < 0.01, "on the top: {:?}", s.body().feet);
    // over and down on the far side
    assert!(s.run_until(4.0, |s| s.loco().current == ActorContextId::Ground && s.body().feet.y < 0.01), "never landed beyond: {:?} {:?}", s.loco().current, s.body().feet);
    assert!(s.body().feet.z < 3.85, "on the far side: {:?}", s.body().feet);
}

// ---------------------------------------------------------------- swing bars (RE/03 §7.10)

#[test]
fn swing_from_bar_to_bar() {
    use crate::player::swing::{SwingPhase, SWING_CYCLE, SWING_LANDING, TAKEOFF_SWING};
    let mut s = Sim::new(Vec3::new(60.0, 1.2, 85.0), std::f32::consts::PI);
    s.pad(Vec3::Z, 1.0, true, false);
    s.run(0.35);
    s.press_legs();
    assert!(s.run_until(0.5, |s| s.loco().current == ActorContextId::InAir), "no jump");
    assert_eq!(s.data().air.target.unwrap().type_flags, crate::player::targets::TARGET_LEDGE_FREE);
    assert!(s.run_until(3.0, |s| s.data().ledge.swing.is_some()), "never swung: {:?} {:?}", s.loco().current, s.body().feet);
    assert_eq!(s.data().ledge.swing.unwrap().action.id, SWING_LANDING);
    // a bar ahead → the swing cycle
    assert!(s.run_until(2.0, |s| s.data().ledge.swing.is_some_and(|w| matches!(w.phase, SwingPhase::Cycle(_)))), "no cycle: {:?}", s.data().ledge.swing.map(|w| w.phase));
    assert_eq!(s.data().ledge.swing.unwrap().action.id, SWING_CYCLE);
    for z in [93.5f32, 97.0] {
        s.press_legs();
        assert!(s.run_until(3.0, |s| s.loco().current == ActorContextId::InAir), "no swing jump toward {z}");
        assert_eq!(s.data().air.takeoff.unwrap().id, TAKEOFF_SWING);
        assert!(s.run_until(3.0, |s| s.data().ledge.swing.is_some() && s.loco().current == ActorContextId::Ledge), "never reached bar {z}: {:?} {:?}", s.loco().current, s.body().feet);
        let mid = (s.data().ledge.hand_l + s.data().ledge.hand_r) * 0.5;
        assert!((mid.z - (z - 0.1)).abs() < 0.05, "hands on bar {z}: {mid:?}");
        assert!(s.run_until(2.0, |s| s.data().ledge.swing.is_some_and(|w| matches!(w.phase, SwingPhase::Cycle(_)))), "no cycle on {z}");
    }
    // and onto the far platform
    s.press_legs();
    assert!(s.run_until(3.0, |s| s.loco().current == ActorContextId::InAir));
    assert!(s.run_until(3.0, |s| s.loco().current == ActorContextId::Ground), "never landed: {:?} {:?}", s.loco().current, s.body().feet);
    assert!(s.body().feet.z > 100.0 && (s.body().feet.y - 1.2).abs() < 0.05, "on the far platform: {:?}", s.body().feet);
}

#[test]
fn landing_on_a_bar_with_nothing_ahead_settles_into_the_hang() {
    use crate::player::swing::{SwingPhase, IMPACT_ELBOW};
    // from the end platform back toward bar 3 (z 97): nothing to jump to beyond bar 2 ... use bar 1 from the start
    // platform facing -Z instead: the start platform side has no bar behind
    let mut s = Sim::new(Vec3::new(60.0, 1.2, 100.5), 0.0);
    s.pad(Vec3::NEG_Z, 1.0, true, false);
    s.run(0.2);
    s.press_legs();
    assert!(s.run_until(3.0, |s| s.data().ledge.swing.is_some()), "never swung");
    s.pad(Vec3::ZERO, 0.0, true, false);
    assert!(s.run_until(2.0, |s| s.data().ledge.swing.is_some_and(|w| !matches!(w.phase, SwingPhase::Landing))));
    let w = s.data().ledge.swing.unwrap();
    // bar 2 lies ahead (3.5 m): a target, so it swings; released stick on a down phase -> stop -> hang
    if matches!(w.phase, SwingPhase::Settle(id, _) if id == IMPACT_ELBOW) {
        assert!(s.run_until(3.0, |s| s.data().ledge.swing.is_none()));
    } else {
        assert!(s.run_until(3.0, |s| s.data().ledge.swing.is_none()), "the released stick stops the swing");
    }
    assert_eq!(s.loco().current, ActorContextId::Ledge, "hanging");
}
