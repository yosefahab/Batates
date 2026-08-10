//! The overlay window, and the monitor geometry behind it.

use bevy::prelude::*;
use bevy::window::{
    CursorOptions, Monitor, MonitorSelection, PrimaryMonitor, PrimaryWindow, Window, WindowLevel,
    WindowMode, WindowPosition, WindowResolution,
};
// Only the macOS and Linux compositors need an explicit alpha mode.
#[cfg(any(target_os = "macos", target_os = "linux"))]
use bevy::window::CompositeAlphaMode;

use crate::core::coords::{MonitorGeometry, ScreenGeometry, ScreenLogical, SurfaceOrigin};

/// Click-through for the window's whole life: the overlay covers the screen, so
/// it must never intercept input meant for the apps underneath.
pub const CURSOR_OPTIONS: CursorOptions = CursorOptions {
    hit_test: false,
    visible: true,
    grab_mode: bevy::window::CursorGrabMode::None,
};

/// The overlay window.
///
/// Deliberately undecorated: a title bar would show above the desktop, and it
/// would also inset the client area from the window frame. Bevy reports only
/// the outer position, so that inset is unmeasurable and every cursor reading
/// would inherit it as error.
///
/// The size is a placeholder; [`track_monitors`] resizes it to the primary
/// display once winit reports one, because monitors are not known when the
/// window is described.
pub fn overlay_window() -> Window {
    Window {
        title: String::from("Batates"),
        transparent: true,
        has_shadow: false,
        decorations: false,
        resizable: false,
        skip_taskbar: true,
        window_level: WindowLevel::AlwaysOnTop,
        // Windowed, not fullscreen: macOS composites a borderless-fullscreen
        // window against an opaque backdrop, which destroys the transparency
        // the whole overlay depends on. The surface origin is read from winit
        // instead, so nothing here needs to guarantee a particular position.
        mode: WindowMode::Windowed,
        position: WindowPosition::Centered(MonitorSelection::Primary),
        resolution: WindowResolution::new(1280, 720),
        #[cfg(target_os = "macos")]
        composite_alpha_mode: CompositeAlphaMode::PostMultiplied,
        #[cfg(target_os = "linux")]
        composite_alpha_mode: CompositeAlphaMode::PreMultiplied,
        ..default()
    }
}

/// Mirrors winit's monitor list into [`ScreenGeometry`], and sizes the overlay
/// to the primary display.
///
/// Monitors arrive over time rather than at startup, so this watches for
/// additions and removals instead of running once. It replaces the `resolution`
/// crate, which reported only the primary display's size and nothing about
/// scale factors or layout.
pub fn track_monitors(
    monitors: Query<(&Monitor, Option<&PrimaryMonitor>)>,
    added: Query<(), Added<Monitor>>,
    mut removed: RemovedComponents<Monitor>,
    mut geometry: ResMut<ScreenGeometry>,
    window: Option<Single<&mut Window, With<PrimaryWindow>>>,
) {
    let changed = !added.is_empty() || !removed.read().collect::<Vec<_>>().is_empty();
    if !changed {
        return;
    }

    let mut list = Vec::new();
    let mut primary = 0;
    for (index, (monitor, is_primary)) in monitors.iter().enumerate() {
        if is_primary.is_some() {
            primary = index;
        }
        list.push(MonitorGeometry {
            physical_position: monitor.physical_position,
            physical_size: monitor.physical_size(),
            scale_factor: monitor.scale_factor,
        });
    }

    if list.is_empty() {
        return;
    }

    geometry.monitors = list;
    geometry.primary = primary;

    // Size and place the overlay over the primary display. Writing the position
    // is safe because nothing reads it back: the OS may clamp the request (macOS
    // will not put a plain window under the menu bar) and `track_surface` reads
    // the clamped truth from winit. Left to itself macOS cascades the window to
    // an arbitrary offset instead.
    let Some(mut window) = window else { return };
    let target = geometry.monitors[geometry.primary];
    window
        .resolution
        .set_physical_resolution(target.physical_size.x, target.physical_size.y);
    window.position = WindowPosition::At(target.physical_position);

    info!(
        "monitors: {} (primary {}x{} @{}x)",
        geometry.monitors.len(),
        target.physical_size.x,
        target.physical_size.y,
        target.scale_factor
    );
}

/// Publishes where the overlay sits, in logical pixels.
///
/// The origin is read from winit rather than from `Window::position`, because
/// that field reports what was *requested*: Bevy never resolves `Centered` into
/// a concrete position, and a written position is a request macOS may refuse
/// (it will not put a plain window under the menu bar). Either way the value
/// disagrees with where the window really is, and since every cursor reading is
/// measured against this origin, the error lands directly on the pet's hitbox.
///
/// `inner_position` is the client area, which is what we render into.
pub fn track_surface(
    window: Option<Single<(Entity, &Window), With<PrimaryWindow>>>,
    mut commands: Commands,
    // WINIT_WINDOWS is a thread-local that is only populated on the main
    // thread; without this the lookup silently finds nothing on a worker.
    _non_send_marker: bevy::ecs::system::NonSendMarker,
) {
    let Some(window) = window else { return };
    let (entity, window) = *window;

    let scale = window.resolution.scale_factor();
    let origin = bevy::winit::WINIT_WINDOWS.with_borrow(|winit_windows| {
        winit_windows
            .get_window(entity)
            .and_then(|w| w.inner_position().ok())
            .map(|p| Vec2::new(p.x as f32, p.y as f32) / scale)
    });

    // Before the window exists there is nothing to publish; guessing an origin
    // here is what produced a silently offset hitbox.
    let Some(origin) = origin else { return };

    commands.insert_resource(SurfaceOrigin {
        origin: ScreenLogical(origin),
        size: window.resolution.size(),
    });
}
