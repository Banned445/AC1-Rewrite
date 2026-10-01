//! Assassin's Creed (2008) movement — clean-room recreation in Bevy.
//!
//! Stage 1: player locomotion-context framework, input, ground locomotion, jumps/falls/landing on a
//! greybox level. Behaviour is specified by the reverse-engineering notes in ../RE/*.md; no game
//! code or assets are included. Game assets will be loaded at runtime from the user's own install.

mod anim;
mod assets;
mod camera;
mod collision;
mod debug_capture;
mod guidance;
mod hud;
mod ik;
mod input;
mod level;
mod model;
mod player;
#[cfg(test)]
mod sim_tests;
mod tuning;

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "AC1 movement port — stage 1".into(),
                resolution: (1600, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .init_resource::<collision::CollisionWorld>()
        .add_plugins((
            level::LevelPlugin,
            guidance::GuidancePlugin,
            input::InputPlugin,
            camera::CameraPlugin,
            player::PlayerPlugin,
            hud::HudPlugin,
            model::ModelPlugin,
            anim::AnimPlugin,
            ik::IkPlugin,
            debug_capture::DebugCapturePlugin,
            debug_capture::ShotsPlugin,
        ))
        .run();
}
