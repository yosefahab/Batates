//! The Linux backend.
//!
//! # Why this cannot reuse the desktop backend
//!
//! The macOS and Windows backend is a winit window that happens to be
//! fullscreen, transparent and always-on-top. On Wayland none of those three
//! are properties a client may ask for: xdg-shell has no always-on-top, no way
//! to position a surface, and no way to observe the pointer outside your own
//! surface. They are only reachable through `zwlr_layer_shell_v1`, which winit
//! does not implement (open PRs since 2024). So this backend owns its surface
//! rather than letting winit create one, running Bevy with `primary_window:
//! None` and copying its rendered frame onto that surface itself (see
//! [`render`] for why that is a copy rather than a direct handle-off).
//!
//! # Status
//!
//! A single output, single layer-shell surface is created, sized once at
//! startup and never reconfigured: monitor hot-plug or resize mid-run is not
//! handled. Multi-monitor spanning is not handled either — [`ScreenGeometry`]
//! reports exactly one monitor, the one the surface was created on. Both are
//! follow-up work, not silent gaps: a future backend would add per-output
//! surfaces the same way `bevy_live_wallpaper` does for its Background layer.
//!
//! Render everything into the one surface. Do not use `wl_subsurface` per pet:
//! that is what breaks `wl_shimeji` on Hyprland, which violates subsurface
//! clipping, and on Gamescope, which has no `wl_subcompositor`.

mod handles;
pub mod probe;
mod render;
mod state;

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use wayland_client::protocol::wl_surface;
use wayland_client::{Connection, EventQueue};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

use crate::core::PetSystems;
use crate::core::coords::{
    MonitorGeometry, ScreenGeometry, ScreenLogical, SurfaceLogical, SurfaceOrigin,
};
use crate::core::hitbox::DesiredInputRegion;
use crate::core::input::{ButtonMask, PointerAt, PointerSample};
use crate::shell::shutdown::AppShutdown;
use handles::WaylandSurfaceHandles;
use state::{PointerEvent, WaylandState};

/// Linux evdev button codes, as `wl_pointer.button` reports them.
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

/// How this backend wants its window created: it does not.
///
/// The overlay is a layer-shell surface this backend owns, so winit must not
/// create a window at all. Everything else in the app is unaffected, because
/// the surface is published through `SurfaceOrigin` either way.
///
/// `exit_condition` must not be the default `OnAllClosed`: with no `Window`
/// entity ever existing, that would exit on the very first frame. Quitting
/// stays reachable through the one path everything else already uses — see
/// [`crate::shell::shutdown`].
pub fn window_plugin() -> bevy::window::WindowPlugin {
    bevy::window::WindowPlugin {
        primary_window: None,
        exit_condition: bevy::window::ExitCondition::DontExit,
        ..default()
    }
}

/// The connection and the objects it handed us, bundled so accessing one
/// borrows all of them together rather than fighting over separate
/// `NonSend` slots.
///
/// Not `Send`: `wayland-client`'s queue is thread-affine, and this must stay
/// on the thread it was created on, same as [`crate::shell::tray::Tray`].
struct WaylandConnection {
    // Kept alive alongside the queue: nothing reads it after setup, but
    // dropping it would be free to tear down the socket the queue still
    // needs.
    connection: Connection,
    queue: EventQueue<WaylandState>,
    state: WaylandState,
    surface: wl_surface::WlSurface,
    layer_surface: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
}

/// The pointer's last known state, rebuilt from queued `wl_pointer` events
/// once per frame. A resource rather than inline in the event struct because
/// `wl_pointer.motion` reports absolute position, so only the latest sample
/// matters — unlike buttons, which must not be dropped between polls.
#[derive(Resource, Default)]
struct WaylandPointerState {
    position: Option<Vec2>,
    buttons: ButtonMask,
}

/// Installs the Wayland backend.
pub struct WaylandBackendPlugin;

impl Plugin for WaylandBackendPlugin {
    fn build(&self, app: &mut App) {
        let connection = connect_and_create_surface();
        let (width, height) = connection
            .state
            .configured_size
            .expect("the compositor must configure the layer surface before this returns");

        let monitor = connection.state.monitor.unwrap_or(MonitorGeometry {
            physical_position: IVec2::ZERO,
            physical_size: UVec2::new(width, height),
            scale_factor: 1.0,
        });

        app.insert_resource(ScreenGeometry {
            monitors: vec![monitor],
            primary: 0,
        })
        .insert_resource(SurfaceOrigin {
            origin: ScreenLogical(Vec2::ZERO),
            size: Vec2::new(width as f32, height as f32),
        })
        .init_resource::<WaylandPointerState>();

        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let image = render::create_render_target(&mut images, width, height);
        app.insert_resource(render::WaylandRenderTarget {
            image: image.clone(),
        });
        app.insert_resource(WaylandSurfaceHandles::new(
            &connection.connection.display(),
            &connection.surface,
            width,
            height,
        ));

        render::install(app);

        app.insert_non_send(connection);

        app.add_systems(
            Startup,
            assign_camera_target.after(crate::camera::spawn_camera),
        )
        .add_systems(Update, pump_wayland_events.before(PetSystems::Sample))
        .add_systems(Update, sample_pointer.in_set(PetSystems::Sample))
        .add_systems(
            PostUpdate,
            apply_input_region.after(crate::pet::compute_input_region),
        );
    }
}

