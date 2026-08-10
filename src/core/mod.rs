//! Platform-generic gameplay.
//!
//! Nothing in here may touch windowing, the OS, wall-clock time, ambient
//! randomness, or `cfg(target_os)`. Backends feed it resources and messages;
//! it answers with component state and a desired input region. That constraint
//! is what makes the bulk of the app unit-testable.

pub mod animation;
pub mod brain;
pub mod coords;
pub mod hitbox;
pub mod input;
pub mod movement;
pub mod rng;

/// Ordering for one frame of pet simulation.
///
/// The old code registered systems as unordered tuples, so input could run
/// against the previous frame's cursor position. These run in sequence.
#[derive(bevy::prelude::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PetSystems {
    /// Backend reads the pointer.
    Sample,
    /// Samples become intents.
    Normalize,
    /// Intents and timers advance the state machine.
    Brain,
    /// State entry syncs animation and locomotion.
    Enter,
    /// Locomotion picks a velocity.
    Locomote,
    /// Velocity moves the transform.
    Integrate,
    /// Frames advance.
    Animate,
}
