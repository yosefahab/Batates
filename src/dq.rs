use bevy::prelude::*;
use device_query::DeviceState;
use std::time::Instant;

#[derive(Component, Debug, Clone)]
pub struct DQ {
    pub last_click_time: Instant,
    pub held_start_time: Option<Instant>,
    pub state: DeviceState,
    pub cursor_pos: Vec2,
    pub was_pressed: bool,
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
        held_start_time: None,
        state: DeviceState::new(),
        cursor_pos: Vec2::default(),
        was_pressed: false,
    });
}
