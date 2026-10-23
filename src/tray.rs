//! System-tray integration for close-to-tray behaviour.
//!
//! Tray callbacks wake the native window and queue meaningful actions for
//! the app thread. Hover events are ignored. Actions run once in update(),
//! and Exit uses eframe's normal close path so ownership and settings drop.
//!
//! On Windows, close keeps the window iconic to avoid the current eframe
//! hidden-window redraw spin. Native tool-window styles suppress its taskbar
//! and Alt+Tab entries; restore puts back its original extended style.
//!
//! macOS menu-bar diffs (see [`tray_title_text`] and `init_tray`): the
//! status item is icon-only with the icon as a template silhouette (native
//! dark/light rendering, per HIG); title text next to the icon is a
//! Windows/Linux convention and is omitted there. No LSUIElement — the dock
//! icon stays, close-to-tray is minimize via the shared viewport path, and
//! restore is viewport commands only (no Win32 wake exists off-Windows).
//!
//! [`TrayState`] owns the icon, menu and items for the app lifetime —
//! dropping it removes the icon. Creation returns `None` where no tray is
//! available (headless CI); callers then fall back to normal quit-on-close
//! behaviour.
//!
//! NOTE: the tray stack is currently Windows-only — all call sites in
//! `main.rs` (init, close-intercept, settings toggle) are
//! `#[cfg(target_os = "windows")]`-gated and this module carries
//! `allow(dead_code)` off-Windows. macOS menu-bar / Linux indicator support
//! is deferred, not removed: the builder diffs and pure mapping helpers are
//! kept portable and unit-tested on every OS.

use std::sync::{mpsc, Mutex, OnceLock};

use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

pub const TRAY_SHOW_ID: &str = "databeam_show";
pub const TRAY_QUIT_ID: &str = "databeam_quit";

/// Owned tray handles. Keep inside `DataBeamApp` for the app lifetime.
pub struct TrayState {
    _tray: TrayIcon,
    generation: u64,
    _menu: Menu,
    _show_item: MenuItem,
    _quit_item: MenuItem,
}

/// Raw tray/menu events forwarded by the installed handlers.
#[derive(Debug, Clone)]
pub enum TrayRawEvent {
    Icon(TrayIconEvent),
    Menu(MenuEvent),
}

impl TrayRawEvent {
    /// Short stable tag for the debug log.
    pub fn kind(&self) -> &'static str {
        match self {
            TrayRawEvent::Icon(event) => match event {
                TrayIconEvent::Click { .. } => "icon-click",
                TrayIconEvent::DoubleClick { .. } => "icon-double-click",
                TrayIconEvent::Enter { .. } => "icon-enter",
                TrayIconEvent::Move { .. } => "icon-move",
                TrayIconEvent::Leave { .. } => "icon-leave",
                _ => "icon-unknown",
            },
            TrayRawEvent::Menu(_) => "menu",
        }
    }
}

/// High-level tray actions for the app thread to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    Quit,
}

/// Process-wide forward entry, refreshed on every [`init_tray`] so
/// toggle-off/on cycles can never strand events on a dropped channel.
/// Handlers are installed once (both crates ignore later `set` calls).
/// The handler wakes the native window; update() owns action application.
struct TrayForward {
    generation: u64,
    tx: mpsc::Sender<TrayRawEvent>,
    ctx: eframe::egui::Context,
    // Plain strings: Menu/MenuItem handles are main-thread-only (!Send),
    // so only ids cross threads. Compared via MenuId::as_ref().
    show_id: String,
    quit_id: String,
    hwnd: Option<isize>,
}

static TRAY_FORWARD: OnceLock<Mutex<Option<TrayForward>>> = OnceLock::new();

fn forward_entry() -> &'static Mutex<Option<TrayForward>> {
    TRAY_FORWARD.get_or_init(|| Mutex::new(None))
}

/// Current Win32 HWND, if captured at init.
pub fn tray_hwnd() -> Option<isize> {
    forward_entry().lock().ok()?.as_ref()?.hwnd
}

/// Restore the window immediately on the calling thread (any thread):
/// raw Win32 show first (needs no event loop — this is what wakes a hidden
/// loop), then winit-consistent viewport commands.
pub fn restore_window_now(
    ctx: &eframe::egui::Context,
    // Used only for raw Win32 show; other platforms drive the viewport only.
    #[cfg_attr(not(windows), allow(unused_variables))] hwnd: Option<isize>,
) {
    #[cfg(windows)]
    if let Some(hwnd) = hwnd {
        sw_show(hwnd);
    }
    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Focus);
}

