#![windows_subsystem = "windows"]

mod animation;
mod camera;
mod dq;
mod movement;
mod pet;

use animation::PetAnimationPlugin;
use camera::CameraPlugin;
use dq::*;
use movement::MovementPlugin;
use pet::*;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;

use bevy::window::{CompositeAlphaMode, WindowLevel, WindowResolution};

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::NONE))
        .add_plugins(setup_plugins())
        .add_plugins(CameraPlugin)
        .add_plugins(DQPlugin)
        .add_plugins(PetPlugin)
        .add_plugins(MovementPlugin)
        .add_plugins(PetAnimationPlugin)
        .run();
}

fn setup_plugins() -> PluginGroupBuilder {
    let window = Window {
        title: String::from("batates"),
        transparent: false,
        decorations: true,
        window_level: WindowLevel::AlwaysOnTop,
        resolution: WindowResolution::new(500.0, 500.0),
        #[cfg(target_os = "macos")]
        composite_alpha_mode: CompositeAlphaMode::PostMultiplied,
        ..default()
    };
    DefaultPlugins
        .set(ImagePlugin::default_nearest())
        .set(WindowPlugin {
            primary_window: Some(window),
            ..default()
        })
        .set(AssetPlugin {
            mode: AssetMode::Unprocessed,
            ..default()
        })
}
