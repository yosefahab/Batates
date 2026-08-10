//! ECS glue: wires the pure `core` logic to Bevy components and systems.
//!
//! Everything decision-making lives in `core`. The systems here only move data
//! between the ECS and those pure functions, which is why none of them contain
//! branching on pet state.

use bevy::prelude::*;
use std::time::Duration;

use crate::config::{Config, paths};
use crate::core::PetSystems;
use crate::core::animation::{AnimationCursor, atlas_index, step_animation};
use crate::core::brain::{
    BrainStep, Locomotion, PetBrain, PetState, StateTable, plan_duration, step_brain,
};
use crate::core::coords::{SurfaceOrigin, surface_to_world};
#[cfg(target_os = "linux")]
use crate::core::hitbox::aggregate_input_region;
use crate::core::hitbox::{pet_rect_world, pick_topmost};
use crate::core::input::{
    GestureConfig, GestureState, Intent, InteractionTier, PointerAt, PointerSample,
};
use crate::core::movement::{Facing, facing_from_velocity, steer_toward, travel_time};
use crate::core::rng::PetRng;
use crate::skin::{Skin, load_or_builtin};

/// Everything one pet's brain tick touches. Named because the tuple is long
/// enough that spelling it inline obscures the system's signature.
type BrainTickData<'a> = (
    &'a mut PetBrain,
    &'a mut AnimationCursor,
    &'a mut PendingInterrupt,
    &'a mut MoveTarget,
    &'a mut Velocity,
    &'a Transform,
);

/// Fraction of the surface that initial pets are scattered over, so several
/// pets do not spawn on top of each other. A quarter keeps them near the middle
/// where they are visible rather than hugging the screen edges.
const SPAWN_SCATTER: f32 = 0.25;

/// Scatter extent used before the surface size is known. Roughly a few pet
/// widths, which is enough to separate them without throwing any off-screen on
/// a small display.
const SPAWN_SCATTER_FALLBACK: Vec2 = Vec2::splat(150.0);

/// How much larger than the hitbox the debug overlay draws its "being dragged"
/// outline, so it reads as a halo rather than overlapping the green box.
const DEBUG_DRAG_OUTLINE_SCALE: f32 = 1.1;

/// Headroom added to a walk's estimated travel time, so a walk ends on arrival
/// rather than on the clock.
const TRAVEL_GRACE: Duration = Duration::from_secs(2);

/// Slack around a pet's box so it is not pixel-precise to click.
#[cfg(target_os = "linux")]
const INPUT_REGION_PADDING: f32 = 4.0;

/// Marker for a pet entity.
#[derive(Component, Debug)]
pub struct Pet;

/// Current velocity in world units per second.
#[derive(Component, Debug, Default)]
pub struct Velocity(pub Vec2);

/// Where a walking pet is heading, if anywhere.
#[derive(Component, Debug, Default)]
pub struct MoveTarget(pub Option<Vec2>);

/// A state change requested by input, consumed by the brain next tick.
#[derive(Component, Debug, Default)]
pub struct PendingInterrupt(pub Option<PetState>);

pub struct PetPlugin;

