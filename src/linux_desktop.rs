//! Prefer X11/XWayland for working drag-and-drop and close-to-tray.
//!
//! In winit 0.30, native Wayland cannot hide a window, and compositor frame
//! throttling can defer app updates (including tray actions) while minimized.
use std::ffi::OsStr;

fn has_x11_display(display: Option<&OsStr>) -> bool {
    display.is_some_and(|value| !value.is_empty())
}

pub fn configure(options: eframe::NativeOptions) -> eframe::NativeOptions {
    if !has_x11_display(std::env::var_os("DISPLAY").as_deref()) || !has_x11_keyboard_library() {
        return options;
    }
    use winit::platform::x11::EventLoopBuilderExtX11 as _;
    let mut options = options;
    options.event_loop_builder = Some(Box::new(|builder| {
        builder.with_x11();
    }));
    options
}

fn has_x11_keyboard_library() -> bool {
    // Some Wayland-only installations have DISPLAY but omit this optional
    // winit X11 library. Keep native Wayland working rather than panic.
    let handle = unsafe {
        libc::dlopen(c"libxkbcommon-x11.so.0".as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL)
    };
    if handle.is_null() {
        return false;
    }
    unsafe { libc::dlclose(handle); }
    true
}

pub fn supports_hide(handle: raw_window_handle::RawWindowHandle) -> bool {
    matches!(handle, raw_window_handle::RawWindowHandle::Xlib(_)
        | raw_window_handle::RawWindowHandle::Xcb(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_windows_linux_xwayland_selection_preserves_wayland_only_desktops() {
        assert!(has_x11_display(Some(OsStr::new(":0"))));
        assert!(!has_x11_display(Some(OsStr::new(""))));
        assert!(!has_x11_display(None));
    }

    #[test]
    fn non_windows_linux_native_wayland_does_not_claim_window_hiding_support() {
        let surface = std::ptr::NonNull::dangling();
        let wayland = raw_window_handle::WaylandWindowHandle::new(surface);
        assert!(!supports_hide(raw_window_handle::RawWindowHandle::Wayland(wayland)));
        let x11 = raw_window_handle::XlibWindowHandle::new(123);
        assert!(supports_hide(raw_window_handle::RawWindowHandle::Xlib(x11)));
    }
}
