//! The coordinate pipeline.
//!
//! Four distinct spaces, each with its own type, because mixing them silently
//! is exactly what produced the DPI bug: the old code fed physical Windows
//! pixels into an API expecting logical ones, and subtracted a window position
//! that was always `(0, 0)`.
//!
//! Entry point differs per platform, which is why the types are not
//! interchangeable:
//!
//! ```text
//! Windows   GetCursorPos       -> ScreenPhysical -> ScreenLogical -> SurfaceLogical -> World2d
//! macOS     CGEventGetLocation -> ScreenLogical  -> SurfaceLogical -> World2d
//! Wayland   wl_pointer.motion  -> SurfaceLogical -> World2d
//! ```

use bevy::prelude::*;

/// Physical pixels in the OS virtual-desktop space, origin at the primary
/// monitor's top-left, Y down. What Windows `GetCursorPos` and
/// `Monitor::physical_position` speak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPhysical(pub IVec2);

/// Logical (DPI-divided) pixels in the OS virtual-desktop space, Y down.
/// What macOS `CGEventGetLocation` already speaks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenLogical(pub Vec2);

/// Logical pixels relative to our surface's top-left, Y down.
/// Where Wayland pointer events enter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceLogical(pub Vec2);

/// Bevy world units. Origin at the surface centre, Y up. 1 unit == 1 logical px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct World2d(pub Vec2);

/// One display, as reported by the platform.
// Populated from Bevy's `Monitor` by the desktop backend; unit-tested here.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorGeometry {
    pub physical_position: IVec2,
    pub physical_size: UVec2,
    pub scale_factor: f64,
}

#[allow(dead_code)]
impl MonitorGeometry {
    /// Whether `p` falls inside this monitor's physical rect.
    fn contains(&self, p: ScreenPhysical) -> bool {
        let min = self.physical_position;
        let max = min + self.physical_size.as_ivec2();
        p.0.x >= min.x && p.0.x < max.x && p.0.y >= min.y && p.0.y < max.y
    }

    /// This monitor's origin expressed in logical pixels.
    fn logical_origin(&self) -> Vec2 {
        self.physical_position.as_vec2() / self.scale_factor as f32
    }
}

/// All displays. Provided by the platform backend, read-only for gameplay.
#[allow(dead_code)]
#[derive(Resource, Debug, Clone, Default)]
pub struct ScreenGeometry {
    pub monitors: Vec<MonitorGeometry>,
    pub primary: usize,
}

#[allow(dead_code)]
impl ScreenGeometry {
    /// The monitor whose physical rect contains `p`.
    ///
    /// Falls back to the primary monitor: the cursor can legitimately sit in
    /// dead space between monitors of differing heights, and a `None` there
    /// would force every caller to handle a case that has an obvious answer.
    /// Returns `None` only when no monitors are known at all.
    pub fn monitor_containing(&self, p: ScreenPhysical) -> Option<&MonitorGeometry> {
        self.monitors
            .iter()
            .find(|m| m.contains(p))
            .or_else(|| self.monitors.get(self.primary))
    }
}

/// Where our surface sits, and how big it is, in logical pixels.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct SurfaceOrigin {
    pub origin: ScreenLogical,
    pub size: Vec2,
}

/// Converts physical to logical using the scale factor of the monitor the point
/// is actually on.
///
/// A single global divide is wrong on mixed-DPI setups (a 4K@200% beside a
/// 1080p@100%), which is why this needs the whole geometry rather than one
/// scale factor.
#[allow(dead_code)]
pub fn physical_to_logical(p: ScreenPhysical, geo: &ScreenGeometry) -> ScreenLogical {
    let Some(monitor) = geo.monitor_containing(p) else {
        // No monitors known yet: scale factor 1 is the only defensible guess.
        return ScreenLogical(p.0.as_vec2());
    };
    let offset_physical = (p.0 - monitor.physical_position).as_vec2();
    let offset_logical = offset_physical / monitor.scale_factor as f32;
    ScreenLogical(monitor.logical_origin() + offset_logical)
}

/// Rebases a desktop-space logical point onto our surface.
pub fn screen_to_surface(p: ScreenLogical, surface: SurfaceOrigin) -> SurfaceLogical {
    SurfaceLogical(p.0 - surface.origin.0)
}

/// Surface pixels (Y down, origin top-left) to world units (Y up, origin centre).
///
/// This encodes the camera invariant: a fixed 2D orthographic camera with no
/// transform and window-size scaling. It replaces `Camera::viewport_to_world_2d`,
/// which returns a `Result` and hides that invariant behind a query. If the
/// camera ever moves or zooms, this function becomes wrong.
pub fn surface_to_world(p: SurfaceLogical, surface_size: Vec2) -> World2d {
    World2d(Vec2::new(
        p.0.x - surface_size.x * 0.5,
        surface_size.y * 0.5 - p.0.y,
    ))
}

