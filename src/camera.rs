use bevy::prelude::*;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera);
    }
}

/// Marks the app's one camera, so the Wayland backend can find it and
/// redirect its output to the offscreen surface it owns.
#[derive(Component)]
pub struct PrimaryCamera;

pub(crate) fn spawn_camera(mut commands: Commands) {
    commands.spawn((Camera2d, PrimaryCamera));
}
