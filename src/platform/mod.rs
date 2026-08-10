//! Platform backends.
//!
//! A backend owns the window (or surface) and the pointer, and nothing else.
//! The contract, in full:
//!
//! | Direction | Item | Meaning |
//! |---|---|---|
//! | provides | [`ScreenGeometry`](crate::core::coords::ScreenGeometry) | monitor rects and scale factors |
//! | provides | [`SurfaceOrigin`](crate::core::coords::SurfaceOrigin) | where our surface sits, logical |
//! | writes | [`PointerSample`](crate::core::input::PointerSample) | one per frame, in surface space |
//! | reads | [`DesiredInputRegion`](crate::core::hitbox::DesiredInputRegion) | where to accept input, if it can |
//!
//! Gameplay reads those resources and never asks which backend produced them,
//! so no system in `core` or `pet` is `cfg`-gated.
//!
//! Two backends are planned. The desktop one below covers macOS and Windows. A
//! Wayland one must bypass winit entirely, because always-on-top, surface
//! positioning and partial click-through are only reachable through
//! `zwlr_layer_shell_v1`, which winit does not implement.

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod desktop;
#[cfg(target_os = "linux")]
pub mod wayland;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use desktop::DesktopBackendPlugin as BackendPlugin;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use desktop::window_plugin;

#[cfg(target_os = "linux")]
pub use wayland::WaylandBackendPlugin as BackendPlugin;
#[cfg(target_os = "linux")]
pub use wayland::window_plugin;
