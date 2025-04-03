use crate::animation::AnimationConfig;
use crate::dq::DQ;
use crate::movement::{Acceleration, MovingObjectBundle, Velocity};
use std::time::{Duration, Instant};

use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use device_query::DeviceQuery;

//const IMAGE_PATH: &str = "koala.png";
const MAX_ALLOWED_IDLE: Duration = Duration::from_secs(15);
const NUM_FRAMES: usize = 61;
const NUM_STATES: usize = 8;
const FPS: u8 = 12;
const SPRITE_SIZE: UVec2 = UVec2::splat(50);
const PET_ORIGIN: Vec3 = Vec3::splat(0.0);
const PET_SCALE: Vec3 = Vec3::splat(1.5);
const DRAG_THRESHOLD: Duration = Duration::from_millis(125);
const DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(250);

#[derive(Bundle)]
struct PetBundle {
    pet: Pet,
    sprite: Sprite,
    animation_config: AnimationConfig,
}
pub struct PetPlugin;

impl Plugin for PetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_pet);
        app.add_systems(Update, (update_cursor_pos, update_state, handle_clicks));
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
        SPRITE_SIZE,
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
            sprite: Sprite {
                image: image_handle,
                // custom_size: Some(PET_GAME_SIZE),
                texture_atlas: Some(TextureAtlas {
                    layout: texture_atlas_layout.add(layout),
                    index: first_sprite_index,
                }),
                ..default()
            },
        },
        Transform {
            scale: PET_SCALE,
            translation: PET_ORIGIN,
            ..default()
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

fn is_within_bounds(point: Vec2, sprite_transform: &Transform) -> bool {
    let sprite_pos = sprite_transform.translation.xy();
    let half_size = (SPRITE_SIZE.as_vec2() * sprite_transform.scale.xy()) / 2.0;

    point.x >= sprite_pos.x - half_size.x
        && point.x <= sprite_pos.x + half_size.x
        && point.y >= sprite_pos.y - half_size.y
        && point.y <= sprite_pos.y + half_size.y
}

/// updates the cursor position by getting the cursor's screen position and converts it into
/// world2D position. This assumes the window's resolution is maximum.
fn update_cursor_pos(
    mut window: Query<&mut Window>,
    mut dq: Query<&mut DQ>,
    camera: Query<(&Camera, &GlobalTransform)>,
) {
    let window = match window.get_single_mut() {
        Ok(window) => window,
        Err(_) => return,
    };
    let mut dq = dq.single_mut();
    let (camera, camera_transform) = camera.single();

    let cursor = &dq.state.get_mouse();
    let (mut mouse_x, mut mouse_y) = cursor.coords;
    let (window_x, window_y) = match window.position {
        WindowPosition::At(v) => (v.x, v.y),
        _ => (0, 0),
    };
    mouse_x -= window_x;
    mouse_y -= window_y;
    let cursor_pos = camera
        .viewport_to_world_2d(camera_transform, Vec2::new(mouse_x as f32, mouse_y as f32))
        .unwrap();
    dq.cursor_pos = Vec2::new(cursor_pos.x, cursor_pos.y);
}

fn handle_clicks(
    mut dq: Query<&mut DQ>,
    mut pet_query: Query<(&mut Transform, &mut Pet), With<Pet>>,
) {
    let (mut sprite_transform, mut pet) = pet_query.single_mut();
    let now = Instant::now();
    let mut dq = dq.single_mut();
    let cursor_pos = dq.cursor_pos;
    let cursor_on_pet = is_within_bounds(cursor_pos, &sprite_transform);
    let cursor_pressed = dq.state.get_mouse().button_pressed[1];

    if !dq.was_pressed && cursor_pressed {
        dq.was_pressed = true;
        if cursor_on_pet {
            let time_since_last = now.duration_since(dq.last_click_time);
            dq.last_click_time = now;

            if time_since_last < DOUBLE_CLICK_THRESHOLD {
                pet.set_state(PetState::Jumping);
                return;
            } else {
                dq.held_start_time = Some(now);
            }
        } else {
            dq.was_pressed = false;
            dq.last_click_time = now;
            pet.move_target = Some(cursor_pos);
            pet.set_state(PetState::Walking);
        }
    }

    if cursor_pressed && cursor_on_pet {
        dq.last_click_time = now;
        dq.was_pressed = true;
        if let Some(start_time) = dq.held_start_time {
            if now.duration_since(start_time) > DRAG_THRESHOLD && pet.state != PetState::Dragged {
                pet.set_state(PetState::Dragged);
            }
        }
    }

    // just released
    if dq.was_pressed && !cursor_pressed && cursor_on_pet {
        dq.was_pressed = false;
        if let Some(start_time) = dq.held_start_time {
            if now.duration_since(start_time) < DRAG_THRESHOLD && pet.state != PetState::Dragged {
                pet.set_state(PetState::SendingLove);
            }
        }
        dq.held_start_time = None;
    }

    if pet.state == PetState::Dragged {
        // just released
        if !cursor_pressed {
            dq.was_pressed = false;
            dq.held_start_time = None;
            pet.set_state(PetState::Sitting);
        } else {
            sprite_transform.translation = cursor_pos.extend(sprite_transform.translation.z);
        }
    }
}
