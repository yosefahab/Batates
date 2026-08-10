//! Wayland connection state and protocol event handling.
//!
//! One struct implements `Dispatch` for every object we bind, because
//! `wayland-client` routes events by `(interface, state type)` rather than by
//! per-object closures. Everything here only records what happened; deciding
//! what it means (a `PointerSample`, a resized `ScreenGeometry`, ...) is left
//! to the systems in [`super`], which is where Bevy resources like [`Time`]
//! are actually available.

use bevy::math::Vec2;
use wayland_client::protocol::{
    wl_compositor, wl_output, wl_pointer, wl_registry, wl_seat, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

use crate::core::coords::MonitorGeometry;

const COMPOSITOR: &str = "wl_compositor";
const LAYER_SHELL: &str = "zwlr_layer_shell_v1";
const SEAT: &str = "wl_seat";
const OUTPUT: &str = "wl_output";

/// A pointer event queued for the next frame's [`crate::core::input::PointerSample`].
///
/// Kept as raw deltas rather than converted here because `PointerSample`
/// needs `Time::elapsed()`, which only a Bevy system can read.
#[derive(Debug, Clone, Copy)]
pub enum PointerEvent {
    Enter(Vec2),
    Motion(Vec2),
    Leave,
    Button { code: u32, pressed: bool },
}

/// In-progress `wl_output` geometry, assembled across several events and only
/// meaningful once `Done` arrives.
#[derive(Default)]
struct OutputDraft {
    position: (i32, i32),
    size: (u32, u32),
    scale: i32,
}

/// Everything the connection has told us, and the protocol objects we hold.
pub struct WaylandState {
    pub compositor: Option<wl_compositor::WlCompositor>,
    pub layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    pub seat: Option<wl_seat::WlSeat>,
    pub pointer: Option<wl_pointer::WlPointer>,
    pub output: Option<wl_output::WlOutput>,
    output_draft: OutputDraft,
    /// Finalized on the output's `Done` event. `None` beforehand: a v1
    /// backend supports exactly one output, so there is nothing to fall back
    /// to.
    pub monitor: Option<MonitorGeometry>,
    /// Set by the layer surface's `Configure` event; `(width, height)` in
    /// surface-local logical pixels.
    pub configured_size: Option<(u32, u32)>,
    /// The compositor asked us to close.
    pub closed: bool,
    pub pointer_events: Vec<PointerEvent>,
}

impl WaylandState {
    pub fn new() -> Self {
        Self {
            compositor: None,
            layer_shell: None,
            seat: None,
            pointer: None,
            output: None,
            output_draft: OutputDraft::default(),
            monitor: None,
            configured_size: None,
            closed: false,
            pointer_events: Vec::new(),
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            COMPOSITOR => state.compositor = Some(registry.bind(name, version.min(4), qh, ())),
            LAYER_SHELL => state.layer_shell = Some(registry.bind(name, version.min(4), qh, ())),
            SEAT => state.seat = Some(registry.bind(name, version.min(7), qh, ())),
            // A v1 backend supports a single output; later globals are ignored.
            OUTPUT if state.output.is_none() => {
                state.output = Some(registry.bind(name, version.min(3), qh, ()));
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Geometry { x, y, .. } => state.output_draft.position = (x, y),
            wl_output::Event::Mode { width, height, .. } => {
                state.output_draft.size = (width.max(0) as u32, height.max(0) as u32);
            }
            wl_output::Event::Scale { factor } => state.output_draft.scale = factor,
            wl_output::Event::Done => {
                state.monitor = Some(MonitorGeometry {
                    physical_position: bevy::math::IVec2::new(
                        state.output_draft.position.0,
                        state.output_draft.position.1,
                    ),
                    physical_size: bevy::math::UVec2::new(
                        state.output_draft.size.0,
                        state.output_draft.size.1,
                    ),
                    scale_factor: state.output_draft.scale.max(1) as f64,
                });
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for WaylandState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities { capabilities } = event else {
            return;
        };
        let has_pointer = capabilities
            .into_result()
            .map(|caps| caps.contains(wl_seat::Capability::Pointer))
            .unwrap_or(false);
        if has_pointer && state.pointer.is_none() {
            state.pointer = Some(seat.get_pointer(qh, ()));
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface_x,
                surface_y,
                ..
            } => state.pointer_events.push(PointerEvent::Enter(Vec2::new(
                surface_x as f32,
                surface_y as f32,
            ))),
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => state.pointer_events.push(PointerEvent::Motion(Vec2::new(
                surface_x as f32,
                surface_y as f32,
            ))),
            wl_pointer::Event::Leave { .. } => state.pointer_events.push(PointerEvent::Leave),
            wl_pointer::Event::Button {
                button,
                state: wayland_client::WEnum::Value(btn),
                ..
            } => {
                state.pointer_events.push(PointerEvent::Button {
                    code: button,
                    pressed: btn == wl_pointer::ButtonState::Pressed,
                });
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        layer_surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer_surface.ack_configure(serial);
                state.configured_size = Some((width, height));
            }
            zwlr_layer_surface_v1::Event::Closed => state.closed = true,
            _ => {}
        }
    }
}

delegate_noop!(WaylandState: ignore wl_compositor::WlCompositor);
delegate_noop!(WaylandState: ignore wl_surface::WlSurface);
delegate_noop!(WaylandState: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(WaylandState: ignore wayland_client::protocol::wl_region::WlRegion);
