use bevy::prelude::*;
use std::time::Instant;

#[derive(Component, Debug, Clone)]
pub struct DQ {
    pub last_click_time: Instant,
    pub drag_start_time: Option<Instant>,
}

// so far no issues, this is needed cause DeviceState

pub struct DQPlugin;

impl Plugin for DQPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_dq);
    }
}

fn setup_dq(mut commands: Commands) {
    commands.spawn(DQ {
        last_click_time: Instant::now(),
        drag_start_time: None,
    });
}