/// Only wake the native window here. The app applies the queued action
/// once, including graceful Exit, when this wake resumes its event loop.
fn wake_from_handler() {
    if let Some(hwnd) = tray_hwnd() {
        sw_show(hwnd);
    }
}

impl Drop for TrayState {
    fn drop(&mut self) {
        if let Ok(mut entry) = forward_entry().lock() {
            if entry.as_ref().is_some_and(|entry| entry.generation == self.generation) {
                *entry = None;
            }
        }
    }
}

/// Debug-log switch (default off). The log file grows without rotation,
// so it stays off unless the user enables it in Settings. Process-wide so
// the tray handler thread sees the same value as the UI thread.
static DEBUG_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn set_debug_enabled(enabled: bool) {
    // A saved debug setting must not enable hidden logging in a release build.
    DEBUG_ENABLED.store(cfg!(debug_assertions) && enabled, std::sync::atomic::Ordering::SeqCst);
}

/// Append one line to the tray debug log in the system temp dir.
/// Best-effort: all errors ignored. No-op unless enabled via Settings.
pub fn debug_log(line: &str) {
    if !DEBUG_ENABLED.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let ms = START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis();
    let mut path = std::env::temp_dir();
    path.push("databeam-tray-debug.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write as _;
        let _ = writeln!(file, "[+{ms}ms] {line}");
    }
}

fn raw_event_has_action(event: &TrayRawEvent) -> bool {
    match event {
        TrayRawEvent::Icon(event) => map_icon_event(event),
        TrayRawEvent::Menu(event) => map_menu_id(TRAY_SHOW_ID, TRAY_QUIT_ID, event).is_some(),
    }
}

fn forward_raw(event: TrayRawEvent) {
    // Hover, movement and unrelated menu events must neither queue nor wake
    // an idle application. A click is delivered exactly once to update().
    if !raw_event_has_action(&event) { return; }
    debug_log(&format!("tray event forwarded: {}", event.kind()));
    let delivered = forward_entry().lock().ok().and_then(|guard| {
        guard.as_ref().map(|fwd| {
            let delivered = fwd.tx.send(event).is_ok();
            if delivered { fwd.ctx.request_repaint(); }
            delivered
        })
    }).unwrap_or(false);
    debug_log(&format!("tray forward delivered: {delivered}"));
}

fn install_handlers_once() {
    static INSTALLED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    if INSTALLED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    // Native restore wakes an iconic window before the app drains actions.
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if map_icon_event(&event) {
            wake_from_handler();
        }
        forward_raw(TrayRawEvent::Icon(event));
    }));
    // Menu picks need this tray's item ids, read from the shared entry.
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let action = forward_entry().lock().ok().and_then(|guard| {
            let fwd = guard.as_ref()?;
            map_menu_id(&fwd.show_id, &fwd.quit_id, &event)
        });
        if action.is_some() {
            wake_from_handler();
        }
        forward_raw(TrayRawEvent::Menu(event));
    }));
}

fn load_tray_icon() -> Option<Icon> {
    let png = include_bytes!("../assets/icons/icon-64.png");
    let image = image::load_from_memory(png).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).ok()
}

/// Status-item title text. macOS menu-bar convention is icon-only — text
/// next to the icon is non-standard there — while Windows/Linux show the
/// label. Pure (no muda/tray objects) so it is unit-tested on every OS,
/// unlike the init path which needs the main thread on macOS.
pub fn tray_title_text() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        None
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some("DataBeam")
    }
}

