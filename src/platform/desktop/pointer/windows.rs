//! Global pointer reads on Windows.
//!
//! `GetCursorPos` reports **physical** pixels in virtual-screen coordinates,
//! unlike macOS which reports logical points. That asymmetry is the whole
//! reason [`ScreenPhysical`] and [`ScreenLogical`] are separate types: the old
//! code fed physical pixels into a logical-space conversion, so the pet landed
//! in the wrong place at any display scaling other than 100%.
//!
//! The values are only physical if the process is per-monitor DPI aware. winit
//! sets that during window creation, so this must not be called before the
//! window exists.

use bevy::prelude::*;

use crate::core::coords::ScreenPhysical;
use crate::core::input::{ButtonMask, PointerAt};

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

const VK_LBUTTON: i32 = 0x01;
const VK_RBUTTON: i32 = 0x02;
/// `GetAsyncKeyState` reports "currently down" in the high-order bit.
const KEY_DOWN_MASK: i16 = -0x8000;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetCursorPos(point: *mut Point) -> i32;
    fn GetAsyncKeyState(key: i32) -> i16;
}

/// Reads the cursor position, in physical pixels.
pub fn pointer_position() -> PointerAt {
    let mut point = Point { x: 0, y: 0 };
    // SAFETY: `point` is a valid, correctly sized, exclusively borrowed
    // allocation for the duration of the call.
    let ok = unsafe { GetCursorPos(&mut point) } != 0;
    if !ok {
        // Fails when the calling desktop is not the input desktop, e.g. while
        // the lock screen is up.
        return PointerAt::Absent;
    }
    PointerAt::Global(ScreenPhysical(IVec2::new(point.x, point.y)))
}

/// Reads which mouse buttons are held.
pub fn buttons() -> ButtonMask {
    let mut mask = ButtonMask::empty();
    // SAFETY: a pure query of global key state; no pointers involved.
    unsafe {
        if GetAsyncKeyState(VK_LBUTTON) & KEY_DOWN_MASK != 0 {
            mask |= ButtonMask::LEFT;
        }
        if GetAsyncKeyState(VK_RBUTTON) & KEY_DOWN_MASK != 0 {
            mask |= ButtonMask::RIGHT;
        }
    }
    mask
}
