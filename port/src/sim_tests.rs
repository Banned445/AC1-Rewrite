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
            .add_systems(Update, (ground::update_ground, air::update_air, ledge::update_ledge, climb::update_climb).chain());
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
    // tower at (-12, 14), 5x5, h 8: walk off its +X edge
    let mut s = Sim::new(Vec3::new(-11.0, 8.0, 14.0), -std::f32::consts::FRAC_PI_2);
    s.pad(Vec3::X, 1.0, false, false);
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
    s.pad(Vec3::X, 1.0, false, false);
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
    assert!(s.run_until(4.0, |s| s.data().ledge.mv.is_none()), "reception never ended");
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
