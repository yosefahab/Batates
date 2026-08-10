//! Pointer samples in, gameplay intents out.
//!
//! Two layers so no gameplay system is `cfg`-gated: backends produce
//! [`PointerSample`] in whatever space their platform speaks, and one pure
//! classifier turns a stream of samples into [`Intent`]s.
//!
//! Time arrives as a parameter rather than from `Instant::now()`, which is what
//! made the old `handle_clicks` impossible to test.

use bevy::prelude::*;
use std::time::Duration;

use super::coords::{ScreenLogical, ScreenPhysical, SurfaceLogical, World2d};

/// What a backend can physically deliver.
///
/// Wayland cannot report the cursor outside our own surface, so click-to-summon
/// is unavailable there; macOS and Windows can read the global cursor.
// PetOnly is selected by the Wayland backend.
#[allow(dead_code)]
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionTier {
    /// Global cursor known: clicking bare desktop summons the pet.
    ClickToSummon,
    /// Only input over the pet itself is visible.
    PetOnly,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ButtonMask: u8 {
        const LEFT = 1;
        const RIGHT = 2;
        const MIDDLE = 4;
    }
}

/// Where a pointer sample was taken, in the space its platform speaks.
// Each variant is produced by a different platform backend.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerAt {
    /// Windows: physical pixels.
    Global(ScreenPhysical),
    /// macOS: points, already logical.
    GlobalLogical(ScreenLogical),
    /// Wayland: relative to our surface.
    Surface(SurfaceLogical),
    /// The pointer is not observable right now.
    Absent,
}

/// One frame's pointer reading.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct PointerSample {
    pub at: PointerAt,
    pub buttons: ButtonMask,
    /// Monotonic app time, from `Time::elapsed`. Never wall-clock.
    pub at_time: Duration,
}

/// A gameplay-level request. The only input vocabulary gameplay knows.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub enum Intent {
    /// Walk to a point on bare desktop. `ClickToSummon` tier only.
    Summon {
        to: World2d,
    },
    Grab {
        pet: Entity,
        offset: Vec2,
    },
    DragTo {
        pet: Entity,
        to: World2d,
    },
    Release {
        pet: Entity,
    },
    /// A short click on the pet.
    Pet {
        pet: Entity,
    },
    /// A double click on the pet.
    Poke {
        pet: Entity,
    },
}

/// Timing thresholds, injected so tests can pin them.
#[derive(Resource, Debug, Clone, Copy)]
pub struct GestureConfig {
    pub double_click: Duration,
    pub drag_threshold: Duration,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            double_click: Duration::from_millis(250),
            drag_threshold: Duration::from_millis(125),
        }
    }
}

/// Carried between frames. A resource, not a component: it describes the one
/// pointer, not any particular pet.
#[derive(Resource, Debug, Clone, Default)]
pub struct GestureState {
    pub buttons: ButtonMask,
    pub last_click_at: Option<Duration>,
    pub press_started_at: Option<Duration>,
    /// Set once a press has been promoted to a drag.
    pub dragging: Option<Entity>,
    /// The pet the current press started on.
    pub pressed_on: Option<Entity>,
    pub cursor: Option<World2d>,
}

