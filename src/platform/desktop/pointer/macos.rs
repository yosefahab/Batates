//! Global pointer reads on macOS.
//!
//! Declared directly rather than pulled from a crate: this is two calls, and
//! owning them means the signature can state which coordinate space they return
//! instead of leaving it to the call site to guess. Guessing is what produced
//! the DPI bug on Windows.
//!
//! `CGEventGetLocation` reports **points**, which are already logical pixels,
//! with the origin at the top-left of the main display and Y growing downward.

use bevy::prelude::*;
use std::ffi::c_void;

use crate::core::coords::ScreenLogical;
use crate::core::input::{ButtonMask, PointerAt};

#[repr(C)]
struct CGPoint {
    x: f64,
    y: f64,
}

type CGEventRef = *mut c_void;
type CGEventSourceRef = *mut c_void;

/// Combined session state: what the user is actually doing, including input
/// from other processes. Matches the constant `kCGEventSourceStateCombinedSessionState`.
const COMBINED_SESSION_STATE: i32 = 0;
const MOUSE_BUTTON_LEFT: u32 = 0;
const MOUSE_BUTTON_RIGHT: u32 = 1;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    /// A null source yields an event describing the current pointer state.
    fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// Reads the cursor position, in logical points.
pub fn pointer_position() -> PointerAt {
    // SAFETY: CGEventCreate(null) is documented to return an autoreleased event
    // describing current state, or null under memory pressure. The event is
    // released before returning, and the CGPoint is copied out by value.
    unsafe {
        let event = CGEventCreate(std::ptr::null_mut());
        if event.is_null() {
            return PointerAt::Absent;
        }
        let point = CGEventGetLocation(event);
        CFRelease(event as *const c_void);
        PointerAt::GlobalLogical(ScreenLogical(Vec2::new(point.x as f32, point.y as f32)))
    }
}

/// Reads which mouse buttons are held.
pub fn buttons() -> ButtonMask {
    let mut mask = ButtonMask::empty();
    // SAFETY: a pure query of global button state; no pointers involved.
    unsafe {
        if CGEventSourceButtonState(COMBINED_SESSION_STATE, MOUSE_BUTTON_LEFT) {
            mask |= ButtonMask::LEFT;
        }
        if CGEventSourceButtonState(COMBINED_SESSION_STATE, MOUSE_BUTTON_RIGHT) {
            mask |= ButtonMask::RIGHT;
        }
    }
    mask
}