impl Plugin for PetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GestureState>()
            .add_message::<PointerSample>()
            .add_message::<Intent>()
            .add_message::<SpawnPet>()
            .add_message::<DespawnPet>()
            .add_systems(PreStartup, setup_from_config)
            .add_systems(Startup, request_initial_pets)
            .add_systems(Update, (spawn_requested_pets, despawn_requested_pets))
            .configure_sets(
                Update,
                (
                    PetSystems::Sample,
                    PetSystems::Normalize,
                    PetSystems::Brain,
                    PetSystems::Enter,
                    PetSystems::Locomote,
                    PetSystems::Integrate,
                    PetSystems::Animate,
                )
                    .chain(),
            )
            .add_systems(Update, normalize_input.in_set(PetSystems::Normalize))
            .add_systems(
                Update,
                apply_intents
                    .in_set(PetSystems::Normalize)
                    .after(normalize_input),
            )
            .add_systems(Update, brain_tick.in_set(PetSystems::Brain))
            .add_systems(Update, locomote.in_set(PetSystems::Locomote))
            .add_systems(Update, integrate.in_set(PetSystems::Integrate))
            .add_systems(Update, animate.in_set(PetSystems::Animate))
            .add_systems(PostUpdate, draw_debug_overlay);

        // The input region is only read by the Wayland layer-shell backend:
        // winit's hit test is all-or-nothing per window, so macOS and Windows
        // keep the overlay click-through and read the pointer globally. Building
        // it anywhere else would be per-frame work nothing consumes.
        #[cfg(target_os = "linux")]
        app.init_resource::<crate::core::hitbox::DesiredInputRegion>()
            .add_systems(
                PostUpdate,
                compute_input_region.after(TransformSystems::Propagate),
            );
    }
}

/// Ask for a pet to exist.
#[derive(Message, Debug, Clone, Copy)]
pub struct SpawnPet {
    pub at: Vec2,
}

/// Ask for a pet to stop existing.
#[derive(Message, Debug, Clone, Copy)]
pub struct DespawnPet {
    pub pet: Entity,
}

/// Turns config into the resources the rest of the app reads.
///
/// Runs in `PreStartup` so everything exists before the first pet spawns.
fn setup_from_config(
    mut commands: Commands,
    config: Res<Config>,
    mut images: ResMut<Assets<Image>>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let source = config.skin_source(&paths::skins_dir());
    let (skin, table) = load_or_builtin(&source, &mut images, &mut layouts);
    info!(
        "loaded skin {:?} ({} columns)",
        skin.geometry.name,
        skin.columns()
    );

    // Click-to-summon needs a global cursor, which only some backends have.
    // The config can turn it off, but cannot turn it on where it cannot work:
    // Wayland only ever sees the pointer over our own surface.
    let tier = if config.click_to_summon && !cfg!(target_os = "linux") {
        InteractionTier::ClickToSummon
    } else {
        InteractionTier::PetOnly
    };

    commands.insert_resource(skin);
    commands.insert_resource(table);
    commands.insert_resource(PetRng::from_seed(config.seed));
    commands.insert_resource(config.gestures);
    commands.insert_resource(tier);
}

/// Requests the configured number of pets.
fn request_initial_pets(
    config: Res<Config>,
    surface: Option<Res<SurfaceOrigin>>,
    mut rng: ResMut<PetRng>,
    mut spawns: MessageWriter<SpawnPet>,
) {
    // The window may not have reported its size yet; spread pets over a modest
    // area around the origin in that case rather than stacking them.
    let half = surface
        .map(|s| s.size * SPAWN_SCATTER)
        .unwrap_or(SPAWN_SCATTER_FALLBACK);

    for _ in 0..config.pets.0.get() {
        spawns.write(SpawnPet {
            at: rng.point_in(half),
        });
    }
}

/// Spawns pets on request. The single path by which a pet comes into existence.
fn spawn_requested_pets(
    mut commands: Commands,
    mut requests: MessageReader<SpawnPet>,
    config: Res<Config>,
    skin: Res<Skin>,
    table: Res<StateTable>,
    mut rng: ResMut<PetRng>,
    existing: Query<&Transform, With<Pet>>,
) {
    // Draw order lives in translation.z alone: it is both what the renderer
    // sorts by and what hit-picking uses, so storing it twice invites drift.
    let first_order = existing
        .iter()
        .map(|t| t.translation.z as u32)
        .max()
        .map_or(0, |m| m + 1);

    for (next_order, request) in (first_order..).zip(requests.read()) {
        let state = PetState::Chilling;
        let cursor = AnimationCursor::default();
        let planned = plan_duration(table.get(state), &mut rng);

        commands.spawn((
            Pet,
            PetBrain::new(state, planned),
            AnimationCursor::default(),
            Velocity::default(),
            MoveTarget::default(),
            PendingInterrupt::default(),
            Facing::default(),
            Sprite::from_atlas_image(
                skin.image.clone(),
                TextureAtlas {
                    layout: skin.layout.clone(),
                    index: atlas_index(state, skin.columns(), &cursor),
                },
            ),
            // Scale is set once here and never written again: the old code drove
            // facing through scale.x, which broke both the size and the hitbox.
            Transform::from_translation(request.at.extend(next_order as f32))
                .with_scale(Vec3::splat(config.scale.0)),
        ));
    }
}

