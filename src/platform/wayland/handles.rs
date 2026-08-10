//! Raw handles for handing our own `wl_surface` to `wgpu`.
//!
//! Stored as bare pointers rather than the live proxies so the type can be
//! `Copy` and cross into the render sub-app as a plain
//! [`bevy::render::extract_resource::ExtractResource`]; the proxies themselves
//! stay on the main-world side, owned by [`super::state::WaylandState`].

use std::ffi::c_void;
use std::ptr::NonNull;

use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use wayland_client::Proxy;
use wayland_client::protocol::{wl_display::WlDisplay, wl_surface::WlSurface};
use wgpu::rwh::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};

/// Everything `wgpu::Instance::create_surface_unsafe` needs, plus the pixel
/// size the surface was last configured at.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, PartialEq)]
pub struct WaylandSurfaceHandles {
    display_ptr: usize,
    window_ptr: usize,
    pub width: u32,
    pub height: u32,
}

impl WaylandSurfaceHandles {
    pub fn new(display: &WlDisplay, surface: &WlSurface, width: u32, height: u32) -> Self {
        Self {
            display_ptr: display.id().as_ptr() as usize,
            window_ptr: surface.id().as_ptr() as usize,
            width,
            height,
        }
    }

    pub fn raw_display_handle(&self) -> RawDisplayHandle {
        let handle = WaylandDisplayHandle::new(
            NonNull::new(self.display_ptr as *mut c_void).expect("display ptr should be valid"),
        );
        RawDisplayHandle::Wayland(handle)
    }

    pub fn raw_window_handle(&self) -> RawWindowHandle {
        let handle = WaylandWindowHandle::new(
            NonNull::new(self.window_ptr as *mut c_void).expect("surface ptr should be valid"),
        );
        RawWindowHandle::Wayland(handle)
    }
}
