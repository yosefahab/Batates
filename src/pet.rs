use crate::animation::AnimationConfig;
use crate::dq::DQ;
use crate::movement::{Acceleration, MovingObjectBundle, Velocity};
use std::time::{Duration, Instant};

use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;

//const IMAGE_PATH: &str = "koala.png";
const STARTING_TRANSLATION: Vec3 = Vec3::new(0.0, 0.0, -20.0);
const MAX_ALLOWED_IDLE: Duration = Duration::from_secs(15);
const NUM_FRAMES: usize = 61;
const NUM_STATES: usize = 8;
const WIDTH: f32 = 50.0;
const HEIGHT: f32 = 50.0;
const SPRITE_SIZE: Vec2 = Vec2::new(WIDTH, HEIGHT);
const FPS: u8 = 12;

const DRAG_THRESHOLD: Duration = Duration::from_millis(200);
const DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(250);

#[derive(Bundle)]
struct PetBundle {
    pet: Pet,
    animation_config: AnimationConfig,
    sprite_bundle: Sprite,
}
pub struct PetPlugin;

impl Plugin for PetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_pet);
        app.add_systems(Update, (update_state, handle_clicks));
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub enum PetState {
    Chilling = 0,
    Dragged = 1,
    Eating = 2,
    Idle = 3,
    Jumping = 4,
    SendingLove = 5,
    Sitting = 6,
    Walking = 7,
}

#[derive(Component, Clone, Debug)]
pub struct Pet {
    pub state: PetState,
    pub idle_timer: Timer,
    pub move_target: Option<Vec2>,
}

impl Default for Pet {
    fn default() -> Self {
        Pet {
            idle_timer: Timer::new(MAX_ALLOWED_IDLE, TimerMode::Once),
            state: PetState::Chilling,
            move_target: None,
        }
    }
}
impl Pet {
    pub fn set_state(&mut self, new_state: PetState) {
        if self.state == new_state {
            return;
        }
        self.state = new_state;
    }
}

fn generate_texture_layout() -> TextureAtlasLayout {
    TextureAtlasLayout::from_grid(
        SPRITE_SIZE.as_uvec2(),
        NUM_FRAMES as u32,
        NUM_STATES as u32,
        None,
        None,
    )
}
fn get_pet_image() -> Image {
    const EMBEDDED_IMAGE: &[u8] = include_bytes!("../assets/koala.png");
    Image::from_buffer(
        EMBEDDED_IMAGE,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::nearest(),
        RenderAssetUsages::default(),
    )
    .expect("Failed to create image")
}
fn spawn_pet(
    mut commands: Commands,
    //asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut texture_atlas_layout: ResMut<Assets<TextureAtlasLayout>>,
) {
    let animation_config = AnimationConfig::new(FPS);
    let pet = Pet::default();
    let first_sprite_index = AnimationConfig::get_frame_range(&pet.state).0;
    let layout = generate_texture_layout();
    let image = get_pet_image();

    let image_handle = images.add(image);
    commands.spawn((
        MovingObjectBundle {
            velocity: Velocity::new(Vec3::ZERO),
            acceleration: Acceleration::new(Vec3::ZERO),
        },
        PetBundle {
            pet,
            animation_config,
            sprite_bundle: Sprite {
                image: image_handle,
                custom_size: Some(SPRITE_SIZE + Vec2::new(20.0, 20.0)),
                // transform: Transform::from_translation(STARTING_TRANSLATION),
                texture_atlas: Some(TextureAtlas {
                    layout: texture_atlas_layout.add(layout),
                    index: first_sprite_index,
                }),
                ..default()
            },
        },
    ));
}
fn update_state(mut pet: Query<&mut Pet>, time: Res<Time>) {
    let mut pet = pet.single_mut();
    let delta = time.delta();
    match pet.state {
        PetState::Idle | PetState::Chilling => {
            pet.idle_timer.tick(delta);
            if pet.idle_timer.finished() {
                pet.set_state(PetState::Sitting);
                pet.idle_timer.reset();
            }
        }
        PetState::Sitting => {
            pet.idle_timer.tick(delta);
            if pet.idle_timer.finished() {
                pet.set_state(PetState::Eating);
                pet.idle_timer.reset();
            }
        }
        _ => {}
    };
}