fn despawn_requested_pets(mut commands: Commands, mut requests: MessageReader<DespawnPet>) {
    for request in requests.read() {
        commands.entity(request.pet).despawn();
    }
}

/// Pointer samples become intents.
// Bevy systems declare their dependencies as parameters; splitting this into a
// SystemParam struct would hide them without reducing the coupling.
#[allow(clippy::too_many_arguments)]
fn normalize_input(
    mut samples: MessageReader<PointerSample>,
    mut intents: MessageWriter<Intent>,
    mut gesture: ResMut<GestureState>,
    cfg: Res<GestureConfig>,
    tier: Res<InteractionTier>,
    surface: Option<Res<SurfaceOrigin>>,
    skin: Res<Skin>,
    pets: Query<(Entity, &Transform), With<Pet>>,
) {
    let Some(surface) = surface else { return };

    for sample in samples.read() {
        let cursor_world = match sample.at {
            PointerAt::Surface(p) => Some(surface_to_world(p, surface.size)),
            // The other spaces are converted by their backend before sending;
            // they are handled when those backends land.
            _ => None,
        };

        let candidates: Vec<(Entity, f32, Rect)> = pets
            .iter()
            .map(|(entity, transform)| {
                (
                    entity,
                    transform.translation.z,
                    pet_rect_world(
                        transform.translation.truncate(),
                        skin.frame_size(),
                        transform.scale.x,
                    ),
                )
            })
            .collect();

        let hit = cursor_world.and_then(|p| pick_topmost(&candidates, p));

        let (next, produced) =
            crate::core::input::classify(&gesture, sample, cursor_world, hit, *tier, &cfg);
        *gesture = next;
        for intent in produced {
            intents.write(intent);
        }
    }
}

/// Intents become per-pet interrupts and drag positions.
fn apply_intents(
    mut intents: MessageReader<Intent>,
    mut pets: Query<
        (
            Entity,
            &mut PendingInterrupt,
            &mut Transform,
            &mut MoveTarget,
        ),
        With<Pet>,
    >,
) {
    for intent in intents.read() {
        match *intent {
            Intent::Summon { to } => {
                // Only the nearest pet answers, so a group does not pile onto
                // the same point.
                let nearest = pets
                    .iter()
                    .min_by(|a, b| {
                        let da = a.2.translation.truncate().distance_squared(to.0);
                        let db = b.2.translation.truncate().distance_squared(to.0);
                        da.total_cmp(&db)
                    })
                    .map(|(entity, ..)| entity);

                if let Some(pet) = nearest
                    && let Ok((_, mut interrupt, _, mut target)) = pets.get_mut(pet)
                {
                    target.0 = Some(to.0);
                    interrupt.0 = Some(PetState::Walking);
                }
            }
            Intent::Grab { pet, .. } => {
                if let Ok((_, mut interrupt, _, _)) = pets.get_mut(pet) {
                    interrupt.0 = Some(PetState::Dragged);
                }
            }
            Intent::DragTo { pet, to } => {
                if let Ok((_, _, mut transform, _)) = pets.get_mut(pet) {
                    transform.translation.x = to.0.x;
                    transform.translation.y = to.0.y;
                }
            }
            Intent::Release { pet } => {
                if let Ok((_, mut interrupt, _, _)) = pets.get_mut(pet) {
                    interrupt.0 = Some(PetState::Sitting);
                }
            }
            Intent::Pet { pet } => {
                if let Ok((_, mut interrupt, _, _)) = pets.get_mut(pet) {
                    interrupt.0 = Some(PetState::SendingLove);
                }
            }
            Intent::Poke { pet } => {
                if let Ok((_, mut interrupt, _, _)) = pets.get_mut(pet) {
                    interrupt.0 = Some(PetState::Jumping);
                }
            }
        }
    }
}