/// Inverse of [`surface_to_world`]. Used when building the Wayland input region.
#[allow(dead_code)]
pub fn world_to_surface(p: World2d, surface_size: Vec2) -> SurfaceLogical {
    SurfaceLogical(Vec2::new(
        p.0.x + surface_size.x * 0.5,
        surface_size.y * 0.5 - p.0.y,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo() -> ScreenGeometry {
        ScreenGeometry {
            monitors: vec![
                // Primary: 2880x1800 physical at 2x -> 1440x900 logical at origin.
                MonitorGeometry {
                    physical_position: IVec2::new(0, 0),
                    physical_size: UVec2::new(2880, 1800),
                    scale_factor: 2.0,
                },
                // Secondary to the LEFT: 1920x1080 at 1x, negative x.
                MonitorGeometry {
                    physical_position: IVec2::new(-1920, 0),
                    physical_size: UVec2::new(1920, 1080),
                    scale_factor: 1.0,
                },
            ],
            primary: 0,
        }
    }

    #[test]
    fn surface_world_round_trips() {
        let size = Vec2::new(1440.0, 900.0);
        for p in [
            Vec2::ZERO,
            Vec2::new(1440.0, 900.0),
            Vec2::new(720.0, 450.0),
            Vec2::new(13.5, 887.25),
        ] {
            let round = world_to_surface(surface_to_world(SurfaceLogical(p), size), size);
            assert!((round.0 - p).length() < 1e-4, "{p:?} -> {round:?}");
        }
    }

    #[test]
    fn surface_centre_is_world_origin() {
        let size = Vec2::new(1440.0, 900.0);
        let centre = surface_to_world(SurfaceLogical(size * 0.5), size);
        assert_eq!(centre.0, Vec2::ZERO);
    }

    #[test]
    fn surface_top_left_is_upper_left_in_world() {
        let size = Vec2::new(1440.0, 900.0);
        let tl = surface_to_world(SurfaceLogical(Vec2::ZERO), size);
        // Y is up in world space, so the top-left corner has POSITIVE y.
        assert_eq!(tl.0, Vec2::new(-720.0, 450.0));
    }

    #[test]
    fn hidpi_primary_divides_by_scale_factor() {
        let logical = physical_to_logical(ScreenPhysical(IVec2::new(1440, 900)), &geo());
        assert_eq!(logical.0, Vec2::new(720.0, 450.0));
    }

    #[test]
    fn secondary_monitor_at_negative_x_uses_its_own_scale() {
        // Physical (-960, 540) is the centre of the 1x monitor. At 1x its
        // logical origin is (-1920, 0), so the point stays (-960, 540).
        let logical = physical_to_logical(ScreenPhysical(IVec2::new(-960, 540)), &geo());
        assert_eq!(logical.0, Vec2::new(-960.0, 540.0));
    }

    #[test]
    fn mixed_dpi_does_not_use_one_global_scale() {
        // The same physical x on two monitors must NOT map to the same logical
        // offset, because their scale factors differ. This is the regression
        // test for the old single-divide approach.
        let on_primary = physical_to_logical(ScreenPhysical(IVec2::new(100, 100)), &geo());
        let on_secondary = physical_to_logical(ScreenPhysical(IVec2::new(-1820, 100)), &geo());
        assert_eq!(on_primary.0, Vec2::new(50.0, 50.0)); // divided by 2
        assert_eq!(on_secondary.0, Vec2::new(-1820.0, 100.0)); // divided by 1
    }

    #[test]
    fn point_in_dead_space_falls_back_to_primary() {
        // Below the short secondary monitor but outside every rect.
        let p = ScreenPhysical(IVec2::new(-500, 1500));
        let m = geo().monitor_containing(p).copied();
        assert_eq!(m, Some(geo().monitors[0]));
    }

    #[test]
    fn no_monitors_is_identity_not_a_panic() {
        let empty = ScreenGeometry::default();
        let logical = physical_to_logical(ScreenPhysical(IVec2::new(7, 9)), &empty);
        assert_eq!(logical.0, Vec2::new(7.0, 9.0));
    }

    #[test]
    fn screen_to_surface_subtracts_origin() {
        let surface = SurfaceOrigin {
            origin: ScreenLogical(Vec2::new(100.0, 50.0)),
            size: Vec2::new(1440.0, 900.0),
        };
        let p = screen_to_surface(ScreenLogical(Vec2::new(150.0, 80.0)), surface);
        assert_eq!(p.0, Vec2::new(50.0, 30.0));
    }
}
