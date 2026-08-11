//! Standalone risk test: does a transparent `zwlr_layer_shell_v1` surface on
//! the Overlay layer actually composite as transparent, or does it go opaque
//! like borderless fullscreen did on macOS (see `platform/wayland/mod.rs`)?
//!
//! This is deliberately not part of the app. It owns its own Wayland
//! connection and wgpu instance so the real backend is not built on an
//! unverified assumption.
//!
//! Run with `cargo run --example wayland_transparency_probe`, then while it is
//! running take a screenshot (e.g. `grim shot.png`) and check by eye whether
//! the desktop behind the red is visible.
//!
//! Linux-only: `wayland-client` and `wgpu` are only dependencies on that
//! target. The real work lives in [`linux`] so the rest of the crate's
//! `cargo clippy --all-targets` stays buildable elsewhere; a `cfg` on the
//! whole file would leave it with no `main` to link on other platforms.

#[cfg(target_os = "linux")]
fn main() {
    linux::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("wayland_transparency_probe only runs on Linux (needs wayland-client and wgpu)");
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::time::{Duration, Instant};

    use wayland_client::protocol::{wl_compositor, wl_registry, wl_surface};
    use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
    use wayland_protocols_wlr::layer_shell::v1::client::{
        zwlr_layer_shell_v1, zwlr_layer_surface_v1,
    };
    use wgpu::rwh::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};

    const LAYER_SHELL: &str = "zwlr_layer_shell_v1";
    const COMPOSITOR: &str = "wl_compositor";

    /// How long the surface stays up, so there is time to screenshot it by hand.
    const RUN_FOR: Duration = Duration::from_secs(20);

    struct App {
        compositor: Option<wl_compositor::WlCompositor>,
        layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
        /// Set once the compositor has told us the surface's size.
        configured: Option<(u32, u32)>,
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for App {
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
                COMPOSITOR => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()));
                }
                LAYER_SHELL => {
                    state.layer_shell = Some(registry.bind(name, version.min(4), qh, ()));
                }
                _ => {}
            }
        }
    }

    impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for App {
        fn event(
            state: &mut Self,
            layer_surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
            event: zwlr_layer_surface_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } = event
            {
                layer_surface.ack_configure(serial);
                state.configured = Some((width, height));
            }
        }
    }

    delegate_noop!(App: ignore wl_compositor::WlCompositor);
    delegate_noop!(App: ignore wl_surface::WlSurface);
    delegate_noop!(App: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);

    /// Builds `raw-window-handle` handles from live Wayland proxies.
    ///
    /// Bevy's real renderer will need the same conversion once the backend hands
    /// its surface to `wgpu`; the pointers are stable identity, not owned memory.
    fn raw_handles(
        connection: &Connection,
        surface: &wl_surface::WlSurface,
    ) -> (RawDisplayHandle, RawWindowHandle) {
        let display_ptr = connection.display().id().as_ptr() as *mut c_void;
        let window_ptr = surface.id().as_ptr() as *mut c_void;
        let display = WaylandDisplayHandle::new(NonNull::new(display_ptr).expect("display ptr"));
        let window = WaylandWindowHandle::new(NonNull::new(window_ptr).expect("surface ptr"));
        (
            RawDisplayHandle::Wayland(display),
            RawWindowHandle::Wayland(window),
        )
    }

    pub fn main() {
        let connection = Connection::connect_to_env().expect("connect to the Wayland compositor");
        let mut queue = connection.new_event_queue();
        let qh = queue.handle();
        connection.display().get_registry(&qh, ());

        let mut app = App {
            compositor: None,
            layer_shell: None,
            configured: None,
        };
        queue.roundtrip(&mut app).expect("initial roundtrip");

        let compositor = app
            .compositor
            .clone()
            .expect("compositor did not advertise wl_compositor");
        let layer_shell = app.layer_shell.clone().expect(
            "compositor did not advertise zwlr_layer_shell_v1; see platform/wayland/probe.rs",
        );

        let surface = compositor.create_surface(&qh, ());
        let layer_surface = layer_shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Overlay,
            "batates-transparency-probe".to_string(),
            &qh,
            (),
        );
        layer_surface.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right,
        );
        layer_surface.set_exclusive_zone(-1);
        layer_surface
            .set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        surface.commit();

        println!("waiting for the compositor to configure the surface...");
        while app.configured.is_none() {
            queue.blocking_dispatch(&mut app).expect("dispatch");
        }
        let (width, height) = app.configured.expect("just checked");
        println!("configured at {width}x{height}");

        let instance = wgpu::Instance::default();
        let (raw_display, raw_window) = raw_handles(&connection, &surface);
        let gpu_surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(raw_display),
                    raw_window_handle: raw_window,
                })
                .expect("create the wgpu surface from the layer-shell surface")
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&gpu_surface),
            ..Default::default()
        }))
        .expect("find a GPU adapter compatible with this Wayland surface");

        let (device, queue_gpu) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("transparency-probe"),
                ..Default::default()
            }))
            .expect("request a device");

        let capabilities = gpu_surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(capabilities.formats[0]);

        // The whole point of this probe: request a genuinely transparent alpha
        // mode rather than `Opaque`, which is what a wallpaper-layer surface
        // would ask for. If the compositor only offers `Opaque` here, an overlay
        // is not viable on this compositor without further work.
        let alpha_mode = capabilities
            .alpha_modes
            .iter()
            .copied()
            .find(|mode| {
                matches!(
                    mode,
                    wgpu::CompositeAlphaMode::PreMultiplied
                        | wgpu::CompositeAlphaMode::PostMultiplied
                )
            })
            .unwrap_or_else(|| {
                eprintln!(
                    "warning: compositor only offers {:?}; a transparent overlay may not be possible",
                    capabilities.alpha_modes
                );
                capabilities.alpha_modes[0]
            });
        println!("using format {format:?}, alpha mode {alpha_mode:?}");

        gpu_surface.configure(
            &device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: width.max(1),
                height: height.max(1),
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        );

        // Half-transparent red, premultiplied so it is correct under either
        // `PreMultiplied` or `PostMultiplied` (which ignores the distinction for a
        // flat clear anyway).
        let clear_color = wgpu::Color {
            r: 0.5,
            g: 0.0,
            b: 0.0,
            a: 0.5,
        };

        println!("surface is up; take a screenshot now (e.g. `grim shot.png`)");
        let start = Instant::now();
        while start.elapsed() < RUN_FOR {
            let _ = queue.dispatch_pending(&mut app);

            let frame = match gpu_surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                other => {
                    eprintln!("dropped a frame: {other:?}");
                    continue;
                }
            };
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(clear_color),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            queue_gpu.submit(Some(encoder.finish()));
            frame.present();

            std::thread::sleep(Duration::from_millis(16));
        }

        println!("done; tearing down the surface");
        layer_surface.destroy();
        surface.destroy();
    }
}
