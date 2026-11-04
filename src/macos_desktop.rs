//! Read AppKit window state directly on the UI thread.
//!
//! egui-winit 0.31 deliberately does not refresh minimized state on macOS.
//! Its cached `true` after our close-to-tray command survives Dock restore.
use objc2::MainThreadMarker;
use objc2_app_kit::NSView;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub fn is_minimized(window: &impl HasWindowHandle) -> Option<bool> {
    let _main_thread = MainThreadMarker::new()?;
    let borrowed = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(handle) = borrowed.as_raw() else { return None; };
    // SAFETY: eframe supplies the live window's NSView. The borrowed raw
    // handle remains valid for this call, and AppKit is accessed only on the
    // main thread. Query AppKit directly instead of winit's dispatching getter.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    Some(view.window()?.isMiniaturized())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_windows_macos_state_query_rejects_worker_thread_access() {
        struct WindowProbe;
        impl HasWindowHandle for WindowProbe {
            fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
                panic!("must not access AppKit window handles off the main thread");
            }
        }
        assert_eq!(is_minimized(&WindowProbe), None);
    }
}
