//! Sprite-sheet frame stepping.
//!
//! The sheet is a strict `rows x columns` grid built by `scripts/make_sprite.py`,
//! where the row is the state's position in [`PetState::ALL`]. That makes every
//! frame index arithmetic, replacing the hand-maintained `match` of absolute
//! indices which had drifted: `Chilling` was declared `(1, 60)` when row 0
//! starts at 0, so its first frame had never rendered.

use bevy::prelude::*;
use std::time::Duration;

use super::brain::{PetState, Playback};

/// Per-pet animation cursor.
#[derive(Component, Debug, Clone)]
pub struct AnimationCursor {
    /// Frame within the current state, 0-based.
    pub frame: u32,
    pub elapsed: Duration,
    /// Set once a `Once` animation has played its last frame.
    pub finished: bool,
}

impl Default for AnimationCursor {
    fn default() -> Self {
        Self {
            frame: 0,
            elapsed: Duration::ZERO,
            finished: false,
        }
    }
}

impl AnimationCursor {
    /// Resets to the first frame. Called on every state entry so the cursor can
    /// never lag behind the brain.
    pub fn restart(&mut self) {
        *self = Self::default();
    }
}

/// Absolute atlas index of frame `n` of the state on `row`.
pub fn frame_index(row: u32, columns: u32, n: u32) -> usize {
    (row * columns + n) as usize
}

/// Atlas index for a state's current cursor position.
pub fn atlas_index(state: PetState, columns: u32, cursor: &AnimationCursor) -> usize {
    frame_index(state.row(), columns, cursor.frame)
}

/// Advances the cursor by `dt`.
///
/// `frames` is the state's frame count and must be at least 1. A `Once`
/// animation stops on its last frame and reports `finished`, which the brain
/// treats as a reason to leave the state.
pub fn step_animation(
    cursor: &mut AnimationCursor,
    frames: u32,
    fps: u8,
    playback: Playback,
    dt: Duration,
) {
    debug_assert!(frames >= 1, "a state needs at least one frame");
    if fps == 0 || frames <= 1 {
        // A single-frame state (Sitting) has nothing to advance; marking it
        // finished lets `Once` single-frame states still terminate.
        cursor.finished = playback == Playback::Once;
        return;
    }

    let per_frame = Duration::from_secs_f32(1.0 / f32::from(fps));
    cursor.elapsed += dt;

    // A loop rather than a single step so a long frame (a stall, a breakpoint)
    // does not silently slow the animation down.
    while cursor.elapsed >= per_frame {
        cursor.elapsed -= per_frame;
        match playback {
            Playback::Loop => {
                cursor.frame = (cursor.frame + 1) % frames;
            }
            Playback::Once => {
                if cursor.frame + 1 >= frames {
                    cursor.finished = true;
                    cursor.elapsed = Duration::ZERO;
                    break;
                }
                cursor.frame += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Regression test for the shipped off-by-one: Chilling is row 0 and its
    /// first frame is index 0, but the old table declared the range as (1, 60).
    #[test]
    fn chilling_starts_at_frame_zero() {
        assert_eq!(frame_index(PetState::Chilling.row(), 61, 0), 0);
    }

    /// The old hardcoded table ended Walking at 434; the arithmetic must agree,
    /// which proves the `match` was replaceable rather than merely removed.
    #[test]
    fn arithmetic_matches_the_old_hardcoded_ranges() {
        let columns = 61;
        assert_eq!(frame_index(PetState::Dragged.row(), columns, 0), 61);
        assert_eq!(frame_index(PetState::Eating.row(), columns, 0), 122);
        assert_eq!(frame_index(PetState::Idle.row(), columns, 0), 183);
        assert_eq!(frame_index(PetState::Jumping.row(), columns, 0), 244);
        assert_eq!(frame_index(PetState::SendingLove.row(), columns, 0), 305);
        assert_eq!(frame_index(PetState::Sitting.row(), columns, 0), 366);
        assert_eq!(frame_index(PetState::Walking.row(), columns, 0), 427);
        assert_eq!(frame_index(PetState::Walking.row(), columns, 7), 434);
    }

    /// A different skin has a different column count; the same row arithmetic
    /// must follow it. Panda is 2450px wide at 50px frames = 49 columns.
    #[test]
    fn column_count_comes_from_the_skin() {
        assert_eq!(frame_index(PetState::Walking.row(), 49, 0), 343);
    }

    #[test]
    fn loop_wraps_to_start() {
        let mut c = AnimationCursor::default();
        // 12 fps => 1 frame per ~83.3ms. Eight frames then wrap.
        for _ in 0..8 {
            step_animation(&mut c, 8, 12, Playback::Loop, ms(84));
        }
        assert_eq!(c.frame, 0);
        assert!(!c.finished, "looping animations never finish");
    }

    #[test]
    fn once_stops_on_last_frame_and_reports_finished() {
        let mut c = AnimationCursor::default();
        for _ in 0..50 {
            step_animation(&mut c, 11, 12, Playback::Once, ms(84));
        }
        assert_eq!(c.frame, 10, "stops on the last frame, not past it");
        assert!(c.finished);
    }

    #[test]
    fn single_frame_state_does_not_advance() {
        let mut c = AnimationCursor::default();
        step_animation(&mut c, 1, 12, Playback::Loop, ms(500));
        assert_eq!(c.frame, 0);
    }

    /// One long tick must advance the same as many short ticks covering the
    /// same span, so a stall drops frames rather than slowing the animation to
    /// a crawl. Exact counts are not asserted: `1/12` is not representable in
    /// f32, so the frame boundary lands a few nanoseconds either side.
    #[test]
    fn a_long_stall_catches_up_rather_than_slowing_down() {
        let mut one_tick = AnimationCursor::default();
        step_animation(&mut one_tick, 61, 12, Playback::Loop, ms(500));

        let mut many_ticks = AnimationCursor::default();
        for _ in 0..10 {
            step_animation(&mut many_ticks, 61, 12, Playback::Loop, ms(50));
        }

        assert_eq!(one_tick.frame, many_ticks.frame);
        assert!(
            one_tick.frame >= 5,
            "expected ~6 frames, got {}",
            one_tick.frame
        );
    }

    #[test]
    fn restart_clears_finished() {
        let mut c = AnimationCursor::default();
        step_animation(&mut c, 2, 12, Playback::Once, ms(500));
        assert!(c.finished);
        c.restart();
        assert_eq!(c.frame, 0);
        assert!(!c.finished);
    }
}
