//! The Linux backend.
//!
//! # Why this cannot reuse the desktop backend
//!
//! The macOS and Windows backend is a winit window that happens to be
//! fullscreen, transparent and always-on-top. On Wayland none of those three
//! are properties a client may ask for: xdg-shell has no always-on-top, no way
//! to position a surface, and no way to observe the pointer outside your own
//! surface. They are only reachable through `zwlr_layer_shell_v1`, which winit
//! does not implement (open PRs since 2024). So this backend must own its
//! surface rather than let winit create one, which means running Bevy with
//! `primary_window: None` and handing a raw handle to the renderer.
//!
//! # Status
//!
//! [`probe`] is complete: the app detects an unsupported session and exits with
//! an explanation instead of failing obscurely. Surface creation is not yet
//! implemented, so a supported compositor is detected but not yet drawn on.
//!
//! # Finishing it
//!
//! The remaining work, in order:
//!
//! 1. Create a `zwlr_layer_shell_v1` surface on the `Overlay` layer, anchored to
//!    all four edges, with `set_exclusive_zone(-1)` so it does not reserve space
//!    and `keyboard_interactivity = None` so it never takes focus.
//! 2. Hand its `RawWindowHandle`/`RawDisplayHandle` to Bevy's renderer with
//!    `WindowPlugin { primary_window: None, .. }`.
//! 3. Publish [`ScreenGeometry`](crate::core::coords::ScreenGeometry) from
//!    `wl_output`, and [`SurfaceOrigin`](crate::core::coords::SurfaceOrigin) as
//!    the origin, since the surface covers the output.
//! 4. Feed `wl_pointer` events as
//!    [`PointerSample`](crate::core::input::PointerSample) with
//!    [`PointerAt::Surface`](crate::core::input::PointerAt::Surface); they are
//!    already surface-relative, so no conversion is needed.
//! 5. Apply [`DesiredInputRegion`](crate::core::hitbox::DesiredInputRegion) with
//!    `wl_surface.set_input_region`, gated on `resource_changed`. This is the
//!    piece winit cannot express and the reason the region is computed at all.
//! 6. Select [`InteractionTier::PetOnly`](crate::core::input::InteractionTier),
//!    because click-to-summon needs a global cursor that Wayland will not give.
//!
//! Render everything into the one surface. Do not use `wl_subsurface` per pet:
//! that is what breaks `wl_shimeji` on Hyprland, which violates subsurface
//! clipping, and on Gamescope, which has no `wl_subcompositor`.
//!
//! `bevy_live_wallpaper` v0.5.0 (Bevy 0.19, MIT/Apache) already solves steps 1
//! and 2 for the `Background` layer; `Overlay` is the same code with a different
//! layer argument, but it does not exercise a transparent surface, which is the
//! main unknown.

pub mod probe;

use bevy::prelude::*;

/// Installs the Wayland backend.
///
/// Currently a placeholder: it satisfies the plugin shape so `main` is
/// platform-agnostic, but it does not yet create a surface.
pub struct WaylandBackendPlugin;

impl Plugin for WaylandBackendPlugin {
    fn build(&self, _app: &mut App) {
        warn!(
            "the Wayland layer-shell surface is not implemented yet; \
             see src/platform/wayland/mod.rs for what remains"
        );
    }
}