/// Advances every pet's state machine and syncs animation and locomotion on entry.
///
/// Entry handling lives here rather than in a separate system so the cursor and
/// the target can never be one frame out of step with the brain.
fn brain_tick(
    time: Res<Time>,
    table: Res<StateTable>,
    skin: Res<Skin>,
    config: Res<Config>,
    mut rng: ResMut<PetRng>,
    surface: Option<Res<SurfaceOrigin>>,
    mut pets: Query<BrainTickData, With<Pet>>,
) {
    let dt = time.delta();
    let frame_size = skin.frame_size();
    for (mut brain, mut cursor, mut interrupt, mut target, mut velocity, transform) in &mut pets {
        let def = table.get(brain.state);
        // Entering a walk always assigns a target, so its absence means
        // `locomote` cleared it on arrival.
        let arrived = target.0.is_none();
        let step = step_brain(
            &mut brain,
            def,
            interrupt.0.take(),
            cursor.finished,
            arrived,
            dt,
            &mut rng,
        );

        let BrainStep::Enter(entered) = step else {
            continue;
        };

        cursor.restart();
        brain.planned = plan_duration(table.get(entered), &mut rng);

        match table.get(entered).locomotion {
            Locomotion::Still | Locomotion::Held => {
                velocity.0 = Vec2::ZERO;
                target.0 = None;
            }
            Locomotion::Walk { speed } => {
                // A summon has already set a target; otherwise wander.
                if target.0.is_none()
                    && let Some(surface) = surface.as_deref()
                {
                    let half = surface.size * 0.5 - frame_size * config.scale.0;
                    target.0 = Some(rng.point_in(half.max(Vec2::ZERO)));
                }

                // Give the walk long enough to actually get there. Without this
                // the drawn 2-8s duration can expire mid-journey and the pet
                // abandons a spot the user explicitly sent it to. As with a
                // `Once` animation, the drawn duration stays only as a backstop.
                if let Some(to) = target.0
                    && speed > 0.0
                {
                    let distance = (to - transform.translation.truncate()).length();
                    brain.planned = brain
                        .planned
                        .max(travel_time(distance, speed) + TRAVEL_GRACE);
                }
            }
        }
    }
}

/// Turns locomotion into velocity.
fn locomote(
    time: Res<Time>,
    table: Res<StateTable>,
    mut pets: Query<(&PetBrain, &mut Velocity, &mut MoveTarget, &Transform), With<Pet>>,
) {
    let dt = time.delta();
    for (brain, mut velocity, mut target, transform) in &mut pets {
        let Locomotion::Walk { speed } = table.get(brain.state).locomotion else {
            continue;
        };
        let Some(to) = target.0 else { continue };

        match steer_toward(transform.translation.truncate(), to, speed, dt) {
            Some(v) => velocity.0 = v,
            None => {
                target.0 = None;
                velocity.0 = Vec2::ZERO;
            }
        }
    }
}

/// Applies velocity to position, and updates facing.
fn integrate(
    time: Res<Time>,
    table: Res<StateTable>,
    mut pets: Query<
        (
            &PetBrain,
            &mut Transform,
            &mut Velocity,
            &mut Facing,
            &mut Sprite,
        ),
        With<Pet>,
    >,
) {
    let dt = time.delta();
    for (brain, mut transform, mut velocity, mut facing, mut sprite) in &mut pets {
        // A held pet is positioned by the pointer, not by physics.
        if matches!(table.get(brain.state).locomotion, Locomotion::Held) {
            velocity.0 = Vec2::ZERO;
            continue;
        }

        transform.translation.x += velocity.0.x * dt.as_secs_f32();
        transform.translation.y += velocity.0.y * dt.as_secs_f32();

        let next = facing_from_velocity(velocity.0.x, *facing);
        if next != *facing {
            *facing = next;
        }
        sprite.flip_x = facing.flip_x();
    }
}

