#![windows_subsystem = "windows"]

mod camera;
mod config;
mod core;
mod pet;
mod platform;
mod shell;
mod skin;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use clap::Parser;

use camera::CameraPlugin;
use config::Config;
use pet::PetPlugin;
use platform::{BackendPlugin, CURSOR_OPTIONS, overlay_window};
use shell::ShellPlugin;

/// Exit code for a config the user must fix.
const EXIT_BAD_CONFIG: i32 = 2;

/// Exit code for a session that cannot host the overlay at all.
#[cfg(target_os = "linux")]
const EXIT_UNSUPPORTED_SESSION: i32 = 3;

/// A desktop pet.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    /// Ask a running instance to exit, then quit.
    #[arg(long)]
    quit: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.quit {
        match shell::ipc::request_quit() {
            Ok(true) => println!("batates: asked the running instance to quit"),
            Ok(false) => println!("batates: no instance is running"),
            Err(error) => {
                eprintln!("batates: could not reach a running instance: {error}");
                std::process::exit(1);
            }
        }
        return;
    }

    // Refuse early and legibly on a session that cannot host an overlay,
    // rather than failing somewhere inside the renderer.
    #[cfg(target_os = "linux")]
    {
        let check = platform::wayland::probe::check_session();
        if check != platform::wayland::probe::SessionCheck::Ok {
            eprintln!("{}", platform::wayland::probe::explain(&check));
            std::process::exit(EXIT_UNSUPPORTED_SESSION);
        }
    }

    // One overlay is enough, and several would fight over the same screen.
    if shell::ipc::instance_running() {
        eprintln!("batates: already running. Use `batates --quit` to stop it.");
        std::process::exit(1);
    }

    let config = load_config_or_exit();

    App::new()
        .insert_resource(ClearColor(Color::NONE))
        .insert_resource(config)
        .add_plugins(setup_plugins())
        .add_plugins((CameraPlugin, BackendPlugin, PetPlugin, ShellPlugin))
        .run();
}

/// Loads config before Bevy starts, so a bad file produces a readable message
/// rather than a panic inside a system.
///
/// A missing file is normal and yields defaults; a malformed one is the user's
/// typo and is reported with the parser's line and column.
fn load_config_or_exit() -> Config {
    let path = config::config_path();
    match config::load_config(&path) {
        Ok(Some(config)) => config,
        Ok(None) => Config::default(),
        Err(error) => {
            eprintln!("batates: {error}");
            eprintln!("\nFix the file or delete it to fall back to defaults.");
            std::process::exit(EXIT_BAD_CONFIG);
        }
    }
}

fn setup_plugins() -> PluginGroupBuilder {
    DefaultPlugins
        .set(ImagePlugin::default_nearest())
        .set(WindowPlugin {
            primary_window: Some(overlay_window()),
            // Split out of `Window` in Bevy 0.17.
            primary_cursor_options: Some(CURSOR_OPTIONS),
            ..default()
        })
        .set(AssetPlugin {
            mode: AssetMode::Unprocessed,
            ..default()
        })
}