/// Create the tray icon + right-click menu (`Show DataBeam` / `Exit`) and
/// return the owned state plus the raw-event channel. `hwnd` is captured for
/// direct Win32 wake/restore from the handler thread (None off-Windows).
/// Returns `None` when the platform cannot provide a tray. Call from the
/// main thread; the returned state must be kept alive (dropping removes
/// the icon).
pub fn init_tray(
    ctx: eframe::egui::Context,
    hwnd: Option<isize>,
) -> Option<(TrayState, mpsc::Receiver<TrayRawEvent>)> {
    let menu = Menu::new();
    let show_item = MenuItem::with_id(TRAY_SHOW_ID, "Show DataBeam", true, None);
    let quit_item = MenuItem::with_id(TRAY_QUIT_ID, "Exit", true, None);
    // Append results are logged, not ignored: a degraded second menu
    // (toggle re-enable path) must leave evidence instead of failing mute.
    if menu.append(&show_item).is_err() {
        debug_log("tray init failed: append show item");
        return None;
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    if menu.append(&quit_item).is_err() {
        debug_log("tray init failed: append quit item");
        return None;
    }

    let icon = load_tray_icon().or_else(|| {
        debug_log("tray init failed: icon load");
        None
    })?;

    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu.clone()))
        // Explicit: left-click restores via Click events. The library
        // DEFAULT shows the menu on left-click too (verified in source),
        // which fights the restore and confuses the toggle-re-enable path.
        // (macOS: left-click then delivers Click; right/Ctrl-click pops
        // the menu — same split as Windows.)
        .with_menu_on_left_click(false)
        .with_tooltip("DataBeam — Secure & Fast Transfer")
        .with_icon(icon);
    // macOS menu-bar diff: template silhouette (native tint in dark/light
    // mode) instead of the full-color icon; the 64px asset's alpha channel
    // becomes the mask. A dedicated monochrome asset can replace it later.
    #[cfg(target_os = "macos")]
    {
        builder = builder.with_icon_as_template(true);
    }
    // Title text beside the icon is Windows/Linux convention only.
    if let Some(title) = tray_title_text() {
        builder = builder.with_title(title);
    }
    let tray = match builder.build() {
        Ok(tray) => tray,
        Err(e) => {
            debug_log(&format!("tray init failed: build: {e:?}"));
            return None;
        }
    };
    debug_log("tray icon built");

    static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    if let Ok(mut guard) = forward_entry().lock() {
        *guard = Some(TrayForward {
            generation,
            tx,
            ctx,
            show_id: TRAY_SHOW_ID.to_string(),
            quit_id: TRAY_QUIT_ID.to_string(),
            hwnd,
        });
    }
    install_handlers_once();

    Some((
        TrayState {
            generation,
            _tray: tray,
            _menu: menu,
            _show_item: show_item,
            _quit_item: quit_item,
        },
        rx,
    ))
}

/// True when a tray icon is currently owned.
pub fn is_active() -> bool {
    forward_entry()
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false)
}

/// Drain the forwarded raw-event channel without blocking, mapping to
/// actions with the stored item ids. The handler wakes the native loop;
/// update() owns the sole action-application path.
pub fn drain_actions_global(rx: &mpsc::Receiver<TrayRawEvent>) -> Vec<TrayAction> {
    let mut actions = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            TrayRawEvent::Icon(icon_event) => {
                if map_icon_event(&icon_event) {
                    actions.push(TrayAction::Show);
                }
            }
            TrayRawEvent::Menu(menu_event) => {
                let action = forward_entry().lock().ok().and_then(|guard| {
                    let fwd = guard.as_ref()?;
                    map_menu_id(&fwd.show_id, &fwd.quit_id, &menu_event)
                });
                if let Some(action) = action {
                    actions.push(action);
                }
            }
        }
    }
    actions
}

/// A left-click release or left double-click restores the
/// window. Button press is ignored to avoid applying one click twice.
/// Right-clicks (menu opens separately)
/// and other buttons are ignored.
pub fn map_icon_event(event: &TrayIconEvent) -> bool {
    match event {
        TrayIconEvent::Click { button, button_state, .. } => {
            *button == MouseButton::Left && *button_state == tray_icon::MouseButtonState::Up
        },
        TrayIconEvent::DoubleClick { button, .. } => *button == MouseButton::Left,
        _ => false,
    }
}

/// Pure menu-id mapping (unit-testable without a real tray). Ids are plain
/// strings because menu handles are main-thread-only.
pub fn map_menu_id(show_id: &str, quit_id: &str, event: &MenuEvent) -> Option<TrayAction> {
    if event.id.as_ref() == show_id {
        Some(TrayAction::Show)
    } else if event.id.as_ref() == quit_id {
        Some(TrayAction::Quit)
    } else {
        None
    }
}

// ── Window visibility ──────────────────────────────────────────────

#[cfg(windows)]
fn hidden_window_style() -> &'static Mutex<Option<(isize, isize)>> {
    static STYLE: OnceLock<Mutex<Option<(isize, isize)>>> = OnceLock::new();
    STYLE.get_or_init(|| Mutex::new(None))
}

