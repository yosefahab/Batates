//! Reading the OS cursor while the overlay is click-through.
//!
//! The window sets `hit_test: false` so clicks reach the apps underneath, which
//! also means Bevy's own input never sees the pointer. Asking the OS directly is
//! what keeps click-to-summon working.
//!
//! Each platform module returns a [`PointerAt`] that names its own coordinate
//! space, so the conversion to surface space is forced to be explicit.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
use macos as backend;
#[cfg(target_os = "windows")]
use windows as backend;

use crate::core::input::{ButtonMask, PointerAt};

/// The cursor position, in whichever space this platform reports.
pub fn pointer_position() -> PointerAt {
    backend::pointer_position()
}

/// Which mouse buttons are currently held.
pub fn buttons() -> ButtonMask {
    backend::buttons()
}
