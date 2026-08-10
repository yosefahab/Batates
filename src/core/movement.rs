//! Kinematics, as pure functions.

use bevy::prelude::*;
use std::time::Duration;

/// Which way the sprite faces.
///
/// Applied via `Sprite::flip_x`, never by negating `Transform::scale`. The old
/// code set `scale.x = ±1.0`, which both destroyed the configured 1.5 scale and
/// made the hitbox test unsatisfiable when facing left.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Facing {
    #[default]
    Right,
    Left,
}

impl Facing {
    pub fn flip_x(self) -> bool {
        self == Facing::Left
    }
}

/// Facing from horizontal velocity, with a dead zone so a pet drifting at
/// near-zero speed does not flicker between directions.
pub fn facing_from_velocity(vx: f32, current: Facing) -> Facing {
    const DEAD_ZONE: f32 = 1.0;
    if vx > DEAD_ZONE {
        Facing::Right
    } else if vx < -DEAD_ZONE {
        Facing::Left
    } else {
        current
    }
}

/// Velocity that carries the pet toward `target`, or `None` once it is there.
///
/// The final frame is clamped so the pet lands exactly on the target instead of
/// stepping past it. That matters more than it sounds: an unclamped step
/// overshoots whenever `speed * dt` exceeds the remaining distance, and the pet
/// then oscillates around its destination forever, never arriving.
///
/// Clamping rather than widening an arrival radius keeps this correct at any
/// speed and any frame rate, with no threshold to tune. The last frame travels
/// slower than `speed`, by at most one frame's worth, which is imperceptible.
pub fn steer_toward(from: Vec2, target: Vec2, speed: f32, dt: Duration) -> Option<Vec2> {
    let delta = target - from;
    let distance = delta.length();
    let seconds = dt.as_secs_f32();

    // Already there. Exact equality is reachable because the clamp below lands
    // precisely on the target.
    if distance == 0.0 {
        return None;
    }
    // A stopped clock cannot produce movement, and dividing by it would not
    // either. Report "still travelling" so the walk is not falsely completed.
    if seconds <= 0.0 || speed <= 0.0 {
        return Some(Vec2::ZERO);
    }

    let step = speed * seconds;
    if step >= distance {
        // Land exactly on the target this frame.
        return Some(delta / seconds);
    }
    Some(delta / distance * speed)
}

/// How long a walk of `distance` takes at `speed`.
///
/// Used to guarantee a walk outlives its journey: a duration drawn without
/// regard to distance can expire mid-walk and strand the pet short of a spot
/// the user sent it to.
pub fn travel_time(distance: f32, speed: f32) -> Duration {
    if speed <= 0.0 || !distance.is_finite() || distance <= 0.0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f32(distance / speed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facing_has_a_dead_zone() {
        assert_eq!(facing_from_velocity(50.0, Facing::Left), Facing::Right);
        assert_eq!(facing_from_velocity(-50.0, Facing::Right), Facing::Left);
        // Inside the dead zone the previous facing is kept.
        assert_eq!(facing_from_velocity(0.2, Facing::Left), Facing::Left);
        assert_eq!(facing_from_velocity(-0.2, Facing::Right), Facing::Right);
    }

    #[test]
    fn facing_maps_to_flip_x() {
        assert!(!Facing::Right.flip_x());
        assert!(Facing::Left.flip_x());
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Walks `from` to `target` a frame at a time, returning where it stopped
    /// and how many frames it took.
    fn walk(from: Vec2, target: Vec2, speed: f32, dt: Duration) -> (Vec2, u32) {
        let mut position = from;
        let mut frames = 0;
        while let Some(velocity) = steer_toward(position, target, speed, dt) {
            position += velocity * dt.as_secs_f32();
            frames += 1;
            assert!(frames < 100_000, "never arrived; stuck at {position:?}");
        }
        (position, frames)
    }

    #[test]
    fn steering_reports_a_velocity_while_travelling() {
        let v =
            steer_toward(Vec2::ZERO, Vec2::new(100.0, 0.0), 60.0, ms(16)).expect("still walking");
        assert!((v - Vec2::new(60.0, 0.0)).length() < 1e-4, "{v:?}");
    }

    #[test]
    fn steering_lands_exactly_on_the_target() {
        let target = Vec2::new(100.0, 40.0);
        let (end, _) = walk(Vec2::ZERO, target, 140.0, ms(16));
        assert_eq!(end, target, "must land exactly, not merely close");
    }

    /// A very slow pet must still reach its target rather than stopping short.
    /// This is what a fixed arrival threshold got wrong: at 3 px/s and 60 fps a
    /// frame covers 0.05 px, far below any sensible fixed radius.
    #[test]
    fn a_very_slow_pet_still_arrives_exactly() {
        let target = Vec2::new(5.0, 0.0);
        let (end, frames) = walk(Vec2::ZERO, target, 3.0, ms(16));
        assert_eq!(end, target);
        assert!(frames > 50, "expected a slow crawl, took {frames} frames");
    }

    #[test]
    fn a_stopped_clock_does_not_complete_the_walk() {
        let v = steer_toward(Vec2::ZERO, Vec2::new(10.0, 0.0), 140.0, Duration::ZERO);
        assert_eq!(v, Some(Vec2::ZERO), "no movement, but not arrived either");
    }

    #[test]
    fn zero_speed_does_not_complete_the_walk() {
        let v = steer_toward(Vec2::ZERO, Vec2::new(10.0, 0.0), 0.0, ms(16));
        assert_eq!(v, Some(Vec2::ZERO));
    }

    /// Regression test for the pet circling its target forever: at 140 px/s and
    /// 60 fps a step is 2.33 px, which a fixed 2 px arrival radius never caught.
    #[test]
    fn a_fast_pet_arrives_rather_than_oscillating() {
        let target = Vec2::new(100.0, 0.0);
        let (end, _) = walk(Vec2::ZERO, target, 140.0, ms(16));
        assert_eq!(end, target);
    }

    #[test]
    fn travel_time_scales_with_distance_and_speed() {
        assert_eq!(travel_time(140.0, 140.0), Duration::from_secs(1));
        assert_eq!(travel_time(280.0, 140.0), Duration::from_secs(2));
        assert_eq!(travel_time(280.0, 280.0), Duration::from_secs(1));
    }

    #[test]
    fn travel_time_is_zero_for_degenerate_input() {
        assert_eq!(travel_time(100.0, 0.0), Duration::ZERO);
        assert_eq!(travel_time(0.0, 140.0), Duration::ZERO);
        assert_eq!(travel_time(f32::NAN, 140.0), Duration::ZERO);
    }

    /// Arrival must not depend on frame rate: the same journey ends in the same
    /// place whether it is simulated in long frames or short ones.
    #[test]
    fn arrival_is_frame_rate_independent() {
        let target = Vec2::new(250.0, -80.0);
        for dt in [ms(4), ms(16), ms(33), ms(100)] {
            let (end, _) = walk(Vec2::ZERO, target, 140.0, dt);
            assert_eq!(end, target, "failed at dt {dt:?}");
        }
    }
}
