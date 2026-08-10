//! Pet bounding boxes, hit picking, and input-region aggregation.
//!
//! The old `is_within_bounds` derived the half-extent from `Transform::scale`,
//! which the facing code set to `-1.0` when walking left. A negative half-extent
//! makes the containment test unsatisfiable, so the pet silently became
//! unclickable. Here the extent comes from the frame size and a positive scale,
//! and facing is not part of the calculation at all.

use bevy::prelude::*;

use super::coords::{World2d, world_to_surface};

/// A pet's world-space bounding box.
///
/// `scale` must be positive; facing is expressed with `Sprite::flip_x` and does
/// not affect the box.
pub fn pet_rect_world(centre: Vec2, frame_size: Vec2, scale: f32) -> Rect {
    debug_assert!(scale > 0.0, "scale must be positive; facing uses flip_x");
    let half = frame_size * scale.abs() * 0.5;
    Rect {
        min: centre - half,
        max: centre + half,
    }
}

/// The topmost pet containing `p`.
///
/// `candidates` is `(entity, z, rect)`. Ties break on the higher entity index
/// so picking is deterministic rather than dependent on query order.
pub fn pick_topmost(candidates: &[(Entity, f32, Rect)], p: World2d) -> Option<Entity> {
    candidates
        .iter()
        .filter(|(_, _, rect)| rect.contains(p.0))
        .max_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        })
        .map(|(entity, _, _)| *entity)
}

/// The rects our surface should accept input on, in surface pixels.
///
/// Empty means fully click-through. Only the Wayland backend consumes this:
/// winit's hit-test is all-or-nothing per window, so macOS and Windows keep the
/// whole overlay click-through and read the pointer globally instead. It is
/// still unit-tested on every platform, hence the allow.
#[allow(dead_code)]
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct DesiredInputRegion {
    pub rects: Vec<IRect>,
}

/// Converts world-space pet rects into surface-space integer rects.
///
/// Rounded outward and padded so a pet is never a pixel harder to click than it
/// looks; the compositor unions overlapping rects, so no merging is needed.
#[allow(dead_code)]
pub fn aggregate_input_region(
    pets: impl Iterator<Item = Rect>,
    surface_size: Vec2,
    padding: f32,
) -> DesiredInputRegion {
    let rects = pets
        .map(|rect| {
            // World Y is up and surface Y is down, so the world max maps to the
            // surface min: convert both corners, then re-normalise.
            let a = world_to_surface(World2d(rect.min), surface_size);
            let b = world_to_surface(World2d(rect.max), surface_size);
            let (min, max) = (a.0.min(b.0), a.0.max(b.0));
            let min = (min - padding).floor();
            let max = (max + padding).ceil();
            IRect {
                min: min.as_ivec2(),
                max: max.as_ivec2(),
            }
        })
        .collect();
    DesiredInputRegion { rects }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(cx: f32, cy: f32) -> Rect {
        pet_rect_world(Vec2::new(cx, cy), Vec2::splat(50.0), 1.5)
    }

    #[test]
    fn rect_is_centred_and_scaled() {
        let r = rect(0.0, 0.0);
        assert_eq!(r.min, Vec2::splat(-37.5));
        assert_eq!(r.max, Vec2::splat(37.5));
    }

    /// Regression test for the unclickable-when-facing-left bug: facing must not
    /// be able to invert the box. Scale is always positive here by construction.
    #[test]
    fn rect_stays_valid_regardless_of_facing() {
        let r = rect(10.0, 10.0);
        assert!(r.min.x < r.max.x && r.min.y < r.max.y);
        assert!(r.contains(Vec2::new(10.0, 10.0)));
        assert!(r.contains(Vec2::new(-20.0, 40.0)));
        assert!(!r.contains(Vec2::new(100.0, 100.0)));
    }

    #[test]
    fn pick_returns_none_when_nothing_is_hit() {
        let e = Entity::from_raw_u32(1).unwrap();
        let candidates = [(e, 0.0, rect(0.0, 0.0))];
        assert_eq!(
            pick_topmost(&candidates, World2d(Vec2::new(500.0, 500.0))),
            None
        );
    }

    #[test]
    fn pick_prefers_higher_z() {
        let low = Entity::from_raw_u32(1).unwrap();
        let high = Entity::from_raw_u32(2).unwrap();
        let candidates = [(low, 0.0, rect(0.0, 0.0)), (high, 5.0, rect(10.0, 0.0))];
        // Point inside both boxes must resolve to the higher z.
        assert_eq!(
            pick_topmost(&candidates, World2d(Vec2::new(5.0, 0.0))),
            Some(high)
        );
    }

    #[test]
    fn pick_breaks_ties_deterministically() {
        let a = Entity::from_raw_u32(1).unwrap();
        let b = Entity::from_raw_u32(2).unwrap();
        let candidates = [(a, 1.0, rect(0.0, 0.0)), (b, 1.0, rect(0.0, 0.0))];
        let reversed = [(b, 1.0, rect(0.0, 0.0)), (a, 1.0, rect(0.0, 0.0))];
        let p = World2d(Vec2::ZERO);
        assert_eq!(pick_topmost(&candidates, p), pick_topmost(&reversed, p));
    }

    #[test]
    fn empty_region_for_no_pets() {
        let region = aggregate_input_region(std::iter::empty(), Vec2::new(800.0, 600.0), 0.0);
        assert!(region.rects.is_empty(), "no pets means fully click-through");
    }

    #[test]
    fn region_maps_world_centre_to_surface_centre() {
        let size = Vec2::new(800.0, 600.0);
        let region = aggregate_input_region(std::iter::once(rect(0.0, 0.0)), size, 0.0);
        let r = region.rects[0];
        // World origin is the surface centre: (400, 300) +/- 37.5.
        assert_eq!(r.min, IVec2::new(362, 262));
        assert_eq!(r.max, IVec2::new(438, 338));
    }

    #[test]
    fn region_has_one_rect_per_pet_including_overlaps() {
        let size = Vec2::new(800.0, 600.0);
        let pets = [rect(0.0, 0.0), rect(10.0, 0.0), rect(300.0, 200.0)];
        let region = aggregate_input_region(pets.into_iter(), size, 0.0);
        assert_eq!(region.rects.len(), 3, "compositor unions; we do not merge");
    }

    #[test]
    fn padding_grows_the_rect() {
        let size = Vec2::new(800.0, 600.0);
        let bare = aggregate_input_region(std::iter::once(rect(0.0, 0.0)), size, 0.0).rects[0];
        let padded = aggregate_input_region(std::iter::once(rect(0.0, 0.0)), size, 4.0).rects[0];
        assert_eq!(padded.min, bare.min - IVec2::splat(4));
        assert_eq!(padded.max, bare.max + IVec2::splat(4));
    }
}
