//! Single instance, and `batates --quit`.
//!
//! A local socket serves both purposes: if connecting succeeds, an instance is
//! already running, which is what makes a second launch refuse to start and
//! what lets `--quit` ask the first one to exit.
//!
//! This is the quit path that always works. The tray needs a StatusNotifierItem
//! host, which not every Linux session runs, and a global hotkey has no Wayland
//! equivalent at all; a socket has neither problem, so it is what the
//! documentation points people at for binding a key in their compositor.

use bevy::prelude::*;
use interprocess::local_socket::traits::Stream;
use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions, Stream as LocalStream, ToNsName, prelude::*,
};
use std::io::{Read, Write};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use crate::shell::shutdown::AppShutdown;

/// What a client may ask of a running instance.
const QUIT_COMMAND: &[u8] = b"quit\n";

/// The socket name. Namespaced rather than a filesystem path so the same code
/// works against a Windows named pipe.
fn socket_name() -> std::io::Result<interprocess::local_socket::Name<'static>> {
    "batates.sock".to_ns_name::<GenericNamespaced>()
}

/// Asks a running instance to quit.
///
/// `Ok(false)` means nothing was listening, which is not an error: asking a
/// stopped app to stop has already succeeded.
pub fn request_quit() -> std::io::Result<bool> {
    let name = socket_name()?;
    match LocalStream::connect(name) {
        Ok(mut stream) => {
            stream.write_all(QUIT_COMMAND)?;
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

/// Whether another instance is already running.
pub fn instance_running() -> bool {
    socket_name()
        .and_then(LocalStream::connect)
        .map(|_| true)
        .unwrap_or(false)
}

/// Receives commands from the listener thread.
///
/// The receiver is `Send` but not `Sync`, so it needs a mutex to live in a
/// resource. There is exactly one reader, so the lock is never contended.
#[derive(Resource)]
pub struct IpcCommands(Mutex<Receiver<()>>);

/// Starts listening for `--quit` requests.
///
/// The listener blocks, so it lives on its own thread and reports through a
/// channel the app polls. A failure here costs the `--quit` path but nothing
/// else, so it warns rather than aborting startup.
pub fn start_listener() -> Option<IpcCommands> {
    let name = match socket_name() {
        Ok(name) => name,
        Err(error) => {
            warn!("could not derive the control socket name: {error}");
            return None;
        }
    };

    // Reclaim a stale socket. A process killed with SIGKILL leaves its socket
    // file behind, and without this every later launch loses `--quit`.
    //
    // This cannot displace a live instance: `main` refuses to start when one
    // answers on this socket, so reaching an AddrInUse error here means the
    // file has no listener behind it.
    let listener = match ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .create_sync()
    {
        Ok(listener) => listener,
        Err(error) => {
            warn!("could not listen on the control socket: {error}; --quit will not work");
            return None;
        }
    };

    let (sender, receiver) = channel();
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            let Ok(mut stream) = connection else { continue };
            let mut request = String::new();
            if stream.read_to_string(&mut request).is_ok()
                && request.trim() == "quit"
                && sender.send(()).is_err()
            {
                // The app is gone; nothing left to serve.
                break;
            }
        }
    });

    Some(IpcCommands(Mutex::new(receiver)))
}

/// Turns a received command into a shutdown request.
pub fn poll_ipc(commands: Option<Res<IpcCommands>>, mut shutdown: MessageWriter<AppShutdown>) {
    let Some(commands) = commands else { return };
    let Ok(receiver) = commands.0.lock() else {
        return;
    };
    loop {
        match receiver.try_recv() {
            Ok(()) => {
                shutdown.write(AppShutdown);
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => return,
        }
    }
}
