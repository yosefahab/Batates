//! Presents Bevy's rendered frame onto our own `wl_surface`.
//!
//! Bevy's own window/swapchain plumbing (`bevy_winit`'s `WindowSurfaces`) is
//! built around a `Window` entity and is not something a hand-created surface
//! can plug into. Instead the main camera renders into an offscreen
//! [`Image`], and this module copies that image onto a `wgpu::Surface` we
//! built ourselves from the layer-shell surface's raw handle, once per frame,
//! in the render sub-app.
//!
//! The surface is created once and never reconfigured: monitor geometry
//! changing mid-run is out of scope for now (see the module doc on
//! [`super`]).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::renderer::{RenderAdapter, RenderDevice, RenderInstance, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use wgpu::{
    CommandEncoderDescriptor, CompositeAlphaMode, CurrentSurfaceTexture, Origin3d, PresentMode,
    SurfaceConfiguration, SurfaceTargetUnsafe, TexelCopyTextureInfo, TextureAspect,
};

use super::handles::WaylandSurfaceHandles;

/// The format the offscreen image and the real surface are both created
/// with, so presenting is a same-format copy rather than a conversion.
const SURFACE_FORMAT: TextureFormat = TextureFormat::Bgra8UnormSrgb;

/// The camera's render target: an image sized to the layer-shell surface,
/// copied onto the real surface every frame.
#[derive(Resource, ExtractResource, Clone)]
pub struct WaylandRenderTarget {
    pub image: Handle<Image>,
}

/// Builds the offscreen render target the main camera draws into.
pub fn create_render_target(images: &mut Assets<Image>, width: u32, height: u32) -> Handle<Image> {
    let mut image = Image::new_fill(
        Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        SURFACE_FORMAT,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC;
    images.add(image)
}

/// Registers the render-app systems that own and present the real surface.
///
/// Called once from [`super::WaylandBackendPlugin::build`], after the render
/// sub-app exists.
pub fn install(app: &mut App) {
    app.add_plugins(ExtractResourcePlugin::<WaylandSurfaceHandles>::default())
        .add_plugins(ExtractResourcePlugin::<WaylandRenderTarget>::default());

    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<WaylandGpuSurface>()
        .add_systems(
            Render,
            prepare_wayland_surface.in_set(RenderSystems::PrepareResources),
        )
        .add_systems(
            Render,
            present_wayland_surface.in_set(RenderSystems::Cleanup),
        );
}

/// The live `wgpu` surface. Lives only in the render sub-app: it wraps a raw
/// pointer into the main world's `wl_surface`, which is not `Send`-safe to
/// extract, so it is built directly from the extracted handles instead of
/// being extracted itself.
#[derive(Resource, Default)]
struct WaylandGpuSurface {
    surface: Option<wgpu::Surface<'static>>,
}

fn prepare_wayland_surface(
    handles: Option<Res<WaylandSurfaceHandles>>,
    mut state: ResMut<WaylandGpuSurface>,
    render_instance: Res<RenderInstance>,
    render_adapter: Res<RenderAdapter>,
    render_device: Res<RenderDevice>,
) {
    let Some(handles) = handles else { return };
    if state.surface.is_some() {
        return;
    }

    let instance = render_instance.0.as_ref();
    let surface = unsafe {
        instance
            .create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(handles.raw_display_handle()),
                raw_window_handle: handles.raw_window_handle(),
            })
            .expect("create the wgpu surface from the layer-shell wl_surface")
    };

    let capabilities = surface.get_capabilities(render_adapter.0.as_ref());
    let format = capabilities
        .formats
        .iter()
        .copied()
        .find(|format| *format == SURFACE_FORMAT)
        .unwrap_or(capabilities.formats[0]);

    // The risk this whole backend hinged on: a wallpaper-layer surface would
    // ask for `Opaque`, but an overlay needs to see through to the desktop
    // behind it. Confirmed available on Sway by `examples/wayland_transparency_probe.rs`.
    let alpha_mode = capabilities
        .alpha_modes
        .iter()
        .copied()
        .find(|mode| {
            matches!(
                mode,
                CompositeAlphaMode::PreMultiplied | CompositeAlphaMode::PostMultiplied
            )
        })
        .unwrap_or_else(|| {
            warn!(
                "compositor only offers {:?} alpha modes; the overlay may render opaque",
                capabilities.alpha_modes
            );
            capabilities.alpha_modes[0]
        });

    let present_mode = capabilities
        .present_modes
        .iter()
        .copied()
        .find(|mode| matches!(mode, PresentMode::Fifo))
        .unwrap_or(capabilities.present_modes[0]);

    render_device.configure_surface(
        &surface,
        &SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_DST,
            format,
            width: handles.width.max(1),
            height: handles.height.max(1),
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        },
    );

    state.surface = Some(surface);
}

fn present_wayland_surface(
    state: Res<WaylandGpuSurface>,
    target: Option<Res<WaylandRenderTarget>>,
    images: Res<RenderAssets<GpuImage>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    let Some(surface) = state.surface.as_ref() else {
        return;
    };
    let Some(target) = target else { return };
    let Some(gpu_image) = images.get(&target.image) else {
        return;
    };

    let surface_texture = match surface.get_current_texture() {
        CurrentSurfaceTexture::Success(texture) | CurrentSurfaceTexture::Suboptimal(texture) => {
            texture
        }
        other => {
            debug!("Wayland surface frame not ready: {other:?}");
            return;
        }
    };

    let mut encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("wayland-surface-present"),
    });

    let extent = Extent3d {
        width: gpu_image
            .texture_descriptor
            .size
            .width
            .min(surface_texture.texture.width()),
        height: gpu_image
            .texture_descriptor
            .size
            .height
            .min(surface_texture.texture.height()),
        depth_or_array_layers: 1,
    };

    encoder.copy_texture_to_texture(
        gpu_image.texture.as_image_copy(),
        TexelCopyTextureInfo {
            texture: &surface_texture.texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        extent,
    );

    render_queue.submit(Some(encoder.finish()));
    surface_texture.present();
}