/// Advances animation frames.
fn animate(
    time: Res<Time>,
    table: Res<StateTable>,
    skin: Res<Skin>,
    mut pets: Query<(&PetBrain, &mut AnimationCursor, &mut Sprite), With<Pet>>,
) {
    let dt = time.delta();
    for (brain, mut cursor, mut sprite) in &mut pets {
        let def = table.get(brain.state);
        step_animation(&mut cursor, def.frames, def.fps, def.playback, dt);

        let Some(atlas) = sprite.texture_atlas.as_mut() else {
            continue;
        };
        atlas.index = atlas_index(brain.state, skin.columns(), &cursor);
    }
}

/// Recomputes where the surface should accept input.
#[cfg(target_os = "linux")]
///
/// Written with `set_if_neq` so backends can gate on change detection: Bevy
/// warns and reverts if a platform rejects the value, and rewriting it every
/// frame would loop on that.
pub(crate) fn compute_input_region(
    surface: Option<Res<SurfaceOrigin>>,
    skin: Res<Skin>,
    pets: Query<&Transform, With<Pet>>,
    mut region: ResMut<crate::core::hitbox::DesiredInputRegion>,
) {
    let Some(surface) = surface else { return };
    let rects = pets.iter().map(|transform| {
        pet_rect_world(
            transform.translation.truncate(),
            skin.frame_size(),
            transform.scale.x,
        )
    });
    let next = aggregate_input_region(rects, surface.size, INPUT_REGION_PADDING);
    region.set_if_neq(next);
}

/// Draws each pet's hitbox and the cursor position the app is working from.
///
/// Enabled with `[debug] overlay = true`. The gap between the crosshair and the
/// real pointer is the coordinate error, so this is the tool for diagnosing
/// "I cannot click the pet".
fn draw_debug_overlay(
    mut gizmos: Gizmos,
    config: Res<Config>,
    skin: Res<Skin>,
    gesture: Res<GestureState>,
    pets: Query<(&Transform, &PetBrain), With<Pet>>,
) {
    if !config.debug_overlay {
        return;
    }

    for (transform, brain) in &pets {
        let rect = pet_rect_world(
            transform.translation.truncate(),
            skin.frame_size(),
            transform.scale.x,
        );
        // Green box: exactly the region that accepts clicks.
        gizmos.rect_2d(
            Isometry2d::from_translation(rect.center()),
            rect.size(),
            Color::srgb(0.0, 1.0, 0.2),
        );
        // A dragged pet turns the box magenta so state is visible too.
        if brain.state == PetState::Dragged {
            gizmos.rect_2d(
                Isometry2d::from_translation(rect.center()),
                rect.size() * DEBUG_DRAG_OUTLINE_SCALE,
                Color::srgb(1.0, 0.0, 1.0),
            );
        }
    }

    // Red crosshair: where the app believes the pointer is.
    if let Some(cursor) = gesture.cursor {
        let p = cursor.0;
        gizmos.circle_2d(
            Isometry2d::from_translation(p),
            8.0,
            Color::srgb(1.0, 0.1, 0.1),
        );
        gizmos.line_2d(
            p - Vec2::X * 20.0,
            p + Vec2::X * 20.0,
            Color::srgb(1.0, 0.1, 0.1),
        );
        gizmos.line_2d(
            p - Vec2::Y * 20.0,
            p + Vec2::Y * 20.0,
            Color::srgb(1.0, 0.1, 0.1),
        );
    }
}
