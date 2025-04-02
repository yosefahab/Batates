use bevy::prelude::*;
use std::time::Duration;

use crate::pet::{Pet, PetState};

#[derive(Component, Debug)]
pub struct AnimationConfig {
    pub frame_timer: Timer,
}

impl AnimationConfig {
    pub fn get_frame_range(state: &PetState) -> (usize, usize) {
        /* panda
        Chill: (0, 48)
        Drag: (49, 56)
        Eat: (98, 129)
        Idle: (147, 177)
        Jump: (196, 202)
        Love: (245, 266)
        Sit: (294, 297)
        Walk:  (343, 353)
        49 frames per row
        */
        // koala
        match state {
            PetState::Chilling => (1, 60),       // 61 frames
            PetState::Dragged => (61, 68),       // 8 frames
            PetState::Eating => (122, 145),      // 24 frames
            PetState::Idle => (183, 220),        // 38 frames
            PetState::Jumping => (244, 254),     // 11 frames
            PetState::SendingLove => (305, 365), // 61 frames
            PetState::Sitting => (366, 366),     // 1 frames
            PetState::Walking => (427, 434),     // 8 frames
        }
    }

    pub fn new(fps: u8) -> Self {
        Self {
            frame_timer: Timer::new(
                Duration::from_secs_f32(1.0 / (fps as f32)),
                TimerMode::Repeating,
            ),
        }
    }
}

pub fn update_animation(
    mut query: Query<(&mut AnimationConfig, &mut Sprite)>,
    pet: Query<&Pet>,
    time: Res<Time>,
) {
    let (mut config, mut sprite) = query.single_mut();
    let pet = pet.single();

    config.frame_timer.tick(time.delta());
    if config.frame_timer.just_finished() {
        let (first_sprite_index, last_sprite_index) = AnimationConfig::get_frame_range(&pet.state);

        if sprite.texture_atlas.is_none() {
            return;
        }

        let atlas = sprite.texture_atlas.as_mut().unwrap();

        if atlas.index >= last_sprite_index || atlas.index < first_sprite_index {
            atlas.index = first_sprite_index;
        } else {
            atlas.index += 1;
        }
        config.frame_timer.reset();
    }
}

pub struct PetAnimationPlugin;

impl Plugin for PetAnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_animation);
    }
}