/// Connects, binds the globals we need, and blocks until the compositor
/// configures our layer surface with a size.
///
/// Blocking here — rather than deferring to a system — mirrors how
/// [`crate::shell::tray::build_tray`] runs at plugin-build time: the surface
/// must exist before `Startup` systems run, since they read its size.
fn connect_and_create_surface() -> WaylandConnection {
    let connection = Connection::connect_to_env()
        .expect("a Wayland session, already confirmed present by the startup probe");
    let mut queue = connection.new_event_queue::<WaylandState>();
    let qh = queue.handle();
    let mut state = WaylandState::new();

    connection.display().get_registry(&qh, ());
    // First roundtrip: registry globals arrive and get bound.
    queue.roundtrip(&mut state).expect("registry roundtrip");
    // Second roundtrip: the bound output/seat send their own events, and any
    // requests those made (like `wl_seat.get_pointer`) are flushed.
    queue
        .roundtrip(&mut state)
        .expect("output and seat roundtrip");

    let compositor = state
        .compositor
        .clone()
        .expect("wl_compositor, a core global");
    let layer_shell = state
        .layer_shell
        .clone()
        .expect("zwlr_layer_shell_v1, already confirmed present by the startup probe");

    let surface = compositor.create_surface(&qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        state.output.as_ref(),
        zwlr_layer_shell_v1::Layer::Overlay,
        "batates".to_string(),
        &qh,
        (),
    );
    layer_surface.set_anchor(
        zwlr_layer_surface_v1::Anchor::Top
            | zwlr_layer_surface_v1::Anchor::Bottom
            | zwlr_layer_surface_v1::Anchor::Left
            | zwlr_layer_surface_v1::Anchor::Right,
    );
    // Reserves no space from other surfaces, and never takes keyboard focus:
    // this is a pet, not a panel.
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
    // (0, 0) asks the compositor to size us to the output; it answers with
    // `Configure`.
    layer_surface.set_size(0, 0);
    surface.commit();

    while state.configured_size.is_none() {
        queue
            .blocking_dispatch(&mut state)
            .expect("the compositor to configure the layer surface");
    }

    WaylandConnection {
        connection,
        queue,
        state,
        surface,
        layer_surface,
    }
}

/// Assigns the offscreen render target to the main camera.
///
/// Ordered after [`crate::camera::spawn_camera`] because the camera does not
/// exist yet when this backend's plugin builds — only once `Startup` runs.
fn assign_camera_target(
    mut commands: Commands,
    target: Res<render::WaylandRenderTarget>,
    cameras: Query<Entity, With<crate::camera::PrimaryCamera>>,
) {
    for camera in &cameras {
        commands
            .entity(camera)
            .insert(RenderTarget::Image(target.image.clone().into()));
    }
}

/// Drains queued Wayland events into the frame's pointer state, and turns a
/// compositor-requested close into the app's one shutdown path.
fn pump_wayland_events(
    mut connection: NonSendMut<WaylandConnection>,
    mut pointer: ResMut<WaylandPointerState>,
    mut shutdown: MessageWriter<AppShutdown>,
) {
    let connection = &mut *connection;
    if let Err(error) = connection.queue.dispatch_pending(&mut connection.state) {
        warn!("Wayland connection error: {error}");
        shutdown.write(AppShutdown);
        return;
    }

    for event in connection.state.pointer_events.drain(..) {
        match event {
            PointerEvent::Enter(p) | PointerEvent::Motion(p) => pointer.position = Some(p),
            PointerEvent::Leave => pointer.position = None,
            PointerEvent::Button { code, pressed } => {
                let button = match code {
                    BTN_LEFT => Some(ButtonMask::LEFT),
                    BTN_RIGHT => Some(ButtonMask::RIGHT),
                    BTN_MIDDLE => Some(ButtonMask::MIDDLE),
                    _ => None,
                };
                if let Some(button) = button {
                    pointer.buttons.set(button, pressed);
                }
            }
        }
    }

    if connection.state.closed {
        shutdown.write(AppShutdown);
    }
}

/// Publishes the pointer state gathered this frame, once, in surface space —
/// already the space `wl_pointer` reports in, so no conversion is needed.
fn sample_pointer(
    time: Res<Time>,
    pointer: Res<WaylandPointerState>,
    mut samples: MessageWriter<PointerSample>,
) {
    let at = match pointer.position {
        Some(p) => PointerAt::Surface(SurfaceLogical(p)),
        None => PointerAt::Absent,
    };
    samples.write(PointerSample {
        at,
        buttons: pointer.buttons,
        at_time: time.elapsed(),
    });
}

/// Applies the input region [`crate::pet::compute_input_region`] computed
/// this frame, gated on it having actually changed: this is the piece winit
/// cannot express, and the reason that resource exists at all.
///
/// Sets the region but does not commit: surface state is double-buffered, so
/// it takes effect on the next commit regardless of who issues it, and the
/// render sub-app already commits every frame when it presents. Committing
/// here too raced that per-frame commit for the same surface, which is what
/// produced `Protocol error 3 on wp_linux_drm_syncobj_surface_v1` under the
/// compositor's explicit-sync path.
fn apply_input_region(
    region: Res<DesiredInputRegion>,
    mut connection: NonSendMut<WaylandConnection>,
) {
    if !region.is_changed() {
        return;
    }

    let connection = &mut *connection;
    let Some(compositor) = connection.state.compositor.clone() else {
        return;
    };
    let qh = connection.queue.handle();
    let wl_region = compositor.create_region(&qh, ());
    for rect in &region.rects {
        wl_region.add(
            rect.min.x,
            rect.min.y,
            rect.max.x - rect.min.x,
            rect.max.y - rect.min.y,
        );
    }
    connection.surface.set_input_region(Some(&wl_region));
    wl_region.destroy();
}

impl Drop for WaylandConnection {
    fn drop(&mut self) {
        self.layer_surface.destroy();
        self.surface.destroy();
    }
}
