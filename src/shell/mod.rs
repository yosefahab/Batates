//! The parts of the app that are not the pet: how it is quit and controlled.

pub mod ipc;
pub mod shutdown;
pub mod tray;

use bevy::prelude::*;

use shutdown::AppShutdown;

/// Tray, signals, and the control socket.
pub struct ShellPlugin;

impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AppShutdown>()
            .insert_resource(shutdown::install_signal_handler())
            .add_systems(
                Update,
                (
                    shutdown::poll_signal,
                    tray::poll_tray,
                    ipc::poll_ipc,
                    // Runs last so a request raised this frame is acted on now
                    // rather than a frame later.
                    shutdown::handle_shutdown,
                )
                    .chain(),
            );

        // Both are created here rather than in a startup system: the tray
        // handle is not thread-safe and must be owned by the main thread, which
        // is where plugin construction runs.
        if let Some(tray) = tray::build_tray() {
            app.insert_non_send(tray);
        }

        if let Some(commands) = ipc::start_listener() {
            app.insert_resource(commands);
        }
    }
}
