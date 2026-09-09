//! System-tray integration for close-to-tray behaviour.
//!
//! Wake-up design (required, not optional): `update()` polling alone cannot
//! work — a hidden/idle window receives no `update()` calls, so tray clicks
//! would queue unseen. Raw [`TrayIconEvent`] / [`MenuEvent`]s are therefore
//! forwarded from installed handlers into an app-owned channel together with
//! a [`egui::Context::request_repaint`] wakeup; [`TrayState::drain_actions`]
//! (called from `update()`) then maps them to [`TrayAction`]s. The mapping
//! functions are pure and unit-tested.
//!
//! Window semantics mirror proven tray apps on the same stack: hide goes
//! through the winit-consistent viewport command; restore calls raw Win32
//! ShowWindow FIRST on Windows (needs no event loop — its OS messages wake
//! a hidden loop) followed by the same viewport commands; any left-click
//! restores, menu Exit quits.
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
//! available (Linux without appindicator, headless CI); callers then fall
//! back to normal quit-on-close behaviour.

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
/// The handler thread applies Show/Quit DIRECTLY (raw Win32 wake + shared
/// quit flag) because `update()` does not run while the window is hidden —
/// verified: ~150 forwarded events, zero `update()` drains. The `update()`
/// drain stays as a backup for when the loop is already awake.
struct TrayForward {
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

fn apply_from_handler(action: TrayAction) {
    // Snapshot what the handler needs, then act without holding the lock.
    // Quit is immediate (see hard_quit): no loop roundtrips.
    let snapshot = forward_entry().lock().ok().and_then(|guard| {
        guard.as_ref().map(|fwd| {
            (
                fwd.ctx.clone(),
                fwd.hwnd,
            )
        })
    });
    if let Some((ctx, hwnd)) = snapshot {
        match action {
            TrayAction::Show => {
                debug_log("tray action applied: Show");
                restore_window_now(&ctx, hwnd);
            }
            TrayAction::Quit => {
                debug_log("tray action applied: Quit");
                hard_quit();
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
    DEBUG_ENABLED.store(enabled, std::sync::atomic::Ordering::SeqCst);
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

fn forward_raw(event: TrayRawEvent) {
    // Hover move/enter/leave spam the log at 60Hz; clicks/menu/leave prove flow.
    let noisy = matches!(
        event,
        TrayRawEvent::Icon(
            TrayIconEvent::Move { .. } | TrayIconEvent::Enter { .. }
        )
    );
    if !noisy {
        debug_log(&format!("tray event forwarded: {}", event.kind()));
    }
    let mut delivered = false;
    if let Ok(guard) = forward_entry().lock() {
        if let Some(fwd) = guard.as_ref() {
            delivered = fwd.tx.send(event).is_ok();
            fwd.ctx.request_repaint();
        }
    }
    if !noisy || !delivered {
        debug_log(&format!("tray forward delivered: {delivered}"));
    }
}

fn install_handlers_once() {
    static INSTALLED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    if INSTALLED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    // Icon clicks are mapped here (no item ids needed) and applied at once:
    // the hidden loop may never wake to drain the backup channel.
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if map_icon_event(&event) {
            apply_from_handler(TrayAction::Show);
        }
        forward_raw(TrayRawEvent::Icon(event));
    }));
    // Menu picks need this tray's item ids, read from the shared entry.
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let action = forward_entry().lock().ok().and_then(|guard| {
            let fwd = guard.as_ref()?;
            map_menu_id(&fwd.show_id, &fwd.quit_id, &event)
        });
        if let Some(action) = action {
            apply_from_handler(action);
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

    let (tx, rx) = mpsc::channel();
    if let Ok(mut guard) = forward_entry().lock() {
        *guard = Some(TrayForward {
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
/// actions with the stored item ids. Backup path for when the loop runs;
/// the handler already applies directly.
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

/// Instant quit for the tray Exit path: kill tracked transfer children (no
/// orphans — `process::exit` skips `Drop`), then exit. Deterministic: needs
/// no event loop cooperation, so quit time equals taskkill time (~100ms).
/// Tradeoffs vs graceful close: the tray icon ghosts until the next
/// mouse-over (Windows clears dead icons on hover) and eframe UI-state
/// persistence (window geometry) is skipped. Settings are safe
/// (`settings.json` persists incrementally).
pub fn hard_quit() -> ! {
    debug_log("tray hard quit: killing children");
    crate::backend::kill_all_tracked_children();
    debug_log("tray hard quit: exiting");
    std::process::exit(0);
}

/// Any left-click (press or release) and left double-click restores the
/// window — matching proven tray apps on this stack, which do not depend on
/// a particular button-state delivery. Right-clicks (menu opens separately)
/// and other buttons are ignored.
pub fn map_icon_event(event: &TrayIconEvent) -> bool {
    match event {
        TrayIconEvent::Click { button, .. } => *button == MouseButton::Left,
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
// Hide goes through the winit-consistent egui viewport command issued by
// the caller (the OS-agnostic equivalent of Tauri's window.hide()).
// Restore additionally calls raw Win32 ShowWindow FIRST: showing needs no
// event loop, and its OS messages wake a hidden loop so queued viewport
// commands get processed. Verified necessary: repaint wakeups alone never
// produced an update() while hidden.

/// Restore a hidden/minimized window via raw Win32. Thread-safe: callable
/// from the tray handler thread. Idempotent when already visible.
#[cfg(windows)]
pub fn sw_show(hwnd: isize) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_RESTORE};
    unsafe {
        ShowWindow(hwnd as HWND, SW_RESTORE);
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
        // Any left-click restores the window (press or release).
        assert!(map_icon_event(&click_event(
            MouseButton::Left,
            MouseButtonState::Up
        )));
        assert!(map_icon_event(&click_event(
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
