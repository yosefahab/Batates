use bevy::prelude::*;

use crate::pet::{Pet, PetState};

const FRICTION: f32 = 0.9; // Adjust between 0 (instant stop) and 1 (no stop)
const SPEED: f32 = 15.0;

#[derive(Component, Debug)]
pub struct Velocity {
    pub value: Vec3,
}

impl Velocity {
    pub fn new(value: Vec3) -> Self {
        Self { value }
    }
}

#[derive(Component, Debug)]
pub struct Acceleration {
    pub value: Vec3,
}

impl Acceleration {
    pub fn new(value: Vec3) -> Self {
        Self { value }
    }
}

#[derive(Bundle)]
pub struct MovingObjectBundle {
    pub velocity: Velocity,
    pub acceleration: Acceleration,
}

pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (update_velocity, update_position, move_toward_target),
        );
    }
}

fn update_velocity(mut query: Query<(&Acceleration, &mut Velocity)>, time: Res<Time>) {
    for (acceleration, mut velocity) in query.iter_mut() {
        velocity.value += acceleration.value * time.delta_seconds();
        velocity.value *= FRICTION; // Apply friction
    }
}
fn update_position(mut query: Query<(&Velocity, &mut Transform, &Pet)>, time: Res<Time>) {
    for (velocity, mut transform, pet) in query.iter_mut() {
        if pet.state != PetState::Dragged {
            transform.translation += velocity.value * SPEED * time.delta_seconds();
        }
        if velocity.value.x > 0.0 {
            transform.scale.x = 1.0; // Facing right
        } else if velocity.value.x < 0.0 {
            transform.scale.x = -1.0; // Facing left (flipped)
        }
    }
}

fn move_toward_target(mut query: Query<(&mut Transform, &mut Velocity, &mut Pet)>) {
    for (transform, mut velocity, mut pet) in query.iter_mut() {
        if let Some(target) = pet.move_target {
            let current_pos = transform.translation.truncate();
            let direction = (target - current_pos).normalize_or_zero();

            velocity.value = direction.extend(0.0) * SPEED;

            // If we're close enough to the target, stop moving
            if target.distance(current_pos) < 2.0 {
                pet.move_target = None;
                velocity.value = Vec3::ZERO; // Stop movement
                pet.set_state(PetState::Idle);
            }
        }
    }
}
