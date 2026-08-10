//! One way out, however the request arrives.
//!
//! Every quit path writes [`AppShutdown`]; a single system turns that into
//! Bevy's `AppExit`. Nothing calls `std::process::exit` from inside the running
//! app, because backends need their `Drop` to run: the Wayland one has a layer
//! surface to tear down, and killing the process mid-frame is what made the old
//! build look like it crashed on quit.

use bevy::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A request to exit. Written by the tray, the hotkey, IPC, or a signal.
#[derive(Message, Debug, Clone, Copy)]
pub struct AppShutdown;

/// Set from the signal handler, which cannot touch the ECS.
#[derive(Resource, Clone)]
pub struct SignalFlag(Arc<AtomicBool>);

impl SignalFlag {
    pub fn raised(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Installs a handler for SIGINT, SIGTERM and SIGHUP.
///
/// Returns the flag even if installation fails: a missing signal handler makes
/// Ctrl-C less graceful, but it is not a reason to refuse to start.
pub fn install_signal_handler() -> SignalFlag {
    let flag = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&flag);

    if let Err(error) = ctrlc::set_handler(move || {
        handler_flag.store(true, Ordering::Relaxed);
    }) {
        warn!("could not install a signal handler: {error}");
    }

    SignalFlag(flag)
}

/// Turns a raised signal into a shutdown request.
pub fn poll_signal(flag: Res<SignalFlag>, mut shutdown: MessageWriter<AppShutdown>) {
    if flag.raised() {
        shutdown.write(AppShutdown);
    }
}

/// The single place the app decides to stop.
pub fn handle_shutdown(mut requests: MessageReader<AppShutdown>, mut exit: MessageWriter<AppExit>) {
    if !requests.is_empty() {
        requests.clear();
        info!("shutting down");
        exit.write(AppExit::Success);
    }
}