//fn pet_movement_controls(
//    mut query: Query<(&mut Transform, &mut Velocity), With<Pet>>,
//    mut pet: Query<&mut Pet>,
//    keyboard_input: Res<ButtonInput<KeyCode>>,
//) {
//    let (transform, mut velocity) = query.single_mut();
//    let mut pet = pet.single_mut();
//
//    let mut movement = Vec2::ZERO;
//
//    if keyboard_input.pressed(KeyCode::KeyD) {
//        movement += transform.right().truncate();
//    }
//    if keyboard_input.pressed(KeyCode::KeyA) {
//        movement += transform.left().truncate();
//    }
//    if keyboard_input.pressed(KeyCode::KeyW) {
//        movement += transform.up().truncate();
//    }
//    if keyboard_input.pressed(KeyCode::KeyS) {
//        movement += transform.down().truncate();
//    }
//
//    if movement != Vec2::ZERO {
//        velocity.value = movement.normalize_or_zero().extend(0.0);
//        pet.set_state(PetState::Walking);
//    }
//}
fn is_within_bounds(point: Vec2, center: Vec2, half_size: Vec2) -> bool {
    point.x >= center.x - half_size.x
        && point.x <= center.x + half_size.x
        && point.y >= center.y - half_size.y
        && point.y <= center.y + half_size.y
}
fn handle_clicks(
    windows: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform)>,
    mut pet_query: Query<(&mut Transform, &mut Pet), With<Pet>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut dq: Query<&mut DQ>,
) {
    let window = windows.get_single();
    if window.is_err() {
        return;
    }
    let window = window.unwrap();
    let (camera, camera_transform) = camera.single();
    let (mut sprite_transform, mut pet) = pet_query.single_mut();
    let now = Instant::now();
    let mut dq = dq.single_mut();
    let mut clicked_on_pet = false;

    if let Some(cursor_pos) = window.cursor_position() {
        let sprite_pos = sprite_transform.translation.xy();
        let cursor_pos = camera
            .viewport_to_world_2d(camera_transform, cursor_pos)
            .unwrap();
        let sprite_size = SPRITE_SIZE * sprite_transform.scale.xy();
        let half_size = sprite_size / 2.0;

        if is_within_bounds(cursor_pos, sprite_pos, half_size) {
            clicked_on_pet = true;

            if buttons.just_pressed(MouseButton::Left) {
                let time_since_last = now.duration_since(dq.last_click_time);
                dq.last_click_time = now;

                if time_since_last < DOUBLE_CLICK_THRESHOLD {
                    pet.set_state(PetState::Jumping);
                    return;
                } else {
                    dq.drag_start_time = Some(now);
                }
            }

            if buttons.pressed(MouseButton::Left) {
                dq.last_click_time = now;

                if let Some(start_time) = dq.drag_start_time {
                    if now.duration_since(start_time) > DRAG_THRESHOLD
                        && pet.state != PetState::Dragged
                    {
                        pet.set_state(PetState::Dragged);
                    }
                }
            }

            if buttons.just_released(MouseButton::Left) {
                if let Some(start_time) = dq.drag_start_time {
                    if now.duration_since(start_time) < DRAG_THRESHOLD
                        && pet.state != PetState::Dragged
                    {
                        pet.set_state(PetState::SendingLove);
                    }
                }
                dq.drag_start_time = None;
            }
        }

        if pet.state == PetState::Dragged && buttons.just_released(MouseButton::Left) {
            dq.drag_start_time = None;
            pet.set_state(PetState::Sitting);
        }

        if buttons.just_pressed(MouseButton::Left) && !clicked_on_pet {
            dq.last_click_time = now;
            pet.move_target = Some(cursor_pos);
            pet.set_state(PetState::Walking);
        }

        if pet.state == PetState::Dragged {
            sprite_transform.translation = cursor_pos.extend(sprite_transform.translation.z);
        }
    }
}
