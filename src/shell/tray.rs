//! The tray icon and its menu.
//!
//! This is the app's only visible affordance: the overlay is click-through and
//! undecorated, so without it there is no way to quit but a signal.
//!
//! Menu events arrive on a global channel from the platform's own event loop
//! rather than through Bevy, so they are polled once per frame.

use bevy::prelude::*;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::pet::{DespawnPet, Pet, SpawnPet};
use crate::shell::shutdown::AppShutdown;

/// The tray icon, kept alive for the process's lifetime.
///
/// Dropping it removes the icon, so this must be stored even though nothing
/// reads it. Held as a non-send resource because the platform handle is not
/// thread-safe and must stay on the thread that created it.
pub struct Tray {
    _icon: TrayIcon,
    add_pet: String,
    remove_pet: String,
    quit: String,
}

/// Builds the tray icon.
///
/// Errors are reported rather than fatal: a missing tray is a degraded
/// experience, not a broken app, and on Linux it depends on the user running a
/// StatusNotifierItem host at all.
pub fn build_tray() -> Option<Tray> {
    // The Linux backend is built on gtk, but neither initializes it nor runs
    // its event loop; both are this app's responsibility. Without this, gtk
    // panics as soon as the menu is built.
    #[cfg(target_os = "linux")]
    if let Err(error) = gtk::init() {
        warn!("could not initialize gtk, so no tray icon will be shown: {error}");
        return None;
    }

    let add_pet = MenuItem::new("Add pet", true, None);
    let remove_pet = MenuItem::new("Remove pet", true, None);
    let quit = MenuItem::new("Quit Batates", true, None);

    let menu = Menu::new();
    let items: [&dyn tray_icon::menu::IsMenuItem; 4] = [
        &add_pet,
        &remove_pet,
        &PredefinedMenuItem::separator(),
        &quit,
    ];
    if let Err(error) = menu.append_items(&items) {
        warn!("could not build the tray menu: {error}");
        return None;
    }

    let ids = (
        add_pet.id().0.clone(),
        remove_pet.id().0.clone(),
        quit.id().0.clone(),
    );

    match TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Batates")
        .with_icon(tray_icon_image())
        .build()
    {
        Ok(icon) => Some(Tray {
            _icon: icon,
            add_pet: ids.0,
            remove_pet: ids.1,
            quit: ids.2,
        }),
        Err(error) => {
            warn!("could not create the tray icon: {error}; use --quit to exit");
            None
        }
    }
}

/// The tray image, decoded from the icon that ships with the app.
fn tray_icon_image() -> Icon {
    const ICON_PNG: &[u8] = include_bytes!("../../assets/icon.png");

    let fallback = || Icon::from_rgba(vec![0; 4], 1, 1).expect("a 1x1 icon is valid");

    let Ok(image) = image_from_png(ICON_PNG) else {
        warn!("the bundled tray icon could not be decoded");
        return fallback();
    };
    let (rgba, width, height) = image;
    Icon::from_rgba(rgba, width, height).unwrap_or_else(|error| {
        warn!("the bundled tray icon is not valid RGBA: {error}");
        fallback()
    })
}

/// Decodes a PNG into raw RGBA using the decoder Bevy already depends on.
fn image_from_png(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), ()> {
    use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};

    let image = bevy::image::Image::from_buffer(
        bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        false,
        ImageSampler::nearest(),
        bevy::asset::RenderAssetUsages::MAIN_WORLD,
    )
    .map_err(|_| ())?;

    let size = image.size();
    let data = image.data.ok_or(())?;
    Ok((data, size.x, size.y))
}

/// Polls the tray's menu channel and turns clicks into app messages.
pub fn poll_tray(
    tray: Option<NonSend<Tray>>,
    pets: Query<Entity, With<Pet>>,
    mut spawns: MessageWriter<SpawnPet>,
    mut despawns: MessageWriter<DespawnPet>,
    mut shutdown: MessageWriter<AppShutdown>,
) {
    let Some(tray) = tray else { return };

    // Bevy's event loop is winit's, not gtk's, so gtk's own loop must be
    // pumped manually or the tray menu never redraws or dispatches clicks.
    #[cfg(target_os = "linux")]
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }

    while let Ok(event) = MenuEvent::receiver().try_recv() {
        let id = &event.id().0;
        if id == &tray.quit {
            shutdown.write(AppShutdown);
        } else if id == &tray.add_pet {
            spawns.write(SpawnPet { at: Vec2::ZERO });
        } else if id == &tray.remove_pet {
            // Removing the last pet would leave nothing to interact with, so
            // keep one alive.
            if pets.iter().count() > 1
                && let Some(pet) = pets.iter().next()
            {
                despawns.write(DespawnPet { pet });
            }
        }
    }
}