/// Turns one sample into zero or more intents.
///
/// `hit` is the topmost pet under the cursor, resolved by the caller so this
/// stays independent of the ECS. Returns the next state rather than mutating,
/// so a test can assert on both halves.
pub fn classify(
    state: &GestureState,
    sample: &PointerSample,
    cursor_world: Option<World2d>,
    hit: Option<Entity>,
    tier: InteractionTier,
    cfg: &GestureConfig,
) -> (GestureState, Vec<Intent>) {
    let mut next = state.clone();
    let mut intents = Vec::new();

    next.cursor = cursor_world;

    let was_down = state.buttons.contains(ButtonMask::LEFT);
    let is_down = sample.buttons.contains(ButtonMask::LEFT);
    next.buttons = sample.buttons;

    let now = sample.at_time;

    // Press.
    if !was_down && is_down {
        next.press_started_at = Some(now);
        next.pressed_on = hit;

        match hit {
            Some(pet) => {
                let is_double = state
                    .last_click_at
                    .is_some_and(|prev| now.saturating_sub(prev) < cfg.double_click);
                next.last_click_at = Some(now);
                if is_double {
                    intents.push(Intent::Poke { pet });
                    // Consume the double so a third click is not also a double.
                    next.last_click_at = None;
                    next.press_started_at = None;
                }
            }
            None => {
                next.last_click_at = Some(now);
                if tier == InteractionTier::ClickToSummon
                    && let Some(to) = cursor_world
                {
                    intents.push(Intent::Summon { to });
                }
            }
        }
    }

    // Held: promote to a drag once past the threshold, then follow the cursor.
    if was_down && is_down {
        if let (Some(pet), Some(started), None) =
            (state.pressed_on, state.press_started_at, state.dragging)
            && now.saturating_sub(started) >= cfg.drag_threshold
        {
            next.dragging = Some(pet);
            let offset = cursor_world.map(|c| c.0).unwrap_or_default();
            intents.push(Intent::Grab { pet, offset });
        }
        if let (Some(pet), Some(to)) = (next.dragging, cursor_world) {
            intents.push(Intent::DragTo { pet, to });
        }
    }

    // Release.
    if was_down && !is_down {
        if let Some(pet) = state.dragging {
            intents.push(Intent::Release { pet });
        } else if let (Some(pet), Some(started)) = (state.pressed_on, state.press_started_at)
            && now.saturating_sub(started) < cfg.drag_threshold
        {
            // A short press that never became a drag is affection. A Poke has
            // already cleared press_started_at, so it cannot double-fire here.
            intents.push(Intent::Pet { pet });
        }
        next.dragging = None;
        next.pressed_on = None;
        next.press_started_at = None;
    }

    (next, intents)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn pet_entity() -> Entity {
        Entity::from_raw_u32(1).unwrap()
    }

    fn sample(down: bool, at_time: Duration) -> PointerSample {
        PointerSample {
            at: PointerAt::Surface(SurfaceLogical(Vec2::ZERO)),
            buttons: if down {
                ButtonMask::LEFT
            } else {
                ButtonMask::empty()
            },
            at_time,
        }
    }

    fn world() -> Option<World2d> {
        Some(World2d(Vec2::new(5.0, 5.0)))
    }

    /// Drives a sequence of (down, time, hit) through the classifier.
    fn run(
        steps: &[(bool, u64, Option<Entity>)],
        tier: InteractionTier,
    ) -> (GestureState, Vec<Intent>) {
        let cfg = GestureConfig::default();
        let mut state = GestureState::default();
        let mut all = Vec::new();
        for (down, t, hit) in steps {
            let (next, intents) =
                classify(&state, &sample(*down, ms(*t)), world(), *hit, tier, &cfg);
            state = next;
            all.extend(intents);
        }
        (state, all)
    }

    #[test]
    fn short_click_on_pet_is_affection() {
        let pet = pet_entity();
        let (_, intents) = run(
            &[
                (false, 0, Some(pet)),
                (true, 10, Some(pet)),
                (false, 60, Some(pet)),
            ],
            InteractionTier::ClickToSummon,
        );
        assert_eq!(intents, vec![Intent::Pet { pet }]);
    }

    #[test]
    fn double_click_on_pet_pokes() {
        let pet = pet_entity();
        let (_, intents) = run(
            &[
                (false, 0, Some(pet)),
                (true, 10, Some(pet)),
                (false, 40, Some(pet)),
                (true, 100, Some(pet)),
            ],
            InteractionTier::ClickToSummon,
        );
        assert!(intents.contains(&Intent::Poke { pet }), "{intents:?}");
    }

    #[test]
    fn a_third_click_does_not_re_poke() {
        let pet = pet_entity();
        let (_, intents) = run(
            &[
                (false, 0, Some(pet)),
                (true, 10, Some(pet)),
                (false, 40, Some(pet)),
                (true, 100, Some(pet)),
                (false, 130, Some(pet)),
                (true, 180, Some(pet)),
            ],
            InteractionTier::ClickToSummon,
        );
        let pokes = intents
            .iter()
            .filter(|i| matches!(i, Intent::Poke { .. }))
            .count();
        assert_eq!(pokes, 1, "{intents:?}");
    }

    #[test]
    fn holding_past_the_threshold_grabs_then_drags() {
        let pet = pet_entity();
        let (state, intents) = run(
            &[
                (false, 0, Some(pet)),
                (true, 10, Some(pet)),
                (true, 200, Some(pet)),
                (true, 220, Some(pet)),
            ],
            InteractionTier::ClickToSummon,
        );
        assert!(intents.contains(&Intent::Grab {
            pet,
            offset: Vec2::new(5.0, 5.0)
        }));
        assert!(intents.iter().any(|i| matches!(i, Intent::DragTo { .. })));
        assert_eq!(state.dragging, Some(pet));
    }

    #[test]
    fn releasing_a_drag_releases_rather_than_pets() {
        let pet = pet_entity();
        let (_, intents) = run(
            &[
                (false, 0, Some(pet)),
                (true, 10, Some(pet)),
                (true, 200, Some(pet)),
                (false, 260, Some(pet)),
            ],
            InteractionTier::ClickToSummon,
        );
        assert!(intents.contains(&Intent::Release { pet }));
        assert!(
            !intents.iter().any(|i| matches!(i, Intent::Pet { .. })),
            "a drag must not also count as affection: {intents:?}"
        );
    }

    #[test]
    fn off_pet_click_summons_on_the_desktop_tier() {
        let (_, intents) = run(
            &[(false, 0, None), (true, 10, None)],
            InteractionTier::ClickToSummon,
        );
        assert_eq!(
            intents,
            vec![Intent::Summon {
                to: World2d(Vec2::new(5.0, 5.0))
            }]
        );
    }

    /// Wayland cannot see clicks on bare desktop at all, so the same input
    /// sequence must produce nothing.
    #[test]
    fn off_pet_click_does_nothing_on_the_pet_only_tier() {
        let (_, intents) = run(
            &[(false, 0, None), (true, 10, None)],
            InteractionTier::PetOnly,
        );
        assert!(intents.is_empty(), "{intents:?}");
    }

    #[test]
    fn slow_second_click_is_not_a_double() {
        let pet = pet_entity();
        let (_, intents) = run(
            &[
                (false, 0, Some(pet)),
                (true, 10, Some(pet)),
                (false, 40, Some(pet)),
                (true, 500, Some(pet)),
            ],
            InteractionTier::ClickToSummon,
        );
        assert!(
            !intents.iter().any(|i| matches!(i, Intent::Poke { .. })),
            "{intents:?}"
        );
    }
}