/// Native taskbar/Alt+Tab suppression while retaining the iconic state
/// required by the current eframe backend for low CPU. The original style
/// is restored exactly when a tray action brings the main window back.
#[cfg(windows)]
pub fn sw_hide_to_tray(hwnd: isize) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let window = hwnd as HWND;
    unsafe {
        if IsWindow(window) == 0 { return; }
        let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
        if let Ok(mut saved) = hidden_window_style().lock() {
            if saved.is_some() { return; }
            *saved = Some((hwnd, style));
        } else { return; }
        // Windows requires hiding before changing taskbar-related styles.
        ShowWindow(window, SW_MINIMIZE);
        ShowWindow(window, SW_HIDE);
        SetWindowLongPtrW(window, GWL_EXSTYLE,
            (style & !(WS_EX_APPWINDOW as isize)) | WS_EX_TOOLWINDOW as isize);
        SetWindowPos(window, std::ptr::null_mut(), 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        ShowWindow(window, SW_SHOWMINNOACTIVE);
    }
}

/// Wake and restore the native main window. Repeated calls leave a visible
/// window's geometry (including maximization) alone.
#[cfg(windows)]
pub fn sw_show(hwnd: isize) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let saved = hidden_window_style().lock().ok().and_then(|mut saved| {
        if saved.as_ref().is_some_and(|(window, _)| *window == hwnd) {
            saved.take().map(|(_, style)| style)
        } else { None }
    });
    unsafe {
        let window = hwnd as HWND;
        if IsWindow(window) == 0 { return; }
        if let Some(style) = saved {
            ShowWindow(window, SW_HIDE);
            SetWindowLongPtrW(window, GWL_EXSTYLE, style);
            SetWindowPos(window, std::ptr::null_mut(), 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        }
        if saved.is_some() || IsIconic(window) != 0 || IsWindowVisible(window) == 0 {
            ShowWindow(window, SW_RESTORE);
        }
    }
}

#[cfg(not(windows))]
pub fn sw_show(_hwnd: isize) {}

