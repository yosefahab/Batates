//! The macOS and Windows backend: a fullscreen, click-through winit window.
//!
//! Satisfies the backend contract in [`crate::platform`]: it provides
//! [`ScreenGeometry`], [`SurfaceOrigin`] and [`InteractionTier`], and publishes
//! one [`PointerSample`] per frame.
//!
//! It does not touch [`crate::core::hitbox::DesiredInputRegion`]: winit's hit
//! test is all-or-nothing per window, so it cannot express "click-through
//! except over the pets". Since the pointer is read globally here anyway, the
//! overlay simply stays click-through for its whole life.

pub mod pointer;
pub mod window;

use bevy::prelude::*;

use crate::core::PetSystems;
use crate::core::coords::{ScreenGeometry, SurfaceOrigin, physical_to_logical, screen_to_surface};
use crate::core::input::{PointerAt, PointerSample};

pub use window::overlay_window;

/// Installs the desktop backend.
pub struct DesktopBackendPlugin;

impl Plugin for DesktopBackendPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScreenGeometry>()
            .add_systems(
                Update,
                (window::track_monitors, window::track_surface).before(PetSystems::Sample),
            )
            .add_systems(Update, sample_pointer.in_set(PetSystems::Sample));
    }
}

/// Reads the OS cursor once per frame and republishes it in surface space.
///
/// The conversion differs per platform and is the reason the pointer modules
/// return a tagged space rather than a bare pair of numbers: Windows reports
/// physical pixels and must be divided by the scale factor of the monitor the
/// cursor is actually on, while macOS already reports logical points.
fn sample_pointer(
    time: Res<Time>,
    geometry: Res<ScreenGeometry>,
    surface: Option<Res<SurfaceOrigin>>,
    mut samples: MessageWriter<PointerSample>,
) {
    let Some(surface) = surface else { return };

    let at = match pointer::pointer_position() {
        PointerAt::Global(physical) => {
            let logical = physical_to_logical(physical, &geometry);
            PointerAt::Surface(screen_to_surface(logical, *surface))
        }
        PointerAt::GlobalLogical(logical) => {
            PointerAt::Surface(screen_to_surface(logical, *surface))
        }
        // Already surface-relative, or unavailable.
        other => other,
    };

    samples.write(PointerSample {
        at,
        buttons: pointer::buttons(),
        at_time: time.elapsed(),
    });
}
