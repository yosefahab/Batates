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

use bevy::window::{
    CompositeAlphaMode, CursorOptions, MonitorSelection, Window, WindowLevel, WindowMode,
    WindowPosition, WindowResolution,
};

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
    let (monitor_width, monitor_height) = resolution::current_resolution().unwrap();
    let window = Window {
        title: String::from("Batates"),
        transparent: true,
        has_shadow: false,
        decorations: true,
        resizable: false,
        cursor_options: CursorOptions {
            hit_test: false,
            ..default()
        },
        window_level: WindowLevel::AlwaysOnTop,
        mode: WindowMode::Windowed,
        position: WindowPosition::Centered(MonitorSelection::Primary),
        resolution: WindowResolution::new(monitor_width as f32, monitor_height as f32),
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