/// Extract the Win32 HWND from a raw window handle, if this is a Win32 window.
pub fn hwnd_from_raw(handle: raw_window_handle::RawWindowHandle) -> Option<isize> {
    match handle {
        raw_window_handle::RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tray_icon::{MouseButtonState, Rect, TrayIconId, dpi};

    fn click_event(button: MouseButton, button_state: MouseButtonState) -> TrayIconEvent {
        TrayIconEvent::Click {
            id: TrayIconId::new("test"),
            position: dpi::PhysicalPosition::new(0.0, 0.0),
            rect: Rect::default(),
            button,
            button_state,
        }
    }

    #[test]
    fn tray_icon_click_mapping() {
        // Only release restores: press must not duplicate the action.
        assert!(map_icon_event(&click_event(
            MouseButton::Left,
            MouseButtonState::Up
        )));
        assert!(!map_icon_event(&click_event(
            MouseButton::Left,
            MouseButtonState::Down
        )));
        // Right release opens the menu separately — never a Show.
        assert!(!map_icon_event(&click_event(
            MouseButton::Right,
            MouseButtonState::Up
        )));
        // Middle release is ignored too.
        assert!(!map_icon_event(&click_event(
            MouseButton::Middle,
            MouseButtonState::Up
        )));
    }

    #[test]
    fn hover_events_do_not_wake_or_queue_actions() {
        assert!(!raw_event_has_action(&TrayRawEvent::Icon(TrayIconEvent::Move {
            id: TrayIconId::new("test"),
            position: dpi::PhysicalPosition::new(0.0, 0.0),
            rect: Rect::default(),
        })));
        assert!(!raw_event_has_action(&TrayRawEvent::Icon(click_event(MouseButton::Right, MouseButtonState::Up))));
        assert!(!raw_event_has_action(&TrayRawEvent::Menu(MenuEvent { id: "unrelated".into() })));
        assert!(raw_event_has_action(&TrayRawEvent::Icon(click_event(MouseButton::Left, MouseButtonState::Up))));
    }

    #[cfg(windows)]
    #[test]
    fn native_tray_hide_restores_window_style_and_geometry() {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        use windows_sys::Win32::Foundation::RECT;
        let class = [83u16, 84, 65, 84, 73, 67, 0]; // built-in STATIC class
        unsafe {
            let window = CreateWindowExW(WS_EX_APPWINDOW, class.as_ptr(), std::ptr::null(),
                WS_OVERLAPPEDWINDOW, -32000, -32000, 460, 400,
                std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            assert!(!window.is_null());
            let original = GetWindowLongPtrW(window, GWL_EXSTYLE);
            sw_hide_to_tray(window as isize);
            assert_ne!(IsIconic(window), 0);
            let hidden = GetWindowLongPtrW(window, GWL_EXSTYLE);
            assert_eq!(hidden & WS_EX_APPWINDOW as isize, 0);
            assert_ne!(hidden & WS_EX_TOOLWINDOW as isize, 0);
            sw_show(window as isize);
            assert_eq!(GetWindowLongPtrW(window, GWL_EXSTYLE), original);
            assert_eq!(IsIconic(window), 0);
            assert_ne!(IsWindowVisible(window), 0);
            let mut rect: RECT = std::mem::zeroed();
            GetWindowRect(window, &mut rect);
            assert_eq!((rect.right - rect.left, rect.bottom - rect.top), (460, 400));
            // Repeat a toggle cycle to catch stale saved styles.
            sw_hide_to_tray(window as isize);
            sw_show(window as isize);
            assert_eq!(GetWindowLongPtrW(window, GWL_EXSTYLE), original);
            DestroyWindow(window);
        }
    }

    #[test]
    fn tray_menu_id_mapping() {
        use tray_icon::menu::MenuEvent;
        let show_id = TRAY_SHOW_ID;
        let quit_id = TRAY_QUIT_ID;
        assert_eq!(
            map_menu_id(show_id, quit_id, &MenuEvent {
                id: show_id.into()
            }),
            Some(TrayAction::Show)
        );
        assert_eq!(
            map_menu_id(show_id, quit_id, &MenuEvent {
                id: quit_id.into()
            }),
            Some(TrayAction::Quit)
        );
        assert_eq!(
            map_menu_id(show_id, quit_id, &MenuEvent { id: "other".into() }),
            None
        );
    }

    #[test]
    fn tray_title_diff_matches_platform() {
        // Pure helper (no Menu/tray objects) so this runs on every OS,
        // unlike the init tests which need the main thread on macOS.
        if cfg!(target_os = "macos") {
            assert_eq!(tray_title_text(), None);
        } else {
            assert_eq!(tray_title_text(), Some("DataBeam"));
        }
    }

    #[test]
    fn tray_icon_double_click_mapping() {
        let left_double = TrayIconEvent::DoubleClick {
            id: TrayIconId::new("test"),
            position: dpi::PhysicalPosition::new(0.0, 0.0),
            rect: Rect::default(),
            button: MouseButton::Left,
        };
        assert!(map_icon_event(&left_double));
        let right_double = TrayIconEvent::DoubleClick {
            id: TrayIconId::new("test"),
            position: dpi::PhysicalPosition::new(0.0, 0.0),
            rect: Rect::default(),
            button: MouseButton::Right,
        };
        assert!(!map_icon_event(&right_double));
    }

    /// Toggle re-enable path: a second init in the same process must build a
    /// working icon+menu and refresh the shared forward entry (no stale
    /// channel, same id values). Needs a windowing system; gracefully skips
    /// where no tray exists, e.g. headless CI. May briefly show an icon on
    /// dev machines; dropped at test end.
    /// Windows-only: muda Menu construction panics on non-main threads on
    /// macOS (and headless Linux has no tray), while the test harness runs
    /// tests on worker threads.
    #[cfg_attr(not(target_os = "windows"), ignore)]
    #[test]
    fn tray_double_init_replaces_forward_entry() {
        use tray_icon::menu::MenuEvent;
        let ctx = eframe::egui::Context::default();
        let Some((_state1, _rx1)) = init_tray(ctx.clone(), None) else {
            return;
        };
        // Simulate toggle-off: drop the first icon.
        drop(_state1);
        assert!(!is_active());
        let Some((_state2, rx2)) = init_tray(ctx.clone(), None) else {
            return;
        };
        assert!(is_active());
        // The refreshed entry must route a Show pick to actions through the
        // NEW channel (proves no stale-channel strand after re-enable).
        let show_id: String = forward_entry()
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|fwd| fwd.show_id.clone()))
            .expect("forward entry present after init");
        forward_entry()
            .lock()
            .ok()
            .and_then(|guard| {
                guard.as_ref().map(|fwd| {
                    fwd.tx
                        .send(TrayRawEvent::Menu(MenuEvent { id: show_id.as_str().into() }))
                        .ok()
                })
            });
        assert_eq!(drain_actions_global(&rx2), vec![TrayAction::Show]);
    }
}
