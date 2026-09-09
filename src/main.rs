#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod backend;
mod theme;
mod tray;
mod widgets;

use eframe::egui;
use egui::{Color32, RichText, Vec2};
use qrcode::types::Color as QrModuleColor;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use backend::*;
use theme::*;
use widgets::*;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

// Window geometry baselines: builder size and the minimum accepted size.
// The restore guard reuses the minimums so a shrunken restore can never be
// mistaken for a legitimate user resize (users cannot go below minimum).
const WINDOW_DEFAULT_W: f32 = 620.0;
const WINDOW_DEFAULT_H: f32 = 820.0;
const WINDOW_MIN_W: f32 = 460.0;
const WINDOW_MIN_H: f32 = 400.0;

/// Decide whether a tray restore must force the window size.
/// Returns the size to apply when `current` is below the usable minimum
/// (a state no legitimate resize can produce); otherwise `None` so healthy
/// restores — including user-resized windows — are never touched.
fn restore_size_override(current: [f32; 2], last_good: [f32; 2]) -> Option<[f32; 2]> {
    if current[0] < WINDOW_MIN_W || current[1] < WINDOW_MIN_H {
        Some(last_good)
    } else {
        None
    }
}

// ── Application State ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum AppView {
    Home,
    Send,
    Receive,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SelectedTool {
    Croc,
    Sendme,
    EazySendme,
}

#[derive(Debug, Clone, PartialEq)]
enum TransferState {
    Idle,
    Running,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TransferPhase {
    Preparing,
    WaitingForReceiver,
    Transferring,
    // EazySendme specific phases
    EazySharingTicket,
    EazyWaitingForPeer,
}

/// Croc self-update (`croc update --check` → `croc update`) state machine.
/// Check and apply each run on a background thread; results come back over
/// channels polled once per frame. At most one phase is ever in flight, and
/// update work never overlaps a running transfer in either direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrocUpdatePhase {
    Idle,
    Checking,
    Updating,
}

struct CrocUpdateCheckResult {
    check: Result<CrocUpdateCheck, String>,
    binary: String,
}

struct CrocUpdateApplyResult {
    outcome: Result<CrocUpdateApplyOutcome, String>,
    binary: String,
    previous_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerRequest {
    SendFolder,
    SendFiles,
    ReceiveFolder,
    SendmeBlobFolder,
}

#[derive(Debug)]
enum PickerResult {
    SendFolder(Option<PathBuf>),
    SendFiles(Option<Vec<PathBuf>>),
    ReceiveFolder(Option<PathBuf>),
    SendmeBlobFolder(Option<PathBuf>),
}

/// A file/folder entry with its cached size
#[derive(Debug, Clone)]
struct SendItem {
    path: PathBuf,
    size: Option<u64>,
    is_dir: bool,
}

impl SendItem {
    fn new(path: PathBuf) -> Self {
        let is_dir = path.is_dir();
        let size = cached_path_size(&path);
        Self { path, size, is_dir }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EazyCacheEntry {
    ticket: String,
    #[serde(default)]
    timestamp: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum SendmeBlobDirMode {
    #[default]
    SystemTemp,
    DownloadDir,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserSettings {
    #[serde(default)]
    croc_recent_codes: Vec<String>,
    #[serde(default)]
    eazysendme_recent_codes: Vec<String>,
    #[serde(default)]
    croc_receive_recent_codes: Vec<String>,
    #[serde(default)]
    receive_output_dir: Option<String>,
    #[serde(default)]
    sendme_blob_dir_mode: SendmeBlobDirMode,
    #[serde(default)]
    sendme_blob_custom_dir: Option<String>,
    #[serde(default = "default_sendme_one_shot")]
    sendme_one_shot: bool,
    #[serde(default)]
    eazysendme_custom_code: String,
    #[serde(default = "default_true")]
    eazysendme_auto_retry: bool,
    #[serde(default)]
    croc_custom_code: String,
    #[serde(default)]
    croc_use_custom_code: bool,
    /// Close hides the window to the system tray instead of quitting.
    #[serde(default = "default_true")]
    minimize_to_tray: bool,
    /// Tray debug log to the temp dir (default off).
    #[serde(default)]
    tray_debug_log: bool,
    /// Maps croc code → cache entry containing Sendme ticket and sizes for that session.
    /// Persisted so local-blob retry works even after app restart or reset_transfer.
    #[serde(default)]
    eazysendme_code_ticket_map: HashMap<String, EazyCacheEntry>,
}


fn default_true() -> bool {
    true
}

fn default_sendme_one_shot() -> bool {
    true
}

struct DataBeamApp {
    view: AppView,
    selected_tool: SelectedTool,

    tool_statuses: Vec<ToolStatus>,
    bundled_croc: Option<PathBuf>,
    bundled_sendme: Option<PathBuf>,
    size_update_tx: Option<mpsc::Sender<(PathBuf, u64)>>,
    size_update_rx: Option<mpsc::Receiver<(PathBuf, u64)>>,

    // Send
    send_items: Vec<SendItem>,
    croc_custom_code: String,
    croc_use_custom_code: bool,
    croc_recent_codes: Vec<String>,
    croc_text_mode: bool,
    croc_text_value: String,

    // Receive
    receive_code: String,
    receive_output_dir: Option<PathBuf>,
    sendme_blob_dir_mode: SendmeBlobDirMode,
    sendme_blob_custom_dir: Option<PathBuf>,

    // Transfer
    transfer_state: TransferState,
    transfer_progress: f32,
    transfer_code: Option<String>,
    transfer_log: Vec<String>,
    transfer_rx: Option<mpsc::Receiver<TransferMsg>>,
    transfer_handle: Option<ProcessHandle>,
    transfer_start_time: Option<f64>,
    transfer_total_bytes: Option<u64>,
    transfer_done_bytes: Option<u64>,
    transfer_speed_bps: Option<f64>,
    transfer_speed_samples: Vec<f64>,
    latest_cli_progress_line: Option<String>,
    croc_file_progress: Option<(u64, u64)>,
    /// Live croc progressbar state: current bar's file name.
    croc_bar_file: Option<String>,
    /// Done bytes of the current file per its latest progressbar frame.
    croc_bar_done: u64,
    /// Total bytes of the current file per its latest progressbar frame.
    croc_bar_file_total: u64,
    /// Byte total of all files whose bars have completed (folded on file switch).
    croc_completed_bytes: u64,
    /// Time of the last parsed progressbar frame, for finalize-phase detection.
    croc_last_frame_at: Option<f64>,
    croc_received_text: Option<String>,
    croc_expect_text_payload: bool,
    /// Data path seen on this transfer (`Sending (...)` direction line),
    /// cleared per transfer. Croc mode only — never set from the EazySendme
    /// ticket leg (that leg's path says nothing about the sendme data leg).
    croc_route: Option<CrocRoute>,
    transfer_phase: TransferPhase,
    preparing_progress: f32,
    transfer_payload_start_time: Option<f64>,
    transfer_end_time: Option<f64>,
    eazysendme_auto_retry: bool,
    eazy_retry_count: u8,
    eazy_next_retry_time: Option<f64>,
    eazy_local_check_started_at: Option<f64>,
    croc_qr_popup_open: bool,
    croc_text_popup_open: bool,
    settings_popup_open: bool,
    /// Tray debug log to temp file (default off; see tray::set_debug_enabled).
    tray_debug_log: bool,

    // EazySendme
    eazysendme_custom_code: String,
    eazysendme_recent_codes: Vec<String>,
    eazysendme_ticket: Option<String>,
    eazysendme_croc_handle: Option<ProcessHandle>,
    eazysendme_croc_rx: Option<mpsc::Receiver<TransferMsg>>,

    // Croc receive
    croc_receive_recent_codes: Vec<String>,

    // Croc self-update (`croc update --check` → `croc update`, 6h cadence)
    croc_update_phase: CrocUpdatePhase,
    croc_update_check_rx: Option<mpsc::Receiver<CrocUpdateCheckResult>>,
    croc_update_apply_rx: Option<mpsc::Receiver<CrocUpdateApplyResult>>,
    croc_legacy_migration_rx: Option<mpsc::Receiver<Option<PathBuf>>>,
    croc_update_last_check_at: Option<f64>,
    croc_update_startup_check_done: bool,

    // UI
    toast_msg: Option<(String, f64, Color32)>,
    animation_time: f64,
    drag_hover: bool,
    picker_block_until: f64,
    picker_in_flight: bool,
    picker_rx: Option<mpsc::Receiver<PickerResult>>,
    sendme_one_shot: bool,
    sendme_peer_connected: bool,
    sendme_had_transfer: bool,
    sendme_waiting_after_cycle: bool,
    sendme_last_activity: Option<f64>,
    sendme_active_transfers: usize,
    sendme_total_items: Option<u64>,
    sendme_done_bytes_est: u64,
    sendme_item_progress: HashMap<u64, u64>,
    sendme_item_totals: HashMap<u64, u64>,
    sendme_stream_done_base: u64,
    sendme_stream_last_done: Option<u64>,
    sendme_stream_last_total: Option<u64>,
    sendme_sender_payload_complete: bool,
    last_done_speed_sample: Option<(f64, u64)>,
    initialized_once: bool,
    native_engines_expanded: bool,
    /// Persisted map of croc_code → sendme_ticket for local-blob retry.
    eazysendme_code_ticket_map: HashMap<String, EazyCacheEntry>,

    cleanup_scan_rx: Option<mpsc::Receiver<(Vec<PathBuf>, u64)>>,
    cleanup_targets: Vec<PathBuf>,
    cleanup_bytes: u64,
    cleanup_prompt_open: bool,

    // System tray (close hides to tray when enabled).
    // Icon ownership stays here (dropping removes the icon); quit intent
    // and menu ids live in tray:: statics (shared with handler thread).
    minimize_to_tray: bool,
    tray_state: Option<tray::TrayState>,
    tray_action_rx: Option<mpsc::Receiver<tray::TrayRawEvent>>,
    /// Win32 HWND captured at startup, independent of the tray toggle, so a
    /// later toggle-time init still restores via direct Win32 wake.
    startup_hwnd: Option<isize>,
    /// Last-known-good inner size, recorded on healthy visible frames.
    /// Session-only (never persisted): stale geometry must not outlive it.
    last_good_inner: [f32; 2],
    /// One-shot launch repair done (see update()).
    launch_size_fixed: bool,
}

impl Drop for DataBeamApp {
    fn drop(&mut self) {
        if let Some(handle) = &self.transfer_handle {
            handle.request_cancel();
        }
        if let Some(handle) = &self.eazysendme_croc_handle {
            handle.request_cancel();
        }
    }
}

impl Default for DataBeamApp {
    fn default() -> Self {
        Self {
            view: AppView::Home,
            selected_tool: SelectedTool::Sendme,
            tool_statuses: Vec::new(),
            bundled_croc: None,
            bundled_sendme: None,
            size_update_tx: None,
            size_update_rx: None,
            send_items: Vec::new(),
            croc_custom_code: String::new(),
            croc_use_custom_code: false,
            croc_recent_codes: Vec::new(),
            croc_text_mode: false,
            croc_text_value: String::new(),
            receive_code: String::new(),
            receive_output_dir: None,
            sendme_blob_dir_mode: SendmeBlobDirMode::SystemTemp,
            sendme_blob_custom_dir: None,
            transfer_state: TransferState::Idle,
            transfer_progress: 0.0,
            transfer_code: None,
            transfer_log: Vec::new(),
            transfer_rx: None,
            transfer_handle: None,
            transfer_start_time: None,
            transfer_total_bytes: None,
            transfer_done_bytes: None,
            transfer_speed_bps: None,
            transfer_speed_samples: Vec::new(),
            latest_cli_progress_line: None,
            croc_file_progress: None,
            croc_bar_file: None,
            croc_bar_done: 0,
            croc_bar_file_total: 0,
            croc_completed_bytes: 0,
            croc_last_frame_at: None,
            croc_received_text: None,
            croc_expect_text_payload: false,
            croc_route: None,
            transfer_phase: TransferPhase::Preparing,
            preparing_progress: 0.0,
            transfer_payload_start_time: None,
            transfer_end_time: None,
            eazysendme_auto_retry: true,
            eazy_retry_count: 0,
            eazy_next_retry_time: None,
            eazy_local_check_started_at: None,
            croc_qr_popup_open: false,
            croc_text_popup_open: false,
            settings_popup_open: false,
            tray_debug_log: false,
            toast_msg: None,
            animation_time: 0.0,
            drag_hover: false,
            picker_block_until: 0.0,
            picker_in_flight: false,
            picker_rx: None,
            sendme_one_shot: true,
            sendme_peer_connected: false,
            sendme_had_transfer: false,
            sendme_waiting_after_cycle: false,
            sendme_last_activity: None,
            sendme_active_transfers: 0,
            sendme_total_items: None,
            sendme_done_bytes_est: 0,
            sendme_item_progress: HashMap::new(),
            sendme_item_totals: HashMap::new(),
            sendme_stream_done_base: 0,
            sendme_stream_last_done: None,
            sendme_stream_last_total: None,
            sendme_sender_payload_complete: false,
            last_done_speed_sample: None,
            initialized_once: false,
            eazysendme_custom_code: String::new(),
            eazysendme_recent_codes: Vec::new(),
            eazysendme_ticket: None,
            eazysendme_croc_handle: None,
            eazysendme_croc_rx: None,
            croc_receive_recent_codes: Vec::new(),
            croc_update_phase: CrocUpdatePhase::Idle,
            croc_update_check_rx: None,
            croc_update_apply_rx: None,
            croc_legacy_migration_rx: None,
            croc_update_last_check_at: None,
            croc_update_startup_check_done: false,
            native_engines_expanded: false,
            eazysendme_code_ticket_map: HashMap::new(),
            cleanup_scan_rx: None,
            cleanup_targets: Vec::new(),
            cleanup_bytes: 0,
            cleanup_prompt_open: false,
            minimize_to_tray: true,
            tray_state: None,
            tray_action_rx: None,
            startup_hwnd: None,
            last_good_inner: [WINDOW_DEFAULT_W, WINDOW_DEFAULT_H],
            launch_size_fixed: false,
        }
    }
}

impl DataBeamApp {
    fn effective_progress(&self) -> f32 {
        match self.transfer_phase {
            TransferPhase::Preparing
            | TransferPhase::WaitingForReceiver
            | TransferPhase::EazySharingTicket
            | TransferPhase::EazyWaitingForPeer => 0.0,
            TransferPhase::Transferring => {
                if self.selected_tool == SelectedTool::Croc && self.croc_file_progress.is_some() {
                    return self.transfer_progress.clamp(0.0, 1.0);
                }
                let mut progress = self.transfer_progress.clamp(0.0, 1.0);
                if let (Some(done), Some(total)) =
                    (self.transfer_done_bytes, self.transfer_total_bytes)
                {
                    if total > 0 {
                        let byte_ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
                        progress = progress.max(byte_ratio);
                    }
                }
                progress
            }
        }
    }

    fn push_speed_sample(&mut self, speed_bps: f64) {
        if speed_bps < 1024.0 {
            return;
        }
        // Show current transfer speed, not averaged speed.
        self.transfer_speed_bps = Some(speed_bps);
    }

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::configure_fonts(&cc.egui_ctx);
        theme::apply_theme(&cc.egui_ctx);

        let mut app = Self::default();
        let (size_tx, size_rx) = mpsc::channel();
        app.size_update_tx = Some(size_tx);
        app.size_update_rx = Some(size_rx);

        let (croc_path, sendme_path) = init_bundled_binaries();
        app.bundled_croc = croc_path;
        app.bundled_sendme = sendme_path;

        app.tool_statuses =
            detect_all_tools(app.bundled_croc.as_ref(), app.bundled_sendme.as_ref());
        app.load_user_settings();

        // Capture the HWND unconditionally: a later toggle-time init needs
        // it for the direct Win32 wake even when no tray init runs now.
        // No tray objects are created while the toggle is off.
        use raw_window_handle::HasWindowHandle as _;
        app.startup_hwnd = cc
            .window_handle()
            .ok()
            .and_then(|handle| tray::hwnd_from_raw(handle.as_raw()));

        // System tray is ON by default; init after settings load so the
        // toggle is honoured. Tray creation may fail (no indicator service
        // on Linux, headless CI) — then close quits normally. The installed
        // handlers forward events plus a repaint wakeup (required: a hidden
        // window gets no update() calls to poll with).
        if app.minimize_to_tray && !tray::is_active() {
            if let Some((state, rx)) =
                tray::init_tray(cc.egui_ctx.clone(), Self::pick_restore_hwnd(app.startup_hwnd, tray::tray_hwnd()))
            {
                app.tray_state = Some(state);
                app.tray_action_rx = Some(rx);
            }
        }

        // Always start with EazySendme; fall back if tools are unavailable.
        let sendme_available = app
            .tool_statuses
            .iter()
            .any(|s| s.tool == Tool::Sendme && s.available);
        let croc_available = app
            .tool_statuses
            .iter()
            .any(|s| s.tool == Tool::Croc && s.available);
        let eazy_available = sendme_available && croc_available;
        if eazy_available {
            app.selected_tool = SelectedTool::EazySendme;
        } else if sendme_available {
            app.selected_tool = SelectedTool::Sendme;
        } else if croc_available {
            app.selected_tool = SelectedTool::Croc;
        }

        app
    }

    fn settings_file_path() -> Option<PathBuf> {
        // Overridable for tests (so they never touch real user data) and
        // portable installs.
        if let Some(custom) = std::env::var_os("DATABEAM_SETTINGS_FILE") {
            let custom = PathBuf::from(custom);
            if !custom.as_os_str().is_empty() {
                return Some(custom);
            }
        }
        let base = dirs::config_dir()
            .or_else(dirs::data_local_dir)
            .or_else(dirs::cache_dir)
            .or_else(dirs::home_dir)?;
        Some(base.join("databeam").join("settings.json"))
    }

    fn load_user_settings(&mut self) {
        let Some(path) = Self::settings_file_path() else {
            return;
        };
        let Ok(raw) = fs::read_to_string(path) else {
            return;
        };
        let Ok(settings) = serde_json::from_str::<UserSettings>(&raw) else {
            return;
        };

        fn dedup_codes(codes: Vec<String>) -> Vec<String> {
            let mut deduped = Vec::new();
            for code in codes {
                let clean = code.trim().to_string();
                if clean.chars().count() <= 6 || deduped.iter().any(|c| c == &clean) {
                    continue;
                }
                deduped.push(clean);
                if deduped.len() >= 5 {
                    break;
                }
            }
            deduped
        }

        self.croc_recent_codes = dedup_codes(settings.croc_recent_codes);
        self.eazysendme_recent_codes = dedup_codes(settings.eazysendme_recent_codes);
        self.croc_receive_recent_codes = dedup_codes(settings.croc_receive_recent_codes);

        // NOTE: selected_tool is intentionally NOT loaded — always starts as EazySendme.
        self.receive_output_dir = settings
            .receive_output_dir
            .as_ref()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty());
        self.sendme_blob_dir_mode = settings.sendme_blob_dir_mode;
        self.sendme_blob_custom_dir = settings
            .sendme_blob_custom_dir
            .as_ref()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty());
        self.sendme_one_shot = settings.sendme_one_shot;
        self.eazysendme_custom_code = settings.eazysendme_custom_code;
        self.eazysendme_auto_retry = true; // Always true on launch
        self.croc_custom_code = settings.croc_custom_code;
        self.croc_use_custom_code = settings.croc_use_custom_code;
        self.minimize_to_tray = settings.minimize_to_tray;
        self.tray_debug_log = settings.tray_debug_log;
        tray::set_debug_enabled(settings.tray_debug_log);
        // Limit map size to 20 entries (keep newest)
        // Accept the full map; 20 entries is tiny for JSON anyway.
        self.eazysendme_code_ticket_map = settings.eazysendme_code_ticket_map;
    }

    fn persist_user_settings(&self) {
        let Some(path) = Self::settings_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let settings = UserSettings {
            croc_recent_codes: self.croc_recent_codes.clone(),
            eazysendme_recent_codes: self.eazysendme_recent_codes.clone(),
            croc_receive_recent_codes: self.croc_receive_recent_codes.clone(),
            receive_output_dir: self
                .receive_output_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            sendme_blob_dir_mode: self.sendme_blob_dir_mode,
            sendme_blob_custom_dir: self
                .sendme_blob_custom_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            sendme_one_shot: self.sendme_one_shot,
            eazysendme_custom_code: self.eazysendme_custom_code.clone(),
            eazysendme_auto_retry: self.eazysendme_auto_retry,
            croc_custom_code: self.croc_custom_code.clone(),
            croc_use_custom_code: self.croc_use_custom_code,
            minimize_to_tray: self.minimize_to_tray,
            tray_debug_log: self.tray_debug_log,
            eazysendme_code_ticket_map: self.eazysendme_code_ticket_map.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let _ = fs::write(path, json);
        }
    }

    fn cleanup_eazy_cache_by_hash(&mut self, hash: &str, delete_blobs: bool) {
        let mut ticket_to_cleanup = None;
        self.eazysendme_code_ticket_map.retain(|_, v| {
            if let Some(h) = native_ticket_to_hex_hash(&v.ticket) {
                if h == hash {
                    if delete_blobs && ticket_to_cleanup.is_none() {
                        ticket_to_cleanup = Some(v.ticket.clone());
                    }
                    false
                } else {
                    true
                }
            } else {
                true
            }
        });
        if let Some(ticket) = ticket_to_cleanup {
            crate::backend::cleanup_sendme_receive_artifacts_for_ticket(
                &ticket,
                self.effective_sendme_blob_dir().as_deref(),
            );
        }
        self.persist_user_settings();
    }

    fn get_tool_binary(&self, tool: &Tool) -> Option<String> {
        resolve_tool_path(tool, &self.tool_statuses)
    }

    fn engine_color(&self) -> Color32 {
        match self.selected_tool {
            SelectedTool::Croc => CROC_COLOR,
            SelectedTool::Sendme => SENDME_COLOR,
            SelectedTool::EazySendme => EAZYSENDME_COLOR, // Orange-ish
        }
    }

    fn effective_receive_folder(&self) -> Option<PathBuf> {
        self.receive_output_dir
            .clone()
            .or_else(|| std::env::current_dir().ok())
    }

    fn effective_sendme_blob_dir(&self) -> Option<PathBuf> {
        match self.sendme_blob_dir_mode {
            SendmeBlobDirMode::SystemTemp => None,
            SendmeBlobDirMode::DownloadDir => self.effective_receive_folder(),
            SendmeBlobDirMode::Custom => self
                .sendme_blob_custom_dir
                .clone()
                .filter(|p| !p.as_os_str().is_empty()),
        }
    }

    fn sendme_blob_scan_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![std::env::temp_dir()];
        if let Some(custom) = self.effective_sendme_blob_dir() {
            if !roots.iter().any(|root| root == &custom) {
                roots.push(custom);
            }
        }
        roots
    }

    fn sendme_blob_dir_summary(&self) -> String {
        match self.sendme_blob_dir_mode {
            SendmeBlobDirMode::SystemTemp => std::env::temp_dir().to_string_lossy().to_string(),
            SendmeBlobDirMode::DownloadDir => self
                .effective_receive_folder()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "Same as download folder".to_string()),
            SendmeBlobDirMode::Custom => self
                .sendme_blob_custom_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "Choose a folder".to_string()),
        }
    }

    fn update_derived_speed_from_done_bytes(&mut self) {
        if self.selected_tool == SelectedTool::Sendme && self.view == AppView::Receive {
            // Receive path should use parsed CLI speed directly.
            return;
        }
        let Some(done) = self.transfer_done_bytes else {
            return;
        };
        if let Some((prev_t, prev_done)) = self.last_done_speed_sample {
            let dt = self.animation_time - prev_t;
            if dt >= 0.3 {
                if done >= prev_done {
                    let rate = (done - prev_done) as f64 / dt;
                    if rate >= 1024.0 {
                        self.transfer_speed_bps = Some(rate);
                    }
                }
                self.last_done_speed_sample = Some((self.animation_time, done));
            }
        } else {
            self.last_done_speed_sample = Some((self.animation_time, done));
        }
    }

    /// Folds a parsed croc progressbar frame into byte-accurate overall progress.
    /// When croc switches to the next file's bar, the finished file's size is
    /// accumulated so progress never resets between files.
    fn apply_croc_bar_frame(&mut self, frame: CrocBarFrame) {
        // A new bar starts with a blank 0% render, so a done-bytes drop means
        // another file with the same name began (croc's bar description is the
        // bare filename, which repeats across folders).
        let new_bar_started = match &self.croc_bar_file {
            Some(prev) => *prev != frame.file_name || frame.done_bytes < self.croc_bar_done,
            None => false,
        };
        if new_bar_started {
            self.croc_completed_bytes = self
                .croc_completed_bytes
                .saturating_add(self.croc_bar_file_total.max(self.croc_bar_done));
        }
        self.croc_bar_file = Some(frame.file_name);
        self.croc_bar_file_total = frame.total_bytes;
        self.croc_bar_done = frame.done_bytes;
        self.croc_last_frame_at = Some(self.animation_time);

        if let (Some(done_files), Some(total_files)) = (frame.file_index, frame.file_count) {
            if total_files > 0 {
                self.croc_file_progress = Some((done_files.min(total_files), total_files));
            }
        }

        // Single-file transfers announce their size only via the bar itself.
        if self.transfer_total_bytes.is_none() {
            self.transfer_total_bytes = Some(frame.total_bytes);
        }

        if self.transfer_phase != TransferPhase::Transferring {
            self.transfer_phase = TransferPhase::Transferring;
            self.preparing_progress = 1.0;
            if self.transfer_payload_start_time.is_none() {
                self.transfer_payload_start_time = Some(self.animation_time);
            }
        }

        let overall_done = self
            .croc_completed_bytes
            .saturating_add(frame.done_bytes)
            .min(self.transfer_total_bytes.unwrap_or(u64::MAX));
        if overall_done >= self.transfer_done_bytes.unwrap_or(0) {
            self.transfer_done_bytes = Some(overall_done);
        }

        let total = self.transfer_total_bytes.unwrap_or(0);
        if total > 0 {
            self.transfer_progress =
                (self.transfer_done_bytes.unwrap_or(0) as f32 / total as f32).clamp(0.0, 1.0);
        } else if let Some((done_files, total_files)) = self.croc_file_progress {
            if total_files > 0 {
                let within = frame.done_bytes as f32 / frame.total_bytes.max(1) as f32;
                self.transfer_progress = (((done_files.saturating_sub(1)) as f32 + within)
                    / total_files as f32)
                    .clamp(0.0, 1.0);
            }
        } else {
            self.transfer_progress = self
                .transfer_progress
                .max((frame.done_bytes as f32 / frame.total_bytes.max(1) as f32).clamp(0.0, 1.0));
        }

        match frame.speed_bps {
            Some(speed) if speed > 0.0 => {
                self.transfer_speed_bps = Some(speed);
                self.last_done_speed_sample =
                    Some((self.animation_time, self.transfer_done_bytes.unwrap_or(0)));
            }
            _ => self.update_derived_speed_from_done_bytes(),
        }
    }

    fn build_croc_panel_data(
        &self,
        accent: Color32,
        effective_progress: f32,
        done: u64,
        total: u64,
    ) -> CrocPanelData {
        let _ = accent;
        // croc emits no bars for unchanged/skipped files and goes quiet while
        // hashing + writing; after a few seconds of frame silence we are in
        // the finalize phase, not stalled mid-transfer.
        let finalizing = self
            .croc_last_frame_at
            .is_some_and(|t| self.animation_time - t > 4.0);
        let verb = if self.view == AppView::Send {
            "Sending"
        } else {
            "Receiving"
        };
        let current_file = self.croc_bar_file.clone().unwrap_or_default();
        let title = if finalizing {
            "Finalizing\u{2026} (verifying files)".to_string()
        } else if current_file.is_empty() {
            format!("{verb}\u{2026}")
        } else {
            format!("{verb} {current_file}")
        };

        let transferred_label = if total > 0 {
            format!(
                "{} / {}",
                format_file_size(done),
                format_file_size(total)
            )
        } else {
            format_file_size(done)
        };

        let detail_left = match self.croc_file_progress {
            Some((done_files, total_files)) if total_files > 0 => {
                Some(format!("{done_files}/{total_files} {current_file}"))
            }
            _ if !current_file.is_empty() => Some(current_file),
            _ => None,
        };

        let (rate_value, eta_value) = self.panel_rate_eta(done, total, finalizing);

        CrocPanelData {
            title,
            transferred_label,
            progress: effective_progress,
            percent_label: format!("{:.1}%", effective_progress * 100.0),
            detail_left,
            rate_value,
            eta_value,
        }
    }

    /// Live rate + overall ETA shared by the croc and sendme panels.
    fn panel_rate_eta(&self, done: u64, total: u64, finalizing: bool) -> (String, String) {
        if finalizing {
            return ("--".to_string(), "--".to_string());
        }
        let rate_value = match self.transfer_speed_bps {
            Some(speed) if speed > 0.0 => {
                format!("{}/s", format_file_size(speed.round() as u64))
            }
            _ => "--".to_string(),
        };
        let eta_value = if total > done {
            match self.transfer_speed_bps {
                Some(speed) if speed > 0.0 => {
                    format_eta_compact((total - done) as f64 / speed)
                }
                _ => "--".to_string(),
            }
        } else if total > 0 {
            "0s".to_string()
        } else {
            "--".to_string()
        };
        (rate_value, eta_value)
    }

    /// Panel data for native sendme transfers: done/total and speed come from
    /// exact native events, so no finalize heuristics are needed.
    fn build_sendme_panel_data(
        &self,
        accent: Color32,
        effective_progress: f32,
        done: u64,
        total: u64,
    ) -> CrocPanelData {
        let _ = accent;
        let verb = if self.view == AppView::Send {
            "Sending"
        } else {
            "Receiving"
        };
        let title = format!("{verb}\u{2026}");
        let transferred_label = if total > 0 {
            format!("{} / {}", format_file_size(done), format_file_size(total))
        } else {
            format_file_size(done)
        };
        let detail_left = self
            .sendme_total_items
            .map(|total_files| format!("{total_files} files"));
        let (rate_value, eta_value) = self.panel_rate_eta(done, total, false);
        CrocPanelData {
            title,
            transferred_label,
            progress: effective_progress,
            percent_label: format!("{:.1}%", effective_progress * 100.0),
            detail_left,
            rate_value,
            eta_value,
        }
    }

    /// End-to-end average speed over the actual payload transfer window
    /// (excluding prep and waiting-for-receiver time), like getcroc's Rate.
    fn average_transfer_speed_bps(&self) -> Option<f64> {
        let start = self.transfer_payload_start_time.or(self.transfer_start_time)?;
        let end = self.transfer_end_time?;
        let secs = end - start;
        if secs <= 0.0 {
            return None;
        }
        let done = self.transfer_done_bytes.unwrap_or(0) as f64;
        if done <= 0.0 {
            return None;
        }
        Some(done / secs)
    }

    fn switch_tool(&mut self, tool: SelectedTool) {
        if self.selected_tool == tool {
            return;
        }
        self.selected_tool = tool;
        self.persist_user_settings();
        self.send_items.clear();
        self.receive_code.clear();
        self.reset_transfer();
    }

    fn launch_picker(&mut self, request: PickerRequest) {
        if self.picker_in_flight {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.picker_in_flight = true;
        self.picker_rx = Some(rx);
        self.picker_block_until = self.animation_time + 0.15;
        thread::spawn(move || {
            let result = match request {
                PickerRequest::SendFolder => {
                    PickerResult::SendFolder(rfd::FileDialog::new().pick_folder())
                }
                PickerRequest::SendFiles => {
                    PickerResult::SendFiles(rfd::FileDialog::new().pick_files())
                }
                PickerRequest::ReceiveFolder => {
                    PickerResult::ReceiveFolder(rfd::FileDialog::new().pick_folder())
                }
                PickerRequest::SendmeBlobFolder => {
                    PickerResult::SendmeBlobFolder(rfd::FileDialog::new().pick_folder())
                }
            };
            let _ = tx.send(result);
        });
    }

    fn poll_picker_results(&mut self) {
        let Some(rx) = self.picker_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.picker_in_flight = false;
                self.picker_block_until = self.animation_time + 0.2;
                match result {
                    PickerResult::SendFolder(Some(path)) => {
                        self.add_path(path);
                    }
                    PickerResult::SendFiles(Some(paths)) => {
                        for path in paths {
                            self.add_path(path);
                        }
                    }
                    PickerResult::ReceiveFolder(Some(path)) => {
                        self.receive_output_dir = Some(path);
                        self.persist_user_settings();
                    }
                    PickerResult::SendmeBlobFolder(Some(path)) => {
                        self.sendme_blob_custom_dir = Some(path);
                        self.sendme_blob_dir_mode = SendmeBlobDirMode::Custom;
                        self.persist_user_settings();
                    }
                    PickerResult::SendFolder(None)
                    | PickerResult::SendFiles(None)
                    | PickerResult::ReceiveFolder(None)
                    | PickerResult::SendmeBlobFolder(None) => {}
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.picker_rx = Some(rx);
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.picker_in_flight = false;
                self.picker_block_until = self.animation_time + 0.2;
            }
        }
    }

    fn add_path(&mut self, path: PathBuf) {
        if self.selected_tool == SelectedTool::Croc && self.croc_text_mode {
            self.show_toast(
                "Disable Croc text mode to add files/folders".to_string(),
                WARNING,
            );
            return;
        }
        if !self.send_items.iter().any(|item| item.path == path) {
            if self.transfer_state != TransferState::Idle
                && self.transfer_state != TransferState::Running
            {
                self.reset_transfer();
            }
            let item = SendItem::new(path.clone());
            let needs_dir_size = item.is_dir && item.size.is_none();
            self.send_items.push(item);
            if needs_dir_size {
                if let Some(tx) = self.size_update_tx.clone() {
                    thread::spawn(move || {
                        let size = dir_size_capped(&path, 0, 64);
                        let _ = tx.send((path, size));
                    });
                }
            }
        }
    }

    fn total_size(&self) -> u64 {
        self.send_items.iter().filter_map(|i| i.size).sum()
    }

    fn total_size_complete(&self) -> bool {
        self.send_items.iter().all(|i| i.size.is_some())
    }

    fn known_total_size(&self) -> Option<u64> {
        if self.send_items.is_empty() || !self.total_size_complete() {
            None
        } else {
            Some(self.total_size())
        }
    }

    fn send_paths(&self) -> Vec<PathBuf> {
        self.send_items.iter().map(|i| i.path.clone()).collect()
    }

    fn reset_transfer(&mut self) {
        if let Some(handle) = &self.transfer_handle {
            handle.request_cancel();
        }
        if let Some(handle) = &self.eazysendme_croc_handle {
            handle.request_cancel();
        }
        self.transfer_state = TransferState::Idle;
        self.transfer_progress = 0.0;
        self.transfer_code = None;
        self.transfer_log.clear();
        self.transfer_rx = None;
        self.transfer_handle = None;
        self.transfer_start_time = None;
        self.transfer_total_bytes = None;
        self.transfer_done_bytes = None;
        self.transfer_speed_bps = None;
        self.transfer_speed_samples.clear();
        self.latest_cli_progress_line = None;
        self.croc_file_progress = None;
        self.croc_bar_file = None;
        self.croc_bar_done = 0;
        self.croc_bar_file_total = 0;
        self.croc_completed_bytes = 0;
        self.croc_last_frame_at = None;
        self.croc_received_text = None;
        self.croc_expect_text_payload = false;
        self.croc_route = None;
        self.transfer_phase = TransferPhase::Preparing;
        self.preparing_progress = 0.0;
        self.transfer_payload_start_time = None;
        self.transfer_end_time = None;
        self.croc_qr_popup_open = false;
        self.eazy_local_check_started_at = None;
        self.eazysendme_ticket = None;
        self.eazysendme_croc_handle = None;
        self.eazysendme_croc_rx = None;
        self.croc_text_popup_open = false;
        self.sendme_peer_connected = false;
        self.sendme_had_transfer = false;
        self.sendme_waiting_after_cycle = false;
        self.sendme_last_activity = None;
        self.sendme_active_transfers = 0;
        self.picker_block_until = 0.0;
        self.picker_in_flight = false;
        self.picker_rx = None;
        self.sendme_total_items = None;
        self.sendme_done_bytes_est = 0;
        self.sendme_item_progress.clear();
        self.sendme_item_totals.clear();
        self.sendme_stream_done_base = 0;
        self.sendme_stream_last_done = None;
        self.sendme_stream_last_total = None;
        self.sendme_sender_payload_complete = false;
        self.last_done_speed_sample = None;
    }

    fn mark_sendme_sender_waiting(&mut self) {
        self.transfer_phase = TransferPhase::WaitingForReceiver;
        self.transfer_payload_start_time = None;
        self.transfer_progress = 0.0;
        self.transfer_done_bytes = None;
        self.transfer_speed_bps = None;
        self.sendme_peer_connected = false;
        self.sendme_had_transfer = false;
        self.sendme_waiting_after_cycle = true;
        self.sendme_last_activity = None;
        self.sendme_active_transfers = 0;
        self.sendme_total_items = None;
        self.sendme_done_bytes_est = 0;
        self.sendme_item_progress.clear();
        self.sendme_item_totals.clear();
        self.sendme_stream_done_base = 0;
        self.sendme_stream_last_done = None;
        self.sendme_stream_last_total = None;
        self.sendme_sender_payload_complete = false;
    }

    fn mark_sendme_one_shot_completed(&mut self) {
        self.transfer_state = TransferState::Completed;
        self.eazy_retry_count = 0;
        self.eazy_next_retry_time = None;
        self.transfer_progress = 1.0;
        self.preparing_progress = 1.0;
        self.transfer_end_time = Some(self.animation_time);
        if let Some(total) = self.transfer_total_bytes {
            if total > 0 {
                self.transfer_done_bytes = Some(total);
            }
        }
        self.sendme_peer_connected = false;
        self.sendme_had_transfer = false;
        self.sendme_waiting_after_cycle = false;
        self.sendme_last_activity = None;
        self.sendme_active_transfers = 0;
        self.sendme_sender_payload_complete = false;
        if let Some(handle) = &self.transfer_handle {
            handle.request_cancel();
        }
        // NOTE: we intentionally do NOT evict the map entry here. The blob dir is cleaned up
        // by iroh after export, so the map entry becomes a dead link — but leaving it in the
        // map doesn’t cause incorrect behavior: next time the code is used, the dir won’t
        // exist and check_and_export_local will return Ok(false), falling through to Croc.
    }

    /// HWND for tray restore: the startup-captured handle wins because it is
    /// always taken, while the shared cached one only exists when a prior
    /// tray init ran. Toggle-time init with no prior init used to pass
    /// `None`, silently disabling the direct Win32 wake on every restore.
    fn pick_restore_hwnd(startup: Option<isize>, cached: Option<isize>) -> Option<isize> {
        startup.or(cached)
    }

    /// Restore a hidden/minimized main window and focus it: raw Win32 show
    /// first (needs no event loop — this is what wakes a hidden loop),
    /// then winit-consistent viewport commands. Finally the size guard:
    /// if the reported rect is below the usable minimum (shrunken-restore
    /// race), force the last-known-good size instead.
    fn show_main_window(&mut self, ctx: &egui::Context) {
        tray::debug_log("tray action applied: Show");
        if let Some(hwnd) = tray::tray_hwnd() {
            tray::sw_show(hwnd);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        let current = ctx.screen_rect();
        if let Some(size) =
            restore_size_override([current.width(), current.height()], self.last_good_inner)
        {
            tray::debug_log(&format!(
                "tray restore size guard: forcing {}x{}",
                size[0] as u32, size[1] as u32
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(size[0], size[1])));
        }
    }

    /// Hide the main window to the tray (transfers keep running).
    /// Minimize-only by design: taskbar-minimize is the verified ~0%-CPU
    /// hidden state on this stack (the OS suppresses redraw delivery for
    /// iconic windows while the event loop keeps processing real events, so
    /// tray clicks keep working). `Visible(false)` is deliberately NOT used:
    /// measured locally, a merely-hidden window spins the main thread at a
    /// full core with zero update() calls — root cause inside winit/eframe
    /// internals, still unnamed. A pending Eazy auto-retry keeps its exact
    /// wake second.
    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        tray::debug_log("window hidden to tray");
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        // A pending Eazy auto-retry keeps its exact wake second.
        if let Some(retry_time) = self.eazy_next_retry_time {
            let secs = (retry_time - self.animation_time).clamp(0.0, 3600.0);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(secs));
        }
    }

    /// Apply one tray-menu/click action. Quit is instant (see hard_quit).
    fn apply_tray_action(&mut self, ctx: &egui::Context, action: tray::TrayAction) {
        match action {
            tray::TrayAction::Show => {
                self.show_main_window(ctx);
            }
            tray::TrayAction::Quit => {
                tray::hard_quit();
            }
        }
    }

    /// Toggle close-to-tray from the settings popup (persisted).
    fn set_minimize_to_tray(&mut self, ctx: &egui::Context, enabled: bool) {
        if self.minimize_to_tray == enabled {
            return;
        }
        self.minimize_to_tray = enabled;
        if enabled && self.tray_state.is_none() {
            // No frame handle here; prefer the startup-captured HWND so the
            // direct Win32 wake works even when no prior tray init ran.
            let hwnd = Self::pick_restore_hwnd(self.startup_hwnd, tray::tray_hwnd());
            tray::debug_log(&format!(
                "tray toggle init hwnd: {}",
                if hwnd.is_some() { "present" } else { "missing" }
            ));
            if let Some((state, rx)) = tray::init_tray(ctx.clone(), hwnd) {
                self.tray_state = Some(state);
                self.tray_action_rx = Some(rx);
            }
        } else if !enabled {
            // Drop the icon immediately so it never lingers.
            self.tray_state.take();
        }
        self.persist_user_settings();
    }

    fn cancel_transfer(&mut self) {
        if let Some(handle) = &self.transfer_handle {
            handle.request_cancel();
        }
        self.transfer_rx = None;
        self.transfer_handle = None;
        self.eazy_retry_count = 0;
        self.eazy_next_retry_time = None;
        self.transfer_end_time = Some(self.animation_time);
        self.transfer_state = TransferState::Failed("Transfer cancelled".to_string());
    }

    /// True when a captured croc output line signals relay DNS resolution
    /// failure (e.g. `lookup 3.getcroc.com: i/o timeout`). The raw failure
    /// surfaces only as a generic process-exit message, so callers use this
    /// to show a DNS/VPN hint on the Failed card instead.
    fn is_relay_dns_failure_line(line: &str) -> bool {
        let lower = line.to_lowercase();
        lower.contains("no such host")
            || lower.contains("could not resolve")
            || lower.contains("cannot resolve")
            || lower.contains("temporary failure in name resolution")
            || lower.contains("server misbehaving")
            || lower.contains("dns error")
            || lower.contains("dns lookup failed")
            || (lower.contains("lookup") && lower.contains("getcroc"))
            || (lower.contains("lookup") && lower.contains("i/o timeout"))
    }

    fn fail_transfer(&mut self, e: String) {
        if let Some(handle) = &self.transfer_handle {
            handle.request_cancel();
        }

        // Don't let generic cancellation message overwrite a more specific error already set.
        if e == "Transfer cancelled" {
            if let TransferState::Failed(_) = self.transfer_state {
                return;
            }
        }

        // "blobs-incomplete" is a sentinel from sendme_check_local: it means the blob store
        // check found the data is not fully cached locally. Immediately fall through to the
        // full Croc receive path without charging a retry or waiting.
        if e == "blobs-incomplete"
            && self.selected_tool == SelectedTool::EazySendme
            && self.view == AppView::Receive
        {
            // Prevent auto-retry from re-inserting the same ticket and looping local-check forever.
            self.eazysendme_ticket = None;
            let code_key = self.receive_code.trim().to_string();
            if !code_key.is_empty() && self.eazysendme_code_ticket_map.remove(&code_key).is_some() {
                self.persist_user_settings();
            }
            self.transfer_log
                .push("Local blobs incomplete, starting full receive...".to_string());
            self.eazy_next_retry_time = Some(self.animation_time); // trigger immediately
            self.transfer_state = TransferState::Failed(e);
            self.transfer_end_time = Some(self.animation_time);
            return;
        }

        // "File already exists" should be intercepted to show the overwrite modal.
        let is_file_exists_error = e.to_lowercase().contains("already exists")
            || e.to_lowercase().contains("file exists")
            || e.to_lowercase().contains("os error 17") // EEXIST on Unix
            || e.to_lowercase().contains("os error 183"); // ERROR_ALREADY_EXISTS on Windows

        // NOTE: failure-path elapsed is shown on the Failed card for consistency;
        // the original request was the Elapsed label in the Completed body row
        // next to Avg speed.
        if is_file_exists_error {
            self.eazy_retry_count = 0;
            if self.selected_tool == SelectedTool::Croc {
                self.transfer_state = TransferState::Failed("The file/folder you intended to download has the same name as another item at the destination. Rename that item and try a new transfer.".to_string());
            }
            // NOTE: freeze end-time here too so the failure-path elapsed is
            // accurate on these early-return paths (same note as above).
            self.transfer_end_time = Some(self.animation_time);
            return;
        }

        if e == "conflict-detected" {
            self.eazy_retry_count = 0;
            self.transfer_state = TransferState::Failed("The file/folder you intended to download has the same name as another item at the destination. Rename that item and try a new transfer.".to_string());
            // NOTE: same as above — freeze end-time for an accurate
            // failure-path elapsed on this early-return path.
            self.transfer_end_time = Some(self.animation_time);
            return;
        }

        if self.selected_tool == SelectedTool::EazySendme
            && self.eazysendme_auto_retry
            && self.eazy_retry_count < 3
            && e != "Transfer cancelled"
            && !is_file_exists_error
        {
            self.eazy_retry_count += 1;

            // Sender retries immediately — it just relaunches a process (no disk import needed).
            // Receiver waits longer because the sender must re-import blobs from disk before it's
            // ready to accept connections again. We scale by payload size: min 30s, +1s per 100 MB,
            // capped at 120s.
            let wait_secs = if self.view == AppView::Send {
                0.0_f64
            } else {
                let bytes = self.transfer_total_bytes.unwrap_or(0);
                let size_mb = bytes as f64 / (100.0 * 1024.0 * 1024.0); // 1s per 100 MB
                (30.0_f64 + size_mb).min(120.0)
            };

            self.eazy_next_retry_time = Some(self.animation_time + wait_secs);
            let retry_msg = if wait_secs == 0.0 {
                format!(
                    "Transfer failed ({}). Auto-retrying {}/3 immediately...",
                    e, self.eazy_retry_count
                )
            } else {
                format!(
                    "Transfer failed ({}). Auto-retrying {}/3 in {}s...",
                    e, self.eazy_retry_count, wait_secs as u32
                )
            };
            self.transfer_log.push(retry_msg.clone());
            self.transfer_state = TransferState::Failed(retry_msg);
        } else {
            self.eazy_retry_count = 0;
            self.eazy_next_retry_time = None;
            self.transfer_state = TransferState::Failed(e);
        }
        self.transfer_end_time = Some(self.animation_time);
    }

    fn retry_send(&mut self, is_auto: bool) {
        if let Some(handle) = &self.transfer_handle {
            handle.request_cancel();
        }
        self.reset_transfer();
        self.start_send(is_auto);
    }

    fn retry_receive(&mut self, is_auto: bool) {
        if let Some(handle) = &self.transfer_handle {
            handle.request_cancel();
        }
        // Persist code→ticket association (for the is_auto path) or clear it (manual retry).
        // NOTE: start_receive reads the ticket directly from the map now, so no need to
        // restore eazysendme_ticket here — start_receive captures it before reset_transfer.
        if self.selected_tool == SelectedTool::EazySendme
            && self.view == AppView::Receive
        {
            let code_key = self.receive_code.trim().to_string();
            // Persist the in-memory ticket to the map so start_receive can find it.
            if let Some(ticket) = &self.eazysendme_ticket {
                if !code_key.is_empty() {
                    let should_update = self
                        .eazysendme_code_ticket_map
                        .get(&code_key)
                        .map(|e| e.ticket != *ticket)
                        .unwrap_or(true);
                    if should_update {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        self.eazysendme_code_ticket_map.insert(code_key, EazyCacheEntry {
                            ticket: ticket.clone(),
                            timestamp: now,
                        });
                        self.persist_user_settings();
                    }
                }
            }
        }
        self.reset_transfer();
        self.start_receive(is_auto);
    }

    fn update_transfer_metrics_from_log(&mut self, line: &str) {
        let lower = line.to_lowercase();
        let eazy_receive_pre_ticket = self.selected_tool == SelectedTool::EazySendme
            && self.view == AppView::Receive
            && self.eazysendme_ticket.is_none();
        if eazy_receive_pre_ticket {
            return;
        }
        if self.selected_tool == SelectedTool::Croc {
            if let Some(frame) = parse_croc_bar_frame(line) {
                self.apply_croc_bar_frame(frame);
                return;
            }
        }
        let eazy_sendme_active = self.selected_tool == SelectedTool::EazySendme
            && match self.view {
                AppView::Send => true,
                AppView::Receive => self.eazysendme_ticket.is_some(),
                _ => false,
            };
        let sendme_like = self.selected_tool == SelectedTool::Sendme || eazy_sendme_active;
        let sendme_r_counter = if sendme_like {
            parse_sendme_r_counter(line)
        } else {
            None
        };
        let export_line = lower.contains("exporting ");
        let croc_counter_any = if self.selected_tool == SelectedTool::Croc {
            parse_croc_file_counter_progress(line)
        } else {
            None
        };
        let sendme_raw_progress_line =
            sendme_like && self.view == AppView::Receive && sendme_r_counter.is_some();
        let sendme_sender_request_line =
            sendme_like && self.view == AppView::Send && is_sendme_sender_request_line(line);
        if is_cli_progress_line(line) || sendme_raw_progress_line || sendme_sender_request_line {
            if !(self.selected_tool == SelectedTool::Croc && croc_counter_any.is_some()) {
                self.latest_cli_progress_line = Some(compact_cli_progress_line(line));
            }
        }
        if self.selected_tool == SelectedTool::Croc
            && self.transfer_phase != TransferPhase::Transferring
            && parse_croc_direction(line).is_some()
        {
            self.transfer_phase = TransferPhase::Transferring;
            self.preparing_progress = 1.0;
            if self.transfer_payload_start_time.is_none() {
                self.transfer_payload_start_time = Some(self.animation_time);
            }
        }

        if let Some(p) = parse_stage_progress(line) {
            if self.transfer_phase == TransferPhase::Preparing {
                self.preparing_progress = self.preparing_progress.max(p);
            }
        }

        if self.selected_tool == SelectedTool::Croc {
            if let Some(total) = parse_croc_total_size_hint(line) {
                let update = match self.transfer_total_bytes {
                    Some(existing) => total > existing,
                    None => true,
                };
                if update {
                    self.transfer_total_bytes = Some(total);
                }
            }
        }
        if sendme_like {
            if let Some(total) = parse_sendme_imported_size_hint(line) {
                let update = match self.transfer_total_bytes {
                    Some(existing) => total > existing,
                    None => true,
                };
                if update {
                    self.transfer_total_bytes = Some(total);
                }
            }
            if let Some(total_items) = parse_sendme_total_files_hint(line) {
                let update = match self.sendme_total_items {
                    Some(existing) => total_items > existing,
                    None => true,
                };
                if update {
                    self.sendme_total_items = Some(total_items);
                }
            }
            if self.view == AppView::Receive {
                if let Some((counter_done, counter_total)) = sendme_r_counter {
                    if counter_total > 0 {
                        let update = match self.transfer_total_bytes {
                            Some(existing) => counter_total > existing,
                            None => true,
                        };
                        if update {
                            self.transfer_total_bytes = Some(counter_total);
                        }
                        let prev_done = self.transfer_done_bytes.unwrap_or(0);
                        let mut next_done = counter_done.max(prev_done);
                        next_done = next_done.min(counter_total);
                        if next_done > prev_done {
                            self.transfer_done_bytes = Some(next_done);
                            self.update_derived_speed_from_done_bytes();
                        }
                        self.transfer_progress =
                            (next_done as f32 / counter_total as f32).clamp(0.0, 1.0);
                        if self.transfer_phase != TransferPhase::Transferring {
                            self.transfer_phase = TransferPhase::Transferring;
                            if self.transfer_payload_start_time.is_none() {
                                self.transfer_payload_start_time = Some(self.animation_time);
                            }
                        }
                    }
                }
            }
            if self.view == AppView::Send && !sendme_sender_request_line {
                let item_index = parse_sendme_item_index(line);
                let sender_payload = parse_payload_progress(line);
                if let (Some((item_done, item_total)), Some(idx)) = (sender_payload, item_index) {
                    if item_total > 0 {
                        let key = idx.saturating_sub(1);
                        let done = item_done.min(item_total);
                        let prev_done = self.sendme_item_progress.get(&key).copied().unwrap_or(0);
                        let prev_total = self
                            .sendme_item_totals
                            .get(&key)
                            .copied()
                            .unwrap_or(item_total);
                        if done >= prev_done {
                            self.sendme_done_bytes_est = self
                                .sendme_done_bytes_est
                                .saturating_add(done.saturating_sub(prev_done));
                        } else if item_total != prev_total {
                            self.sendme_done_bytes_est = self
                                .sendme_done_bytes_est
                                .saturating_sub(prev_done)
                                .saturating_add(prev_total.max(prev_done))
                                .saturating_add(done);
                        }
                        self.sendme_item_progress.insert(key, done);
                        self.sendme_item_totals.insert(key, item_total);
                    }
                } else if let Some((item_done, item_total)) = sender_payload {
                    let done = item_done.min(item_total);
                    if item_total > 0 && line.contains(" r ") && line.contains(" # ") {
                        if let (Some(prev_done), Some(prev_total)) =
                            (self.sendme_stream_last_done, self.sendme_stream_last_total)
                        {
                            if done < prev_done {
                                self.sendme_stream_done_base =
                                    self.sendme_stream_done_base.saturating_add(prev_total);
                            }
                        }
                        self.sendme_stream_last_done = Some(done);
                        self.sendme_stream_last_total = Some(item_total);
                    }
                }
                if let Some(total_bytes) = self.transfer_total_bytes {
                    if total_bytes > 0 {
                        let stream_done = if let (Some(cur_done), Some(cur_total)) =
                            (self.sendme_stream_last_done, self.sendme_stream_last_total)
                        {
                            if cur_total > 0 {
                                self.sendme_stream_done_base
                                    .saturating_add(cur_done.min(cur_total))
                            } else {
                                self.sendme_stream_done_base
                            }
                        } else {
                            self.sendme_stream_done_base
                        };
                        let est_done = self.sendme_done_bytes_est.max(stream_done).min(total_bytes);
                        if est_done > self.transfer_done_bytes.unwrap_or(0) {
                            self.transfer_done_bytes = Some(est_done);
                            self.transfer_progress =
                                (est_done as f32 / total_bytes as f32).clamp(0.0, 1.0);
                            self.update_derived_speed_from_done_bytes();
                        }
                        if let Some((item_done, item_total)) = sender_payload {
                            self.sendme_sender_payload_complete = est_done >= total_bytes
                                && item_total > 0
                                && item_done.min(item_total) >= item_total;
                        }
                    }
                }
            }
        }

        let croc_file_counter = if self.selected_tool == SelectedTool::Croc
            && self.transfer_phase == TransferPhase::Transferring
        {
            croc_counter_any
        } else {
            None
        };
        if let Some((done_files, total_files)) = croc_file_counter {
            if total_files > 0 {
                self.croc_file_progress = Some((done_files, total_files));
                // Byte-accurate bar tracking (croc_bar_file) supersedes the
                // file-counter model once progressbar frames have been seen.
                if self.croc_bar_file.is_none() {
                    let mut overall = done_files as f32 / total_files as f32;
                    if let Some((cur_done, cur_total)) = parse_payload_progress(line) {
                        if cur_total > 0 {
                            let cur_ratio = (cur_done as f32 / cur_total as f32).clamp(0.0, 1.0);
                            let complete_before = done_files.saturating_sub(1) as f32;
                            overall =
                                ((complete_before + cur_ratio) / total_files as f32).clamp(0.0, 1.0);
                        }
                    }
                    // In Croc file-counter mode, overall progress should follow file counter directly.
                    self.transfer_progress = overall;
                    if let Some(total_bytes) = self.transfer_total_bytes {
                        if total_bytes > 0 {
                            let derived_done = (overall as f64 * total_bytes as f64) as u64;
                            self.transfer_done_bytes = Some(derived_done.min(total_bytes));
                        }
                    }
                }
            }
        }

        if !export_line && croc_file_counter.is_none() {
            let payload_progress = if sendme_like && self.view == AppView::Receive {
                parse_sendme_r_payload_progress(line).or_else(|| parse_payload_progress(line))
            } else {
                parse_payload_progress(line)
            };
            if let Some((done, total)) = payload_progress {
                if self.selected_tool == SelectedTool::Croc
                    && self.transfer_phase != TransferPhase::Transferring
                {
                    return;
                }
                let allow_sendme_payload = if sendme_like {
                    match self.view {
                        AppView::Send => {
                            lower.contains("download")
                                || lower.contains("upload")
                                || lower.contains("sending")
                                || lower.contains("transferring")
                        }
                        AppView::Receive => {
                            lower.contains("downloading")
                                || lower.contains("uploading")
                                || lower.contains("[3/4]")
                                || lower.contains("[4/4]")
                                || parse_sendme_r_payload_progress(line).is_some()
                        }
                        _ => true,
                    }
                } else {
                    true
                };
                if !allow_sendme_payload {
                    return;
                }
                if self.selected_tool == SelectedTool::Croc {
                    // Croc emits many per-file payload lines; use file-counter model only.
                    return;
                }
                if self.selected_tool == SelectedTool::Sendme
                    && self.view == AppView::Send
                    && self.transfer_phase != TransferPhase::Transferring
                    && done == 0
                {
                    let active_transfer_line = lower.contains("[3/4]")
                        || lower.contains("[4/4]")
                        || (lower.contains("downloading") && self.sendme_peer_connected);
                    if active_transfer_line {
                        self.transfer_phase = TransferPhase::Transferring;
                        if self.transfer_payload_start_time.is_none() {
                            self.transfer_payload_start_time = Some(self.animation_time);
                        }
                    } else {
                        // Keep sender in waiting while still in connect/prep stages.
                        return;
                    }
                }
                if self.transfer_phase != TransferPhase::Transferring {
                    self.transfer_payload_start_time = Some(self.animation_time);
                    self.preparing_progress = 1.0;
                }
                self.transfer_phase = TransferPhase::Transferring;
                let prev_done = self.transfer_done_bytes.unwrap_or(0);
                let prev_total = self.transfer_total_bytes.unwrap_or(0);
                let next_done =
                    if self.selected_tool == SelectedTool::Sendme && self.view == AppView::Send {
                        done
                    } else {
                        done.max(prev_done)
                    };
                let mut next_total = total;
                if prev_total > 0 && next_total < prev_total {
                    next_total = prev_total;
                }
                if next_total < next_done {
                    next_total = next_done;
                }
                self.transfer_done_bytes = Some(next_done.min(next_total));
                if self.selected_tool != SelectedTool::Croc {
                    self.transfer_total_bytes = Some(next_total);
                }
                if next_total > 0 {
                    self.transfer_progress = (next_done as f32 / next_total as f32).clamp(0.0, 1.0);
                }
                self.update_derived_speed_from_done_bytes();
            }
        }

        if self.transfer_total_bytes.is_none()
            && self.transfer_phase == TransferPhase::Preparing
            && self.selected_tool == SelectedTool::Croc
        {
            if let Some(total) = parse_total_size_hint(line) {
                self.transfer_total_bytes = Some(total);
            }
        }

        if let Some(speed) = parse_speed_hint(line) {
            if self.transfer_phase == TransferPhase::Transferring {
                if self.selected_tool == SelectedTool::Sendme
                    && self.view == AppView::Receive
                    && !lower.contains("downloading")
                {
                    return;
                }
                self.push_speed_sample(speed);
            }
        }
        if sendme_like && self.view == AppView::Send && self.transfer_done_bytes.unwrap_or(0) > 0 {
            self.sendme_had_transfer = true;
        }
        
    }

    fn update_croc_received_text_from_log(&mut self, line: &str) -> bool {
        if let Some(text) = extract_croc_received_text(line) {
            if !text.is_empty() && self.croc_received_text.as_deref() != Some(text.as_str()) {
                self.croc_received_text = Some(text);
                if self.selected_tool == SelectedTool::Croc {
                    self.croc_text_popup_open = true;
                }
                self.croc_expect_text_payload = false;
            }
            return true;
        }
        if (self.selected_tool == SelectedTool::Croc
            || self.selected_tool == SelectedTool::EazySendme)
            && self.view == AppView::Receive
        {
            let lower = line.to_lowercase();
            if (lower.contains("receiv") && lower.contains("text"))
                || lower.contains("text message")
                || lower.contains("message received")
            {
                self.croc_expect_text_payload = true;
            }
            if self.croc_expect_text_payload && is_probable_croc_text_payload_line(line) {
                let text = line.trim().trim_matches('"').to_string();
                if !text.is_empty() && self.croc_received_text.as_deref() != Some(text.as_str()) {
                    self.croc_received_text = Some(text);
                    if self.selected_tool == SelectedTool::Croc {
                        self.croc_text_popup_open = true;
                    }
                    self.croc_expect_text_payload = false;
                    return true;
                }
            }
            if self.croc_received_text.is_none() {
                // Check current line first
                if let Some(text) = extract_croc_received_text(line) {
                    self.croc_received_text = Some(text);
                    if self.selected_tool == SelectedTool::Croc {
                        self.croc_text_popup_open = true;
                    }
                    self.croc_expect_text_payload = false;
                    return true;
                }
                // Then check log
                if let Some(text) = extract_croc_received_text_from_logs(&self.transfer_log) {
                    self.croc_received_text = Some(text);
                    if self.selected_tool == SelectedTool::Croc {
                        self.croc_text_popup_open = true;
                    }
                    self.croc_expect_text_payload = false;
                    return true;
                }
            }
        }
        false
    }

    /// Seconds between automatic croc update checks (after the on-open check).
    const CROC_UPDATE_INTERVAL_SECS: f64 = 6.0 * 3600.0;

    /// One-line self-update status for the home page (`None` when idle).
    fn croc_update_status_line(&self) -> Option<String> {
        match self.croc_update_phase {
            CrocUpdatePhase::Checking => Some("croc: checking for updates…".to_string()),
            CrocUpdatePhase::Updating => Some("croc: updating…".to_string()),
            CrocUpdatePhase::Idle => None,
        }
    }

    /// Refresh the cached croc `ToolStatus` so the home page engine card
    /// shows the new version immediately after an update/migration.
    fn refresh_croc_tool_status(&mut self) {
        let fresh = redetect_croc_status(self.bundled_croc.as_ref());
        if let Some(slot) = self.tool_statuses.iter_mut().find(|s| s.tool == Tool::Croc) {
            *slot = fresh;
        } else {
            self.tool_statuses.push(fresh);
        }
    }

    fn record_croc_update_check_time(&mut self) {
        self.croc_update_last_check_at = Some(self.animation_time);
    }

    /// Kick off `croc update --check` in a background thread. No-op while a
    /// transfer runs or another update phase is in flight. Wakes the UI when
    /// done: a fresh idle app schedules no frames, so without an explicit
    /// wakeup the result would sit in the channel uncollected indefinitely.
    fn start_croc_update_check(&mut self, ctx: &egui::Context) {
        if self.croc_update_phase != CrocUpdatePhase::Idle {
            return;
        }
        if self.transfer_state == TransferState::Running {
            return;
        }
        let Some(binary) = self.get_tool_binary(&Tool::Croc) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.croc_update_check_rx = Some(rx);
        self.croc_update_phase = CrocUpdatePhase::Checking;
        let wake = ctx.clone();
        thread::spawn(move || {
            let check = croc_update_check(&binary);
            let _ = tx.send(CrocUpdateCheckResult { check, binary });
            wake.request_repaint();
        });
    }

    fn start_croc_update_apply(
        &mut self,
        ctx: &egui::Context,
        binary: String,
        previous_version: Option<String>,
    ) {
        let (tx, rx) = mpsc::channel();
        self.croc_update_apply_rx = Some(rx);
        self.croc_update_phase = CrocUpdatePhase::Updating;
        let wake = ctx.clone();
        thread::spawn(move || {
            let outcome = croc_update_apply(&binary);
            let _ = tx.send(CrocUpdateApplyResult {
                outcome,
                binary,
                previous_version,
            });
            wake.request_repaint();
        });
    }

    /// One-time migration for cached binaries that predate `croc update`:
    /// re-download the latest managed release in the background, then
    /// re-detect so the home page version updates. Managed copy only — a
    /// legacy *system* croc is left alone (never overwrite foreign files).
    fn start_croc_legacy_migration(&mut self, ctx: &egui::Context, binary: &str) {
        if self.croc_update_phase != CrocUpdatePhase::Idle {
            return;
        }
        if self.transfer_state == TransferState::Running {
            return;
        }
        if !is_managed_croc_binary(binary) {
            self.transfer_log.push(
                "croc is system-managed and predates self-update; leaving it alone".to_string(),
            );
            return;
        }
        let (tx, rx) = mpsc::channel::<Option<PathBuf>>();
        self.croc_update_phase = CrocUpdatePhase::Updating;
        self.croc_legacy_migration_rx = Some(rx);
        let wake = ctx.clone();
        thread::spawn(move || {
            let path = refresh_managed_croc_to_latest();
            let _ = tx.send(path);
            wake.request_repaint();
        });
    }

    fn poll_croc_update(&mut self, ctx: &egui::Context) {
        // ── check results ──
        if let Some(rx) = self.croc_update_check_rx.take() {
            match rx.try_recv() {
                Ok(result) => {
                    self.croc_update_check_rx = None;
                    self.record_croc_update_check_time();
                    match result.check {
                        Ok(CrocUpdateCheck::Available { current, latest, .. }) => {
                            self.transfer_log.push(format!(
                                "croc update available ({current} → {latest}), updating…"
                            ));
                            self.show_toast(
                                format!("croc update available ({current} → {latest}), updating…"),
                                WARNING,
                            );
                            let prev = croc_version_string(&result.binary);
                            self.start_croc_update_apply(ctx, result.binary, prev);
                        }
                        Ok(CrocUpdateCheck::UpToDate { .. }) => {
                            self.croc_update_phase = CrocUpdatePhase::Idle;
                            self.refresh_croc_tool_status();
                        }
                        Ok(CrocUpdateCheck::NotWritable { .. }) => {
                            // Package-managed croc: never overwrite, just
                            // refresh the displayed version.
                            self.croc_update_phase = CrocUpdatePhase::Idle;
                            self.refresh_croc_tool_status();
                        }
                        Ok(CrocUpdateCheck::Unsupported { .. }) => {
                            // Legacy binary without `update`: migrate via a
                            // managed re-download to the latest release.
                            self.croc_update_phase = CrocUpdatePhase::Idle;
                            let binary = result.binary.clone();
                            self.start_croc_legacy_migration(ctx, &binary);
                        }
                        Ok(CrocUpdateCheck::Unknown { raw }) => {
                            // Unrecognized output (e.g. croc reworded it):
                            // stay quiet in UI but leave one log line so the
                            // drift is visible instead of silent stagnation.
                            self.croc_update_phase = CrocUpdatePhase::Idle;
                            let snippet: String = raw.chars().take(120).collect();
                            self.transfer_log.push(format!(
                                "croc update check returned unrecognized output: {snippet}"
                            ));
                        }
                        Err(e) => {
                            self.croc_update_phase = CrocUpdatePhase::Idle;
                            self.transfer_log.push(format!("croc update check failed: {e}"));
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.croc_update_check_rx = Some(rx);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.croc_update_phase = CrocUpdatePhase::Idle;
                }
            }
        }

        // ── legacy migration results ──
        if let Some(rx) = self.croc_legacy_migration_rx.take() {
            match rx.try_recv() {
                Ok(path) => {
                    self.croc_legacy_migration_rx = None;
                    if let Some(path) = path {
                        self.bundled_croc = Some(path);
                        self.refresh_croc_tool_status();
                        let ver = self
                            .tool_statuses
                            .iter()
                            .find(|s| s.tool == Tool::Croc)
                            .and_then(|s| s.version.clone())
                            .unwrap_or_default();
                        self.transfer_log.push(format!("croc migrated to latest ({ver})"));
                        self.show_toast(format!("croc updated ({ver})"), SUCCESS);
                    } else {
                        self.transfer_log.push("croc migration download failed".to_string());
                    }
                    self.croc_update_phase = CrocUpdatePhase::Idle;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.croc_legacy_migration_rx = Some(rx);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.croc_update_phase = CrocUpdatePhase::Idle;
                }
            }
        }

        // ── apply results ──
        if let Some(rx) = self.croc_update_apply_rx.take() {
            match rx.try_recv() {
                Ok(result) => {
                    self.croc_update_apply_rx = None;
                    self.croc_update_phase = CrocUpdatePhase::Idle;
                    match result.outcome {
                        Ok(outcome) if outcome.updated => {
                            self.refresh_croc_tool_status();
                            let new_ver = outcome.new_version.clone().or_else(|| {
                                croc_version_string(&result.binary)
                            });
                            let msg = match (result.previous_version, new_ver) {
                                (Some(prev), Some(new)) if prev != new => {
                                    format!("croc updated ({prev} → {new})")
                                }
                                (_, Some(new)) => format!("croc updated ({new})"),
                                _ => "croc updated".to_string(),
                            };
                            self.transfer_log.push(msg.clone());
                            self.show_toast(msg, SUCCESS);
                        }
                        Ok(outcome) if outcome.already_up_to_date => {
                            self.refresh_croc_tool_status();
                        }
                        Ok(outcome) if outcome.not_writable => {
                            // Not an error: leave the system binary alone.
                            self.refresh_croc_tool_status();
                        }
                        Ok(outcome) if outcome.needs_redownload => {
                            // Self-update refused by the environment (verified:
                            // always on Windows, unregistered installs on
                            // Unix). Managed copy: re-download latest instead;
                            // foreign binaries are left alone.
                            if is_managed_croc_binary(&result.binary) {
                                self.transfer_log.push(
                                    "croc self-update refused by platform, migrating via re-download…"
                                        .to_string(),
                                );
                                let binary = result.binary.clone();
                                self.start_croc_legacy_migration(ctx, &binary);
                            } else {
                                self.transfer_log.push(
                                    "croc self-update refused; leaving system binary alone"
                                        .to_string(),
                                );
                                self.refresh_croc_tool_status();
                            }
                        }
                        Ok(_) => {
                            // Declined/cancelled or no-op: stay silent-ish.
                            self.refresh_croc_tool_status();
                        }
                        Err(e) => {
                            self.transfer_log.push(format!("croc update failed: {e}"));
                            self.show_toast(format!("croc update failed: {e}"), WARNING);
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.croc_update_apply_rx = Some(rx);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.croc_update_phase = CrocUpdatePhase::Idle;
                }
            }
        }
    }

    /// Decide whether a periodic (or startup) check is due. Called every frame;
    /// cheap no-op most of the time.
    fn maybe_trigger_croc_update_check(&mut self, ctx: &egui::Context) {
        if self.croc_update_phase != CrocUpdatePhase::Idle {
            return;
        }
        if self.croc_update_check_rx.is_some()
            || self.croc_update_apply_rx.is_some()
            || self.croc_legacy_migration_rx.is_some()
        {
            return;
        }
        if self.transfer_state == TransferState::Running {
            return;
        }
        if !self
            .tool_statuses
            .iter()
            .any(|s| s.tool == Tool::Croc && s.available)
        {
            return;
        }
        // First run of the app process: check immediately on open.
        if !self.croc_update_startup_check_done {
            self.croc_update_startup_check_done = true;
            self.start_croc_update_check(ctx);
            return;
        }
        // Afterwards: every 6 hours.
        if let Some(last) = self.croc_update_last_check_at {
            if self.animation_time - last >= Self::CROC_UPDATE_INTERVAL_SECS {
                self.start_croc_update_check(ctx);
            }
        }
    }

    fn poll_transfer(&mut self) {
        // Take the receiver out to satisfy borrow checker
        if let Some(rx) = self.transfer_rx.take() {
            let mut processed = 0usize;
            let mut restart_sendme_serve = false;
            let mut transfer_rx_disconnected = false;
            let max_msgs_per_frame = if self.selected_tool == SelectedTool::Sendme
                || self.selected_tool == SelectedTool::EazySendme
            {
                600
            } else {
                800
            };
            loop {
                if processed >= max_msgs_per_frame {
                    break;
                }
                match rx.try_recv() {
                    Ok(msg) => {
                        processed += 1;
                        match msg {
                            TransferMsg::Output(line) => {
                                let lower = line.to_lowercase();
                                // Data-path route from croc's `Sending (...)`
                                // direction line. First wins; Croc mode only
                                // (the Eazy ticket leg below must not label
                                // the sendme data leg).
                                if self.selected_tool == SelectedTool::Croc
                                    && self.croc_route.is_none()
                                {
                                    if let Some(route) = parse_croc_direction(&line) {
                                        self.croc_route = Some(route);
                                        self.transfer_log.push(format!(
                                            "Route: {}",
                                            route.label()
                                        ));
                                    }
                                }
                                if self.selected_tool == SelectedTool::Sendme
                                    && self.view == AppView::Receive
                                    && self.transfer_phase != TransferPhase::Transferring
                                    && (lower.contains("[2/4] downloading")
                                        || lower.contains("[3/4] downloading")
                                        || lower.contains("[4/4] writing files to disk")
                                        || lower.contains("[4/4] writing..."))
                                {
                                    self.transfer_phase = TransferPhase::Transferring;
                                    self.preparing_progress = 1.0;
                                    if self.transfer_payload_start_time.is_none() {
                                        self.transfer_payload_start_time = Some(self.animation_time);
                                    }
                                }
                                if self.eazy_local_check_started_at.is_some()
                                    && (lower.contains("[local] blobs complete locally, exporting")
                                        || lower.contains("[4/4] writing files to disk")
                                        || lower.contains("[4/4] writing..."))
                                {
                                    self.eazy_local_check_started_at = None;
                                }
                                let sendme_counter_activity =
                                    parse_sendme_r_counter(&line).is_some();
                                if self.transfer_state == TransferState::Running {
                                    self.update_transfer_metrics_from_log(&line);
                                }
                                let mut skip_log_line = false;
                                if (self.selected_tool == SelectedTool::Croc
                                    || self.selected_tool == SelectedTool::EazySendme)
                                    && self.view == AppView::Receive
                                {
                                    skip_log_line = self.update_croc_received_text_from_log(&line);
                                }
                                if self.view == AppView::Send
                                    && self.transfer_state == TransferState::Running
                                {
                                    if (self.selected_tool == SelectedTool::Sendme
                                        || self.selected_tool == SelectedTool::EazySendme)
                                        && lower.contains("sendme receive ")
                                    {
                                        if self.transfer_phase == TransferPhase::Preparing {
                                            self.transfer_phase = TransferPhase::WaitingForReceiver;
                                        }
                                    }
                                    if self.selected_tool == SelectedTool::Croc
                                        && lower.contains("code copied to clipboard")
                                        && self.transfer_phase == TransferPhase::Preparing
                                    {
                                        self.transfer_phase = TransferPhase::WaitingForReceiver;
                                    }
                                    if self.selected_tool == SelectedTool::Croc
                                        && self.transfer_phase == TransferPhase::WaitingForReceiver
                                        && parse_croc_direction(&line).is_some()
                                    {
                                        self.transfer_phase = TransferPhase::Transferring;
                                        self.sendme_had_transfer = true;
                                        if self.transfer_payload_start_time.is_none() {
                                            self.transfer_payload_start_time =
                                                Some(self.animation_time);
                                        }
                                    }
                                }
                                if !skip_log_line
                                    && !(self.selected_tool == SelectedTool::Croc
                                        && (parse_croc_bar_frame(&line).is_some()
                                            || (lower.starts_with("hashing ")
                                                && line.contains('%')
                                                && line.contains('|')))
                                    )
                                    && !(matches!(
                                        self.selected_tool,
                                        SelectedTool::Sendme | SelectedTool::EazySendme
                                    ) && (is_sendme_sender_request_line(&line)
                                        || lower.contains("downloading ...")
                                        || lower.contains("uploading ...")
                                        || lower.contains("writing... ")
                                        || (line.starts_with("n ") && line.contains(" r "))))
                                    && (self.selected_tool == SelectedTool::Sendme
                                        || self.selected_tool == SelectedTool::EazySendme
                                        || self.transfer_log.last() != Some(&line))
                                {
                                    self.transfer_log.push(line);
                                    if self.transfer_log.len() > 500 {
                                        self.transfer_log.drain(0..100);
                                    }
                                }
                                if self.selected_tool == SelectedTool::Sendme
                                    || self.selected_tool == SelectedTool::EazySendme
                                {
                                    let sendme_sender = self.view == AppView::Send;
                                    let sendme_end_signal = lower.contains("client disconnected")
                                        || lower.contains("finished sending")
                                        || lower.contains("transfer complete")
                                        || lower.contains("peer disconnected");
                                    let sendme_waiting_signal = lower
                                        .contains("waiting for incoming transfer")
                                        || lower.contains("waiting for incoming")
                                        || lower.contains("waiting for receiver");
                                    let sendme_cycle_finished = sendme_end_signal
                                        || (sendme_sender
                                            && self.sendme_had_transfer
                                            && sendme_waiting_signal);

                                    if !sendme_sender && sendme_cycle_finished {
                                        // Receiver side still uses log hints for UX; sender-side
                                        // cycle transitions are driven by explicit backend events.
                                    }
                                    // Transition to Transferring only on actual connection acceptance or data flow
                                    if lower.contains("disco_in{endpoint=")
                                        || lower.contains("new direct addr for endpoint")
                                        || lower.contains("new connection type")
                                        || lower.contains("typ=direct")
                                        || lower.contains("typ=mixed")
                                        || lower.contains("connect{")
                                        || lower.contains("add_endpoint_addr")
                                    {
                                        self.sendme_peer_connected = true;
                                        self.sendme_waiting_after_cycle = false;
                                        self.sendme_last_activity = Some(self.animation_time);
                                    } else if self.sendme_peer_connected
                                        && (lower.contains(" r ")
                                            || lower.contains("sending")
                                            || lower.contains("download")
                                            || lower.contains("upload"))
                                    {
                                        self.sendme_last_activity = Some(self.animation_time);
                                        if self.view == AppView::Send
                                            && self.transfer_phase
                                                == TransferPhase::WaitingForReceiver
                                        {
                                            self.transfer_phase = TransferPhase::Transferring;
                                            self.sendme_had_transfer = true;
                                            if self.transfer_payload_start_time.is_none() {
                                                self.transfer_payload_start_time =
                                                    Some(self.animation_time);
                                            }
                                        }
                                    }
                                    if self.view == AppView::Send
                                        && (sendme_counter_activity
                                            || ((lower.contains("[3/4]")
                                                || lower.contains("[4/4]"))
                                                && (lower.contains("uploading")
                                                    || lower.contains("downloading"))))
                                        && (!self.sendme_waiting_after_cycle
                                            || self.sendme_peer_connected)
                                    {
                                        self.sendme_had_transfer = true;
                                        self.sendme_waiting_after_cycle = false;
                                        self.sendme_last_activity = Some(self.animation_time);
                                        if self.transfer_phase != TransferPhase::Transferring {
                                            self.transfer_phase = TransferPhase::Transferring;
                                            if self.transfer_payload_start_time.is_none() {
                                                self.transfer_payload_start_time =
                                                    Some(self.animation_time);
                                            }
                                        }
                                    }
                                }
                            }
                            TransferMsg::Progress(p) => {
                                if self.eazy_local_check_started_at.is_some() && p > 0.0 {
                                    self.eazy_local_check_started_at = None;
                                }
                                let sendme_broadcast_sender = (self.selected_tool
                                    == SelectedTool::Sendme
                                    || self.selected_tool == SelectedTool::EazySendme)
                                    && self.view == AppView::Send
                                    && self.transfer_state == TransferState::Running
                                    && !self.sendme_one_shot;
                                if sendme_broadcast_sender
                                    && self.sendme_waiting_after_cycle
                                    && !self.sendme_peer_connected
                                {
                                    continue;
                                }
                                self.transfer_progress = self.transfer_progress.max(p);
                                if self.transfer_phase == TransferPhase::Transferring {
                                    if let Some(total) = self.transfer_total_bytes {
                                        if total > 0 {
                                            let derived_done =
                                                ((p.clamp(0.0, 1.0) as f64) * total as f64) as u64;
                                            let next_done = self
                                                .transfer_done_bytes
                                                .unwrap_or(0)
                                                .max(derived_done);
                                            self.transfer_done_bytes = Some(next_done.min(total));
                                            self.update_derived_speed_from_done_bytes();
                                        }
                                    }
                                }
                            }
                            TransferMsg::Code(code) => {
                                let should_replace = if is_masked_croc_code(&code) {
                                    self.transfer_code.is_none()
                                } else {
                                    true
                                };
                                if should_replace {
                                    // Serve mode ticket detected — start sharing it via Croc
                                    if self.selected_tool == SelectedTool::EazySendme
                                        && self.view == AppView::Send
                                    {
                                        if self.eazysendme_ticket.is_none() {
                                            self.transfer_phase = TransferPhase::EazySharingTicket;
                                            self.eazysendme_ticket = Some(code.clone());
                                            self.transfer_log.push(format!(
                                                "Ticket generated: {}. Sharing via Croc...",
                                                code
                                            ));

                                            // Start croc_send
                                            if let Some(binary) = self.get_tool_binary(&Tool::Croc)
                                            {
                                                let opts = CrocSendOptions {
                                                    paths: Vec::new(),
                                                    custom_code: if !self
                                                        .eazysendme_custom_code
                                                        .is_empty()
                                                    {
                                                        Some(self.eazysendme_custom_code.clone())
                                                    } else {
                                                        None
                                                    },
                                                    text_mode: true,
                                                    text_value: Some(code.clone()),
                                                };
                                                let (croc_rx, croc_handle) =
                                                    croc_send(opts, &binary);
                                                self.eazysendme_croc_handle = Some(croc_handle);
                                                self.eazysendme_croc_rx = Some(croc_rx);
                                            }
                                        }
                                        // self.transfer_code = Some(code); // Do NOT show the sendme ticket as the code
                                    } else if self.selected_tool == SelectedTool::EazySendme
                                        && self.view == AppView::Receive
                                    {
                                        // Keep Eazy receive code field owned by Croc flow.
                                    } else {
                                        // Normal Sendme or Croc: show the code/ticket we got
                                        self.transfer_code = Some(code);
                                    }
                                }

                                if self.selected_tool == SelectedTool::Sendme
                                    && self.view == AppView::Send
                                    && self.transfer_phase == TransferPhase::Preparing
                                {
                                    self.transfer_phase = TransferPhase::WaitingForReceiver;
                                    self.sendme_waiting_after_cycle = false;
                                    self.sendme_active_transfers = 0;
                                }
                            }
                            TransferMsg::WaitingForReceiver => {
                                let sendme_sender = (self.selected_tool == SelectedTool::Sendme
                                    || self.selected_tool == SelectedTool::EazySendme)
                                    && self.view == AppView::Send
                                    && self.transfer_state == TransferState::Running;
                                if sendme_sender {
                                    if !self.sendme_one_shot
                                        && (self.sendme_had_transfer
                                            || self.sendme_waiting_after_cycle)
                                    {
                                        // Keep repeat-cycle waiting state sticky so late events
                                        // from the previous cycle cannot repopulate stale progress.
                                        self.mark_sendme_sender_waiting();
                                    } else {
                                        self.transfer_phase = TransferPhase::WaitingForReceiver;
                                        self.sendme_waiting_after_cycle = false;
                                        self.sendme_active_transfers = 0;
                                    }
                                }
                            }
                            TransferMsg::SenderTransferActivity => {
                                if (self.selected_tool == SelectedTool::Sendme
                                    || self.selected_tool == SelectedTool::EazySendme)
                                    && self.view == AppView::Send
                                    && self.transfer_state == TransferState::Running
                                {
                                    if self.sendme_waiting_after_cycle
                                        && !self.sendme_peer_connected
                                    {
                                        continue;
                                    }
                                    self.sendme_peer_connected = true;
                                    self.sendme_had_transfer = true;
                                    if self.sendme_active_transfers == 0 {
                                        self.sendme_active_transfers = 1;
                                    }
                                    self.sendme_waiting_after_cycle = false;
                                    self.sendme_last_activity = Some(self.animation_time);
                                    if self.transfer_phase != TransferPhase::Transferring {
                                        self.transfer_phase = TransferPhase::Transferring;
                                        if self.transfer_payload_start_time.is_none() {
                                            self.transfer_payload_start_time =
                                                Some(self.animation_time);
                                        }
                                    }
                                }
                            }
                            TransferMsg::ActiveTransfers(count) => {
                                let sendme_broadcast_sender = (self.selected_tool
                                    == SelectedTool::Sendme
                                    || self.selected_tool == SelectedTool::EazySendme)
                                    && self.view == AppView::Send
                                    && self.transfer_state == TransferState::Running
                                    && !self.sendme_one_shot;
                                if sendme_broadcast_sender {
                                    self.sendme_active_transfers = count;
                                    if count > 0 {
                                        self.sendme_peer_connected = true;
                                        self.sendme_had_transfer = true;
                                        self.sendme_waiting_after_cycle = false;
                                        self.sendme_last_activity = Some(self.animation_time);
                                        if self.transfer_phase == TransferPhase::WaitingForReceiver
                                        {
                                            self.transfer_phase = TransferPhase::Transferring;
                                            if self.transfer_payload_start_time.is_none() {
                                                self.transfer_payload_start_time =
                                                    Some(self.animation_time);
                                            }
                                        }
                                    } else {
                                        self.sendme_peer_connected = false;
                                    }
                                }
                            }
                            TransferMsg::Completed => {
                                self.eazy_local_check_started_at = None;
                                self.transfer_end_time = Some(self.animation_time);
                                if self.selected_tool == SelectedTool::Croc
                                    && self.view == AppView::Send
                                    && self.transfer_code.is_none()
                                {
                                    self.fail_transfer(
                                        "Croc send ended before starting. Check options and retry."
                                            .to_string(),
                                    );
                                } else {
                                    if self.selected_tool == SelectedTool::EazySendme
                                        && self.view == AppView::Receive
                                        && self.eazysendme_ticket.is_none()
                                        && self.transfer_rx.is_none()
                                    {
                                        // Receiver side: Croc finished receiving the ticket
                                        if self.croc_received_text.is_none() {
                                            // Fallback: If Croc finished but we already had this ticket in cache (unlikely but possible during retries), reuse it.
                                            let code_key = self.receive_code.trim().to_string();
                                            if let Some(entry) = self.eazysendme_code_ticket_map.get(&code_key) {
                                                self.croc_received_text = Some(entry.ticket.clone());
                                            }
                                        }

                                        if let Some(ticket) = self.croc_received_text.clone() {
                                            self.transfer_log.push(
                                                "Ticket received via Croc. Starting Sendme..."
                                                    .to_string(),
                                            );
                                            let normalized_ticket =
                                                normalize_sendme_ticket(&ticket).unwrap_or(ticket);
                                            self.eazysendme_ticket =
                                                Some(normalized_ticket.clone()); // Start native sendme_receive
                                            // Record code→ticket mapping so local-blob retry
                                            // works even after reset_transfer clears the ticket.
                                            let code_key = self.receive_code.trim().to_string();
                                            if !code_key.is_empty() {
                                                let now = std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_secs();

                                                 self.eazysendme_code_ticket_map
                                                     .insert(code_key, EazyCacheEntry {
                                                         ticket: normalized_ticket.clone(),
                                                         timestamp: now,
                                                     });
                                                self.persist_user_settings();
                                            }
                                            let opts = SendmeReceiveOptions {
                                                ticket: normalized_ticket,
                                                output_dir: self.receive_output_dir.clone(),
                                                blob_dir: self.effective_sendme_blob_dir(),
                                                overwrite: true, // Smart overwrite is now the default
                                            };
                                            let sendme_binary = self.get_tool_binary(&Tool::Sendme).unwrap_or_default();
                                            let (new_rx, new_handle) = sendme_receive(opts, &sendme_binary);
                                            self.transfer_rx = Some(new_rx);
                                            self.transfer_handle = Some(new_handle);
                                            self.transfer_phase = TransferPhase::Preparing;
                                            self.transfer_start_time = Some(self.animation_time);
                                            return; // Exit poll_transfer, will resume next frame with new_rx
                                        } else {
                                            self.fail_transfer(
                                                "Croc finished but no ticket found".to_string(),
                                            );
                                        }
                                    } else {
                                        self.transfer_state = TransferState::Completed;
                                        self.eazy_retry_count = 0;
                                        self.eazy_next_retry_time = None;
                                        self.transfer_progress = 1.0;
                                        self.preparing_progress = 1.0;
                                        if let Some(total) = self.transfer_total_bytes {
                                            self.transfer_done_bytes = Some(total);
                                        }
                                        // Standalone Sendme receive does not keep local blob cache
                                        // after a successful export.
                                        if self.selected_tool == SelectedTool::Sendme
                                            && self.view == AppView::Receive
                                        {
                                            if let Some(ticket) = self.transfer_code.clone() {
                                                crate::backend::cleanup_sendme_receive_artifacts_for_ticket(
                                                    &ticket,
                                                    self.effective_sendme_blob_dir().as_deref(),
                                                );
                                            }
                                        }
                                         
                                                                                 // Once extraction completes successfully, remove the cache artifact immediately so it doesn't leave orphaned blob dirs.
                                         let code_key = self.receive_code.trim().to_string();
                                         if let Some(entry) = self.eazysendme_code_ticket_map.get(&code_key).cloned() {
                                             if let Some(hash) = native_ticket_to_hex_hash(&entry.ticket) {
                                                 self.cleanup_eazy_cache_by_hash(&hash, true);
                                             } else {
                                                 // Fallback
                                                 self.eazysendme_code_ticket_map.remove(&code_key);
                                                 crate::backend::cleanup_sendme_receive_artifacts_for_ticket(
                                                     &entry.ticket,
                                                     self.effective_sendme_blob_dir().as_deref(),
                                                 );
                                                 self.persist_user_settings();
                                              }
                                         }
                                    }
                                }
                            }
                            TransferMsg::PeerDisconnected => {
                                if self.transfer_state == TransferState::Completed {
                                    continue;
                                }
                                self.transfer_log.push("Peer disconnected".to_string());
                                if (self.selected_tool == SelectedTool::Sendme
                                    || self.selected_tool == SelectedTool::EazySendme)
                                    && self.sendme_one_shot
                                {
                                    let done = self.transfer_done_bytes.unwrap_or(0);
                                    let total = self.transfer_total_bytes.unwrap_or(0);
                                    let has_payload = done > 0 || self.sendme_had_transfer;
                                    let complete = self.sendme_sender_payload_complete
                                        || (total > 0 && done >= total);
                                    if !has_payload {
                                        self.fail_transfer(
                                            "Peer disconnected before transfer started".to_string(),
                                        );
                                    } else if complete {
                                        self.mark_sendme_one_shot_completed();
                                    } else {
                                        self.fail_transfer(
                                            "Transfer was not completed (peer disconnected early)"
                                                .to_string(),
                                        );
                                    }
                                } else {
                                    // Keep running, maybe show a toast
                                    self.show_toast(
                                        "Peer finished downloading".to_string(),
                                        SUCCESS,
                                    );
                                    if self.selected_tool == SelectedTool::EazySendme {
                                        // If our shared ticket was received via Croc, nothing more to do for Croc
                                        if self.transfer_phase == TransferPhase::EazySharingTicket {
                                            self.transfer_phase = TransferPhase::EazyWaitingForPeer;
                                            self.transfer_log.push("Ticket shared via Croc. Waiting for Sendme transfer...".to_string());
                                            // Croc process ends here, but we stay in Running (or EazyWaitingForPeer)
                                            // because Sendme is still serving.
                                        }
                                    }
                                    if self.selected_tool == SelectedTool::Sendme
                                        && self.view == AppView::Send
                                        && !self.sendme_one_shot
                                    {
                                        self.mark_sendme_sender_waiting();
                                    } else {
                                        self.transfer_progress = 0.0;
                                        self.transfer_phase = match (
                                            self.selected_tool,
                                            self.view,
                                            self.sendme_one_shot,
                                        ) {
                                            (SelectedTool::EazySendme, AppView::Send, _) => {
                                                TransferPhase::EazyWaitingForPeer
                                            }
                                            _ => TransferPhase::Preparing,
                                        };
                                        self.preparing_progress = 0.0;
                                        self.transfer_payload_start_time = None;
                                        self.transfer_done_bytes = None;
                                        self.transfer_total_bytes = None;
                                        self.sendme_had_transfer = false;
                                        self.sendme_waiting_after_cycle = false;
                                        self.sendme_active_transfers = 0;
                                    }
                                }
                            }
                            TransferMsg::Error(e) => {
                                self.eazy_local_check_started_at = None;
                                if self.transfer_state == TransferState::Completed {
                                    // Ignore late process-shutdown errors after completion.
                                } else if e == "Transfer cancelled" {
                                    // Keep explicit completion from caller paths; otherwise stay idle-ish failed.
                                    if self.transfer_state != TransferState::Completed {
                                        self.fail_transfer("Transfer cancelled".to_string());
                                    }
                                } else if e == "Send session ended before transfer completed"
                                    && (self.selected_tool == SelectedTool::Sendme
                                        || self.selected_tool == SelectedTool::EazySendme)
                                    && self.sendme_one_shot
                                    && (self.sendme_had_transfer
                                        || self.transfer_done_bytes.unwrap_or(0) > 0)
                                {
                                    let done = self.transfer_done_bytes.unwrap_or(0);
                                    let total = self.transfer_total_bytes.unwrap_or(0);
                                    let has_payload = done > 0 || self.sendme_had_transfer;
                                    let complete = self.sendme_sender_payload_complete
                                        || (total > 0 && done >= total);
                                    if !has_payload {
                                        self.fail_transfer(
                                            "Send session ended before payload started".to_string(),
                                        );
                                    } else if complete {
                                        self.mark_sendme_one_shot_completed();
                                    } else {
                                        self.fail_transfer(
                                            "Transfer was not completed (session ended early)"
                                                .to_string(),
                                        );
                                    }
                                } else if e == "Send session ended before transfer completed"
                                    && (self.selected_tool == SelectedTool::Sendme
                                        || self.selected_tool == SelectedTool::EazySendme)
                                    && !self.sendme_one_shot
                                {
                                    // Serve mode: this session can expire while idle; immediately
                                    // start a fresh sendme process instead of failing the UI.
                                    restart_sendme_serve = true;
                                    break;
                                } else if e == "Sendme transfer failed on sender side"
                                    && (self.selected_tool == SelectedTool::Sendme
                                        || self.selected_tool == SelectedTool::EazySendme)
                                    && !self.sendme_one_shot
                                {
                                    // Serve mode: non-fatal per-cycle sender failure, restart cleanly.
                                    restart_sendme_serve = true;
                                    break;
                                } else if (self.selected_tool == SelectedTool::Croc
                                    || self.selected_tool == SelectedTool::EazySendme)
                                    && self
                                        .transfer_log
                                        .iter()
                                        .rev()
                                        .take(50)
                                        .any(|l| Self::is_relay_dns_failure_line(l))
                                {
                                    // Raw croc failure is only a generic process-exit
                                    // message; the log holds the real cause, so headline
                                    // it as a DNS/VPN hint instead.
                                    self.fail_transfer(
                                        "Couldn't reach the relay: DNS lookup failed. Check your DNS or VPN connection and retry (details in the log below)."
                                            .to_string(),
                                    );
                                } else {
                                    self.fail_transfer(e);
                                }
                            }
                            TransferMsg::Started => {
                                self.transfer_state = TransferState::Running;
                                self.transfer_start_time = Some(self.animation_time);
                                self.transfer_phase = if self.selected_tool == SelectedTool::Sendme
                                    && self.view == AppView::Receive
                                {
                                    // Native receive can start emitting useful transfer/export
                                    // signals immediately; keep it out of "Preparing" stalls.
                                    TransferPhase::Transferring
                                } else {
                                    TransferPhase::Preparing
                                };
                                self.preparing_progress = if self.transfer_phase == TransferPhase::Transferring {
                                    1.0
                                } else {
                                    0.0
                                };
                                self.transfer_payload_start_time = if self.transfer_phase == TransferPhase::Transferring {
                                    Some(self.animation_time)
                                } else {
                                    None
                                };
                                self.transfer_end_time = None;
                                self.croc_route = None;
                                self.transfer_speed_bps = None;
                                self.transfer_speed_samples.clear();
                                self.transfer_done_bytes = None;
                                self.sendme_had_transfer = false;
                                self.sendme_waiting_after_cycle = false;
                                self.sendme_active_transfers = 0;
                            }
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        transfer_rx_disconnected = true;
                        break;
                    }
                }
            }
            if restart_sendme_serve {
                self.start_send(true);
            } else if transfer_rx_disconnected {
                if self.transfer_state == TransferState::Running {
                    self.fail_transfer(
                        "Transfer process ended unexpectedly. Please retry.".to_string(),
                    );
                }
            } else {
                // Put it back
                self.transfer_rx = Some(rx);
            }
        }

        // Poll eazysendme_croc_rx if present to get the code
        if let Some(rx) = self.eazysendme_croc_rx.take() {
            let mut processed = 0;
            loop {
                if processed > 100 {
                    break;
                }
                match rx.try_recv() {
                    Ok(msg) => {
                        processed += 1;
                        match msg {
                            TransferMsg::Output(line) => {
                                let skip = self.update_croc_received_text_from_log(&line);
                                if !skip {
                                    self.transfer_log.push(format!("[Croc] {}", line));
                                    if self.transfer_log.len() > 500 {
                                        self.transfer_log.drain(0..100);
                                    }
                                }
                            }
                            TransferMsg::Code(code) => {
                                // This is the short code we want to show!
                                self.transfer_code = Some(code);
                                if self.selected_tool == SelectedTool::EazySendme
                                    && self.view == AppView::Send
                                    && self.transfer_phase == TransferPhase::EazySharingTicket
                                {
                                    self.transfer_phase = TransferPhase::EazyWaitingForPeer;
                                    self.transfer_log.push(
                                        "Croc code generated. Share this code with receiver."
                                            .to_string(),
                                    );
                                }
                            }
                            TransferMsg::Error(e) => {
                                self.show_toast(format!("Croc error: {}", e), ERROR);
                            }
                            TransferMsg::Completed => {
                                self.transfer_log
                                    .push("Croc finished sharing ticket.".to_string());
                                if self.selected_tool == SelectedTool::EazySendme
                                    && self.view == AppView::Send
                                    && self.transfer_phase == TransferPhase::EazySharingTicket
                                {
                                    self.transfer_phase = TransferPhase::EazyWaitingForPeer;
                                    self.transfer_log.push(
                                        "Waiting for receiver to connect via Sendme...".to_string(),
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(_) => break,
                }
            }
            self.eazysendme_croc_rx = Some(rx);
        }
    }

    fn poll_size_updates(&mut self) {
        let Some(rx) = self.size_update_rx.as_ref() else {
            return;
        };
        for _ in 0..32 {
            match rx.try_recv() {
                Ok((path, size)) => {
                    if let Some(item) = self.send_items.iter_mut().find(|i| i.path == path) {
                        item.size = Some(size);
                    }
                }
                Err(_) => break,
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        if self.transfer_state == TransferState::Running {
            self.drag_hover = false;
            return;
        }
        self.drag_hover = false;
        ctx.input(|i| {
            if !i.raw.hovered_files.is_empty() {
                self.drag_hover = true;
            }
        });

        let (mut dropped, fallback_payloads): (Vec<PathBuf>, Vec<String>) = ctx.input(|i| {
            let mut direct_paths = Vec::new();
            let mut payloads = Vec::new();

            for file in &i.raw.dropped_files {
                if let Some(path) = file.path.clone() {
                    direct_paths.push(path);
                } else if !file.name.trim().is_empty() {
                    payloads.push(file.name.clone());
                }
            }

            for event in &i.events {
                let text = match event {
                    egui::Event::Paste(s) | egui::Event::Text(s) => s,
                    _ => continue,
                };
                if looks_like_file_drop_payload(text) {
                    payloads.push(text.clone());
                }
            }

            (direct_paths, payloads)
        });

        if dropped.is_empty() {
            for payload in fallback_payloads {
                dropped.extend(parse_file_drop_payload(&payload));
            }
        }

        if !dropped.is_empty() {
            dropped.sort();
            dropped.dedup();
            if self.view != AppView::Send {
                self.view = AppView::Send;
            }
            for path in dropped {
                self.add_path(path);
            }
            let count = self.send_items.len();
            self.show_toast(
                format!("{} item{} ready", count, if count == 1 { "" } else { "s" }),
                SUCCESS,
            );
        }
    }

    fn show_toast(&mut self, msg: String, color: Color32) {
        self.toast_msg = Some((msg, 0.0, color));
    }

    fn start_send(&mut self, is_auto: bool) {
        // Never overlap a croc self-update: the binary file may be replaced
        // mid-transfer (fatal on Windows file locks, version skew elsewhere).
        // Sendme legs never touch the croc binary, so they are exempt.
        // Updates take seconds; manual attempts get a toast, Eazy auto-retries
        // re-arm briefly instead of being swallowed.
        if matches!(
            self.selected_tool,
            SelectedTool::Croc | SelectedTool::EazySendme
        ) && self.croc_update_phase != CrocUpdatePhase::Idle
        {
            if is_auto && self.selected_tool == SelectedTool::EazySendme {
                self.eazy_next_retry_time = Some(self.animation_time + 5.0);
            } else if !is_auto {
                self.show_toast(
                    "croc is updating itself, try again in a moment".to_string(),
                    WARNING,
                );
            }
            return;
        }
        if !is_auto {
            self.eazy_retry_count = 0;
            self.eazy_next_retry_time = None;
        }
        let croc_text_mode = self.selected_tool == SelectedTool::Croc && self.croc_text_mode;
        if croc_text_mode && !self.send_items.is_empty() {
            self.show_toast(
                "Croc text mode cannot send files/folders at the same time".to_string(),
                WARNING,
            );
            return;
        }
        if !croc_text_mode && self.send_items.is_empty() {
            self.show_toast("Add files first".to_string(), WARNING);
            return;
        }

        self.reset_transfer();
        let known_total = self.known_total_size();

        let binary = match self.selected_tool {
            SelectedTool::Croc => match self.get_tool_binary(&Tool::Croc) {
                Some(b) => b,
                None => {
                    self.show_toast("Croc not found".to_string(), ERROR);
                    return;
                }
            },
            SelectedTool::Sendme | SelectedTool::EazySendme => {
                self.get_tool_binary(&Tool::Sendme).unwrap_or_default()
            }
        };

        match self.selected_tool {
            SelectedTool::Croc => {
                let wants_custom = self.croc_use_custom_code;
                let custom_code = if wants_custom {
                    let trimmed = self.croc_custom_code.trim();
                    if trimmed.is_empty() {
                        self.show_toast(
                            "Enter a custom code or switch to random code".to_string(),
                            WARNING,
                        );
                        return;
                    }
                    Some(trimmed.to_string())
                } else {
                    None
                };
                if let Some(code) = &custom_code {
                    if code.chars().count() <= 6 {
                        self.show_toast(
                            "Croc code should be more than 6 characters".to_string(),
                            WARNING,
                        );
                        self.croc_use_custom_code = true;
                        return;
                    }
                    self.transfer_code = Some(code.clone());

                    let clean = code.trim().to_string();
                    self.croc_recent_codes.retain(|c| c != &clean);
                    self.croc_recent_codes.insert(0, clean);
                    if self.croc_recent_codes.len() > 5 {
                        self.croc_recent_codes.truncate(5);
                    }
                    self.persist_user_settings();
                }
                let opts = CrocSendOptions {
                    paths: self.send_paths(),
                    custom_code,
                    text_mode: self.croc_text_mode,
                    text_value: if self.croc_text_mode {
                        let t = self.croc_text_value.trim();
                        if t.is_empty() {
                            self.show_toast("Enter text to send in text mode".to_string(), WARNING);
                            return;
                        }
                        Some(t.to_string())
                    } else {
                        None
                    },
                };
                let (rx, handle) = croc_send(opts, &binary);
                self.transfer_rx = Some(rx);
                self.transfer_handle = Some(handle);
            }
            SelectedTool::Sendme => {
                let opts = SendmeSendOptions {
                    paths: self.send_paths(),
                    one_shot: self.sendme_one_shot,
                    blob_dir: self.effective_sendme_blob_dir(),
                };
                let (rx, handle) = sendme_send(opts, &binary);
                self.transfer_rx = Some(rx);
                self.transfer_handle = Some(handle);
            }
            SelectedTool::EazySendme => {
                if self.eazysendme_custom_code.trim().chars().count() < 7 {
                    self.show_toast("Code must be at least 7 characters".to_string(), WARNING);
                    return;
                }
                // Persist the code into recent history
                let clean = self.eazysendme_custom_code.trim().to_string();
                self.eazysendme_recent_codes.retain(|c| c != &clean);
                self.eazysendme_recent_codes.insert(0, clean);
                if self.eazysendme_recent_codes.len() > 5 {
                    self.eazysendme_recent_codes.truncate(5);
                }
                self.persist_user_settings();

                // Step 1: Start sendme_send to get a ticket
                // EazySendme MUST be one-shot because tickets are single-use.
                self.sendme_one_shot = true;
                self.transfer_phase = TransferPhase::Preparing;

                let opts = SendmeSendOptions {
                    paths: self.send_paths(),
                    one_shot: true,
                    blob_dir: self.effective_sendme_blob_dir(),
                };
                let (rx, handle) = sendme_send(opts, &binary);
                self.transfer_rx = Some(rx);
                self.transfer_handle = Some(handle);
            }
        }

        self.transfer_state = TransferState::Running;
        self.transfer_start_time = Some(self.animation_time);
        self.transfer_total_bytes = known_total;
    }

    fn start_receive(&mut self, is_auto: bool) {
        // Same overlap guard as start_send (see above): never run a
        // croc-backed transfer while the croc binary may be replaced.
        if matches!(
            self.selected_tool,
            SelectedTool::Croc | SelectedTool::EazySendme
        ) && self.croc_update_phase != CrocUpdatePhase::Idle
        {
            if is_auto && self.selected_tool == SelectedTool::EazySendme {
                self.eazy_next_retry_time = Some(self.animation_time + 5.0);
            } else if !is_auto {
                self.show_toast(
                    "croc is updating itself, try again in a moment".to_string(),
                    WARNING,
                );
            }
            return;
        }
        if !is_auto {
            self.eazy_retry_count = 0;
            self.eazy_next_retry_time = None;
        }
        if self.receive_code.trim().is_empty() {
            self.show_toast("Enter a code or ticket".to_string(), WARNING);
            return;
        }

        self.reset_transfer();
        self.transfer_code = Some(self.receive_code.trim().to_string());

        // Persist receive code into recent history for Croc / EazySendme
        if self.selected_tool == SelectedTool::Croc
            || self.selected_tool == SelectedTool::EazySendme
        {
            let clean = self.receive_code.trim().to_string();
            if clean.chars().count() > 6 {
                self.croc_receive_recent_codes.retain(|c| c != &clean);
                self.croc_receive_recent_codes.insert(0, clean);
                if self.croc_receive_recent_codes.len() > 5 {
                    self.croc_receive_recent_codes.truncate(5);
                }
                self.persist_user_settings();
            }
        }

        let binary = match self.selected_tool {
            SelectedTool::Croc | SelectedTool::EazySendme => {
                match self.get_tool_binary(&Tool::Croc) {
                    Some(b) => b,
                    None => {
                        self.show_toast("Croc not found".to_string(), ERROR);
                        return;
                    }
                }
            }
            SelectedTool::Sendme => self.get_tool_binary(&Tool::Sendme).unwrap_or_default(),
        };

        match self.selected_tool {
            SelectedTool::Croc => {
                let opts = CrocReceiveOptions {
                    code: self.receive_code.trim().to_string(),
                    output_dir: self.receive_output_dir.clone(),
                };
                let (rx, handle) = croc_receive(opts, &binary);
                self.transfer_rx = Some(rx);
                self.transfer_handle = Some(handle);
            }
            SelectedTool::Sendme => {
                let opts = SendmeReceiveOptions {
                    ticket: self.receive_code.trim().to_string(),
                    output_dir: self.receive_output_dir.clone(),
                    blob_dir: self.effective_sendme_blob_dir(),
                    overwrite: true, // Smart overwrite for standalone Sendme receive.
                };
                let (rx, handle) = sendme_receive(opts, &binary);
                self.transfer_rx = Some(rx);
                self.transfer_handle = Some(handle);
            }
            SelectedTool::EazySendme => {
                let code_key = self.receive_code.trim().to_string();

                if let Some(entry) = self.eazysendme_code_ticket_map.get(&code_key) {
                    let disk_size = crate::backend::get_sendme_blob_directory_size(
                        &entry.ticket,
                        self.effective_sendme_blob_dir().as_deref(),
                    );

                    self.transfer_log.push(if disk_size > 0 {
                        format!(
                            "[Local] Found cached blobs ({} bytes). Verifying local completeness...",
                            disk_size
                        )
                    } else {
                        "[Local] Cached ticket found. Verifying local completeness...".to_string()
                    });

                    // Set the ticket now to prevent "ghost" Croc restarts later
                    self.eazysendme_ticket = Some(entry.ticket.clone());

                    let opts = SendmeReceiveOptions {
                        ticket: entry.ticket.clone(),
                        output_dir: self.receive_output_dir.clone(),
                        blob_dir: self.effective_sendme_blob_dir(),
                        overwrite: true, // Use smart overwrite for local exports too
                    };
                    let (rx, handle) = sendme_check_local(opts);
                    self.transfer_rx = Some(rx);
                    self.transfer_handle = Some(handle);
                    self.eazy_local_check_started_at = Some(self.animation_time);
                    self.transfer_phase = TransferPhase::Transferring;
                    self.transfer_state = TransferState::Running;
                    self.transfer_start_time = Some(self.animation_time);
                    return;
                }

                // Fall back to Phase 1 (Croc)
                let opts = CrocReceiveOptions {
                    code: code_key,
                    output_dir: None, 
                };
                let (rx, handle) = croc_receive(opts, &binary);
                self.transfer_rx = Some(rx);
                self.transfer_handle = Some(handle);
            }
        }

        self.transfer_state = TransferState::Running;
        self.transfer_start_time = Some(self.animation_time);
    }

    fn render_toast(&mut self, ctx: &egui::Context) {
        if let Some((msg, start, color)) = &self.toast_msg {
            let elapsed = ctx.input(|i| i.time) - start;
            if elapsed > 3.0 {
                self.toast_msg = None;
                return;
            }

            let alpha = if elapsed > 2.0 {
                ((3.0 - elapsed) * 255.0) as u8
            } else {
                255
            };

            egui::Area::new(egui::Id::new("toast"))
                .fixed_pos(egui::Pos2::new(
                    ctx.screen_rect().center().x - 150.0,
                    ctx.screen_rect().top() + 12.0,
                ))
                .show(ctx, |ui| {
                    egui::Frame::NONE
                        .fill(Color32::from_rgba_premultiplied(
                            color.r(),
                            color.g(),
                            color.b(),
                            alpha / 6,
                        ))
                        .corner_radius(BUTTON_ROUNDING)
                        .stroke(egui::Stroke::new(
                            1.0,
                            Color32::from_rgba_premultiplied(
                                color.r(),
                                color.g(),
                                color.b(),
                                alpha / 4,
                            ),
                        ))
                        .inner_margin(egui::Margin::symmetric(14, 6))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(msg)
                                    .color(Color32::from_rgba_premultiplied(
                                        color.r(),
                                        color.g(),
                                        color.b(),
                                        alpha,
                                    ))
                                    .size(12.0),
                            );
                        });
                });
            ctx.request_repaint();
        }
    }

    fn render_drag_overlay(&self, ctx: &egui::Context) {
        if !self.drag_hover {
            return;
        }

        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drag_overlay"),
        ));
        painter.rect_filled(
            screen,
            egui::epaint::CornerRadius::same(0),
            Color32::from_rgba_premultiplied(255, 140, 0, 24),
        );
        painter.rect_stroke(
            screen.shrink(3.0),
            egui::epaint::CornerRadius::same(10),
            egui::Stroke::new(2.0, Color32::from_rgba_premultiplied(255, 140, 0, 110)),
            egui::StrokeKind::Inside,
        );
        let is_croc_text = self.selected_tool == SelectedTool::Croc && self.croc_text_mode;
        let text = if is_croc_text {
            "TEXT MODE SELECTED"
        } else {
            "Drop files/folders here"
        };
        let font_size = if is_croc_text { 24.0 } else { 20.0 };
        
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::new(font_size, egui::FontFamily::Proportional),
            Color32::from_rgba_premultiplied(255, 190, 120, 220),
        );
    }

    fn render_popups(&mut self, ctx: &egui::Context) {
        if self.croc_qr_popup_open {
            self.render_croc_qr_popup(ctx);
        }
        if self.croc_text_popup_open {
            self.render_croc_text_popup(ctx);
        }
        if self.cleanup_prompt_open {
            self.render_cleanup_popup(ctx);
        }
        if self.settings_popup_open {
            self.render_settings_popup(ctx);
        }
    }

    fn render_settings_popup(&mut self, ctx: &egui::Context) {
        let mut open = self.settings_popup_open;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -12.0])
            .show(ctx, |ui| {
                ui.set_min_width(260.0);
                // NOTE: checkbox edits a LOCAL copy on purpose. Binding `&mut
                // self.minimize_to_tray` directly would flip the field before
                // `set_minimize_to_tray` runs, making its changed-guard always
                // true and silently skipping init/drop/persist.
                let mut tray_toggle = self.minimize_to_tray;
                if ui
                    .checkbox(&mut tray_toggle, "Live in system tray (close hides to tray)")
                    .changed()
                {
                    let ctx = ui.ctx().clone();
                    self.set_minimize_to_tray(&ctx, tray_toggle);
                }
                let mut debug_toggle = self.tray_debug_log;
                if ui
                    .checkbox(&mut debug_toggle, "Tray debug log (temp file)")
                    .on_hover_text("Writes tray events to databeam-tray-debug.log in the temp dir")
                    .changed()
                {
                    self.tray_debug_log = debug_toggle;
                    tray::set_debug_enabled(debug_toggle);
                    self.persist_user_settings();
                }
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                // Croc self-update status + manual check (automatic checks
                // run on open and every 6 hours).
                if let Some(line) = self.croc_update_status_line() {
                    ui.label(RichText::new(line).size(11.0).color(TEXT_MUTED).italics());
                }
                ui.horizontal(|ui| {
                    let checking = self.croc_update_phase != CrocUpdatePhase::Idle;
                    let btn = ui.add_enabled(
                        !checking,
                        egui::Button::new("↻ Check croc update now"),
                    );
                    if btn.clicked() {
                        self.croc_update_startup_check_done = true;
                        self.start_croc_update_check(ctx);
                        if self.croc_update_phase == CrocUpdatePhase::Idle {
                            self.show_toast("croc is busy or unavailable".to_string(), WARNING);
                        }
                    }
                });
            });
        self.settings_popup_open = open;
    }

    fn render_cleanup_popup(&mut self, ctx: &egui::Context) {
        let mut open = self.cleanup_prompt_open;
        if !open {
            return;
        }
        let mut close_modal = false;
        
        egui::Window::new("Incomplete Downloads Detected")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("DataBeam found incomplete temporary EazySendme downloads.");
                ui.add_space(4.0);
                ui.label(RichText::new(format!(
                    "Found {} folder(s) using {}.",
                    self.cleanup_targets.len(),
                    backend::format_size_unit(self.cleanup_bytes)
                )).strong());
                
                ui.add_space(8.0);
                ui.label(RichText::new("Would you like to delete these incomplete temporary files to reclaim disk space?").color(TEXT_MUTED));
                
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if accent_button_sized(ui, "Delete Files", Color32::from_rgb(200, 60, 60), Vec2::new(120.0, 32.0)).clicked() {
                        let targets = self.cleanup_targets.clone();
                        std::thread::spawn(move || {
                            for path in targets {
                                let _ = std::fs::remove_dir_all(&path);
                            }
                        });
                        self.show_toast("Deleted incomplete downloads".to_string(), SUCCESS);
                        close_modal = true;
                    }
                    if ui.add_sized([80.0, 32.0], egui::Button::new("Keep Files")).clicked() {
                        close_modal = true;
                    }
                });
            });
            
        if !open || close_modal {
            self.cleanup_prompt_open = false;
            self.cleanup_targets.clear();
            self.cleanup_bytes = 0;
        }
    }

    fn render_croc_qr_popup(&mut self, ctx: &egui::Context) {
        let mut open = self.croc_qr_popup_open;
        let code_opt = self.transfer_code.clone();
        let accent = self.engine_color();
        let (title, share_label) = match self.selected_tool {
            SelectedTool::Croc => ("Croc QR", "Scan or copy this code"),
            SelectedTool::Sendme => ("Sendme QR", "Scan or copy this ticket"),
            SelectedTool::EazySendme => ("EazySendme QR", "Scan or copy this ticket"),
        };
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .show(ctx, |ui| {
                let Some(code) = code_opt.as_ref() else {
                    ui.label("Waiting for code...");
                    return;
                };
                ui.label(RichText::new(share_label).color(TEXT_SECONDARY));
                if accent_button_sized(ui, "📋 Copy Code", accent, Vec2::new(100.0, 24.0)).clicked()
                {
                    ui.ctx().copy_text(code.clone());
                    self.show_toast("Code copied".to_string(), SUCCESS);
                }
                ui.add_space(4.0);
                render_qr_blocks(ui, code, 320.0);
                ui.label(
                    RichText::new(code)
                        .monospace()
                        .size(11.0)
                        .color(Color32::BLACK),
                );
            });
        self.croc_qr_popup_open = open;
    }

    fn render_croc_text_popup(&mut self, ctx: &egui::Context) {
        let mut open = self.croc_text_popup_open;
        let text_opt = self.croc_received_text.clone();
        let accent = self.engine_color();
        egui::Window::new("Received Croc Text")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .show(ctx, |ui| {
                let Some(text) = text_opt.as_ref() else {
                    ui.label("No text received yet");
                    return;
                };
                if accent_button_sized(ui, "📋 Copy Text", accent, Vec2::new(100.0, 24.0)).clicked()
                {
                    ui.ctx().copy_text(text.clone());
                    self.show_toast("Text copied".to_string(), SUCCESS);
                }
                let mut display = text.clone();
                ui.add(
                    egui::TextEdit::multiline(&mut display)
                        .desired_rows(6)
                        .interactive(false)
                        .font(egui::FontId::new(12.0, egui::FontFamily::Monospace)),
                );
            });
        self.croc_text_popup_open = open;
    }
}

impl eframe::App for DataBeamApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.animation_time = ctx.input(|i| i.time);
        // One-shot launch repair: a poisoned persisted rect (saved while
        // tiny/iconic) would otherwise open small every launch. A static
        // tiny window generates no further updates, so this cannot wait.
        if !self.launch_size_fixed {
            self.launch_size_fixed = true;
            let first = ctx.screen_rect();
            if let Some(size) = restore_size_override(
                [first.width(), first.height()],
                [WINDOW_DEFAULT_W, WINDOW_DEFAULT_H],
            ) {
                tray::debug_log(&format!(
                    "launch size guard: forcing {}x{}",
                    size[0] as u32, size[1] as u32
                ));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                    size[0], size[1],
                )));
            }
        }
        // Track last-known-good window size for the restore-size guard.
        // Only healthy visible frames qualify (below-minimum rects, e.g. a
        // shrunken restore, are never recorded).
        let screen = ctx.screen_rect();
        if screen.width() >= WINDOW_MIN_W && screen.height() >= WINDOW_MIN_H {
            self.last_good_inner = [screen.width(), screen.height()];
        }
        // ── System tray: close-to-tray + menu actions ──
        // Raw tray/menu events arrive via installed handlers, which apply
        // Show/Quit directly; this drain is a backup for when the loop runs.
        let tray_actions = match self.tray_action_rx.as_ref() {
            Some(rx) => tray::drain_actions_global(rx),
            _ => Vec::new(),
        };
        for action in tray_actions {
            self.apply_tray_action(ctx, action);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            // Tray Exit bypasses this entirely (hard_quit); X always hides
            // to tray while the toggle is on and the icon exists.
            if self.minimize_to_tray && tray::is_active() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.hide_to_tray(ctx);
            }
        }
        if !self.initialized_once {
            self.view = AppView::Home;
            self.initialized_once = true;

            // Silent aging cleanup: remove entries older than 1 week
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let one_week_secs = 7 * 24 * 3600;
            let mut expired_hashes = std::collections::HashSet::new();
            for entry in self.eazysendme_code_ticket_map.values() {
                if entry.timestamp > 0 && now > entry.timestamp + one_week_secs {
                    if let Some(h) = native_ticket_to_hex_hash(&entry.ticket) {
                        expired_hashes.insert(h);
                    }
                }
            }
            for hash in expired_hashes {
                self.cleanup_eazy_cache_by_hash(&hash, true);
            }

            // Background check for orphaned/incomplete temp downloads
            let (tx, rx) = std::sync::mpsc::channel();
            self.cleanup_scan_rx = Some(rx);
            let map = self.eazysendme_code_ticket_map.clone();
            let scan_roots = self.sendme_blob_scan_roots();
            std::thread::spawn(move || {
                let mut targets = Vec::new();
                let mut total_bytes = 0;

                fn dir_size(path: &std::path::Path) -> u64 {
                    let mut total = 0;
                    if let Ok(entries) = std::fs::read_dir(path) {
                        for entry in entries.flatten() {
                            if let Ok(meta) = entry.metadata() {
                                if meta.is_dir() {
                                    total += dir_size(&entry.path());
                                } else {
                                    total += meta.len();
                                }
                            }
                        }
                    }
                    total
                }

                for root in scan_roots {
                    if let Ok(entries) = std::fs::read_dir(&root) {
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if name.starts_with(".sendme-recv-") {
                                let path = entry.path();
                                if !path.is_dir() {
                                    continue;
                                }

                                let hex_hash = name.strip_prefix(".sendme-recv-").unwrap_or("");
                                let mut is_incomplete = true;
                                for cache in map.values() {
                                    if let Some(h) = native_ticket_to_hex_hash(&cache.ticket) {
                                        if h == hex_hash {
                                            is_incomplete = false;
                                            break;
                                        }
                                    }
                                }
                                let size = dir_size(&path);
                                if is_incomplete
                                    && size > 1024 * 1024
                                    && !targets.iter().any(|existing| existing == &path)
                                {
                                    targets.push(path);
                                    total_bytes += size;
                                }
                            }
                        }
                    }
                }
                
                if !targets.is_empty() {
                    let _ = tx.send((targets, total_bytes));
                }
            });
        }

        if let Some(rx) = &self.cleanup_scan_rx {
            match rx.try_recv() {
                Ok((targets, total_bytes)) => {
                    self.cleanup_targets = targets;
                    self.cleanup_bytes = total_bytes;
                    self.cleanup_prompt_open = true;
                    self.cleanup_scan_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.cleanup_scan_rx = None;
                }
                _ => {}
            }
        }

        if let Some((_, ref mut start, _)) = self.toast_msg {
            if *start == 0.0 {
                *start = self.animation_time;
            }
        }

        self.poll_picker_results();
        self.handle_dropped_files(ctx);
        self.poll_size_updates();
        self.poll_transfer();
        // Croc self-update: first check on open, then every 6 hours.
        self.poll_croc_update(ctx);
        self.maybe_trigger_croc_update_check(ctx);
        if let Some(started_at) = self.eazy_local_check_started_at {
            let local_check_running = self.selected_tool == SelectedTool::EazySendme
                && self.view == AppView::Receive
                && self.transfer_state == TransferState::Running;
            if local_check_running && self.animation_time - started_at >= 10.0 {
                self.eazy_local_check_started_at = None;
                self.transfer_log.push(
                    "[Local] Verification timed out after 10s. Retrying...".to_string(),
                );
                self.retry_receive(false);
                // Keep drawing the current frame so the UI doesn't flash blank while the
                // replacement receive flow is being launched.
                ctx.request_repaint();
            }
        }

        if let TransferState::Failed(_) = self.transfer_state {
            if let Some(retry_time) = self.eazy_next_retry_time {
                if self.animation_time >= retry_time {
                    self.eazy_next_retry_time = None;
                    if self.view == AppView::Send {
                        self.retry_send(true);
                    } else {
                        self.retry_receive(true);
                    }
                } else {
                    ctx.request_repaint(); // ensure it repaints for timer updates
                }
            }
        }

        if self.transfer_state == TransferState::Running || self.eazy_next_retry_time.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // ── Top Bar ─────────────────────────────────────────
        egui::TopBottomPanel::top("top_bar")
            .frame(
                egui::Frame::NONE
                    .fill(BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 7)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let accent = self.engine_color();
                    ui.label(RichText::new("⚡").size(18.0).color(accent));
                    ui.label(
                        RichText::new("DataBeam")
                            .size(16.0)
                            .color(TEXT_PRIMARY)
                            .strong(),
                    );
                    ui.add_space(12.0);

                    ui.scope(|ui| {
                        let mut style = (*ui.style()).as_ref().clone();
                        style.visuals.widgets.active.bg_fill = accent;
                        style.visuals.widgets.active.fg_stroke =
                            egui::Stroke::new(1.0, Color32::BLACK);
                        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, accent);
                        style.visuals.selection.bg_fill = Color32::from_rgba_premultiplied(
                            accent.r(),
                            accent.g(),
                            accent.b(),
                            180,
                        );
                        style.visuals.selection.stroke = egui::Stroke::new(1.0, accent);
                        ui.set_style(style);

                        let transfer_running = self.transfer_state == TransferState::Running;
                        for (label, view) in &[
                            ("🏠 Home", AppView::Home),
                            ("📤 Send", AppView::Send),
                            ("📥 Receive", AppView::Receive),
                        ] {
                            let active = self.view == *view;
                            let c = if active {
                                Color32::BLACK
                            } else if transfer_running {
                                TEXT_MUTED
                            } else {
                                TEXT_SECONDARY
                            };
                            let btn = ui.add_enabled(
                                !transfer_running || active,
                                egui::SelectableLabel::new(
                                    active,
                                    RichText::new(*label).size(12.0).color(c),
                                ),
                            );
                            if btn.clicked() && self.view != *view && !transfer_running {
                                self.view = *view;
                                // Clear state to prevent confusion
                                self.send_items.clear();
                                self.receive_code.clear();
                                self.reset_transfer();
                            }
                        }

                        ui.add_space(8.0);
                        if ui
                            .selectable_label(
                                false,
                                RichText::new("❓ Troubleshoot")
                                    .size(12.0)
                                    .color(TEXT_SECONDARY),
                            )
                            .clicked()
                        {
                            let _ = open::that(
                                "https://github.com/vinay-winai/DataBeam/blob/main/trubleshoot.md",
                            );
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Settings cog: lives in the always-visible top bar
                        // (the bottom status bar only renders mid-transfer).
                        if ui
                            .add(egui::Button::new(RichText::new("⚙").size(14.0)).frame(false))
                            .on_hover_text("Settings")
                            .clicked()
                        {
                            self.settings_popup_open = !self.settings_popup_open;
                        }
                        let (tc, tn) = match self.selected_tool {
                            SelectedTool::Croc => (CROC_COLOR, "🐊 croc"),
                            SelectedTool::Sendme => (SENDME_COLOR, "📡 sendme"),
                            SelectedTool::EazySendme => (EAZYSENDME_COLOR, "⚡ eazysendme"),
                        };
                        status_badge(ui, tn, tc);
                    });
                });
            });

        // ── Status Bar ──────────────────────────────────────
        if self.transfer_state == TransferState::Running {
            egui::TopBottomPanel::bottom("status_bar")
                .frame(
                    egui::Frame::NONE
                        .fill(BG_PANEL)
                        .inner_margin(egui::Margin::symmetric(12, 5)),
                )
                .show(ctx, |ui| {
                    let accent = self.engine_color();
                    ui.horizontal(|ui| {
                        let phase = (self.animation_time * 3.0) as usize % 4;
                        ui.label(
                            RichText::new(["◐", "◓", "◑", "◒"][phase])
                                .color(accent)
                                .monospace(),
                        );

                        if let Some(start) = self.transfer_start_time {
                            let e = self.animation_time - start;
                            ui.label(
                                RichText::new(format!("{}:{:02}", e as u64 / 60, e as u64 % 60))
                                    .color(TEXT_MUTED)
                                    .monospace()
                                    .size(11.0),
                            );
                        }

                        match self.transfer_phase {
                            TransferPhase::Preparing => {
                                ui.label(
                                    RichText::new("preparing")
                                        .color(TEXT_MUTED)
                                        .monospace()
                                        .size(11.0),
                                );
                            }
                            TransferPhase::WaitingForReceiver => {
                                ui.label(
                                    RichText::new("waiting")
                                        .color(TEXT_MUTED)
                                        .monospace()
                                        .size(11.0),
                                );
                            }
                            TransferPhase::Transferring => {
                                let effective_progress = self.effective_progress();
                                if effective_progress > 0.01 {
                                    ui.label(
                                        RichText::new(format!(
                                            "{:.0}%",
                                            effective_progress * 100.0
                                        ))
                                        .color(accent)
                                        .monospace()
                                        .size(11.0),
                                    );
                                }
                            }
                            TransferPhase::EazySharingTicket => {
                                ui.label(
                                    RichText::new("sharing ticket")
                                        .color(TEXT_MUTED)
                                        .monospace()
                                        .size(11.0),
                                );
                            }
                            TransferPhase::EazyWaitingForPeer => {
                                ui.label(
                                    RichText::new("waiting for peer")
                                        .color(TEXT_MUTED)
                                        .monospace()
                                        .size(11.0),
                                );
                            }
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if accent_button_sized(ui, "⏹ Stop", ERROR, Vec2::new(75.0, 22.0))
                                .clicked()
                            {
                                self.cancel_transfer();
                            }
                        });
                    });
                });
        }

        // ── Central Panel ───────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                if self.view == AppView::Home {
                    self.show_home(ui);
                } else if self.view == AppView::Send {
                    self.show_send(ui);
                } else {
                    self.show_receive(ui);
                }
                ui.add_space(12.0);
            });
        });

        self.render_toast(ctx);
        self.render_popups(ctx);
        self.render_drag_overlay(ctx);
    }
}

// ── Views ──────────────────────────────────────────────────────────

impl DataBeamApp {
    fn show_home(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(12.0);
            ui.label(
                RichText::new(format!("⚡ DataBeam v{}", APP_VERSION))
                    .size(28.0)
                    .color(TEXT_PRIMARY)
                    .strong(),
            );
            ui.label(
                RichText::new("Secure file transfers via croc & sendme")
                    .size(13.0)
                    .color(TEXT_SECONDARY),
            );
            ui.label(
                RichText::new("Drag & drop files to start")
                    .size(11.0)
                    .color(TEXT_MUTED)
                    .italics(),
            );
            ui.add_space(16.0);
        });

        section_header(ui, "🔧", "Engine");
        ui.add_space(4.0);

        let croc_status = self
            .tool_statuses
            .iter()
            .find(|s| s.tool == Tool::Croc)
            .cloned();
        let sendme_status = self
            .tool_statuses
            .iter()
            .find(|s| s.tool == Tool::Sendme)
            .cloned();

        let eazy_available = croc_status.as_ref().map(|s| s.available).unwrap_or(false)
            && sendme_status.as_ref().map(|s| s.available).unwrap_or(false);

        // ── EazySendme (primary, always shown) ──────────────────────
        if tool_card(
            ui,
            "EazySendme",
            "Sendme performance + Croc like short custom sharing code",
            eazy_available,
            None,
            EAZYSENDME_COLOR,
            self.selected_tool == SelectedTool::EazySendme,
        )
        .clicked()
            && eazy_available
            && self.selected_tool != SelectedTool::EazySendme
        {
            self.switch_tool(SelectedTool::EazySendme);
        }
        ui.add_space(6.0);

        // ── Standalone engines collapsible ──────────────────────────
        {
            let arrow = if self.native_engines_expanded {
                "▾"
            } else {
                "▸"
            };
            let header_resp = ui.add(
                egui::Button::new(
                    RichText::new(format!("{} Standalone Engines", arrow))
                        .size(13.0)
                        .color(TEXT_SECONDARY),
                )
                .fill(BG_CARD)
                .min_size(Vec2::new(ui.available_width(), 36.0))
                .corner_radius(egui::epaint::CornerRadius::same(6u8)),
            );
            if header_resp.clicked() {
                self.native_engines_expanded = !self.native_engines_expanded;
            }

            if self.native_engines_expanded {
                ui.add_space(3.0);
                if let Some(sendme) = &sendme_status {
                    if tool_card(
                        ui,
                        "Sendme",
                        "Cutting-edge performance, reliability, and security",
                        sendme.available,
                        sendme.version.as_deref(),
                        SENDME_COLOR,
                        self.selected_tool == SelectedTool::Sendme,
                    )
                    .clicked()
                        && sendme.available
                        && self.selected_tool != SelectedTool::Sendme
                    {
                        self.switch_tool(SelectedTool::Sendme);
                    }
                }
                ui.add_space(3.0);

                if let Some(croc) = &croc_status {
                    if tool_card(
                        ui,
                        "Croc",
                        "Convenience, ease of use, and 3rd-party mobile support",
                        croc.available,
                        croc.version.as_deref(),
                        CROC_COLOR,
                        self.selected_tool == SelectedTool::Croc,
                    )
                    .clicked()
                        && croc.available
                        && self.selected_tool != SelectedTool::Croc
                    {
                        self.switch_tool(SelectedTool::Croc);
                    }
                }
                ui.add_space(4.0);
            }
        }

        // Live croc self-update status (first check on open, then every 6h).
        // The Croc engine card above re-reads `tool_statuses` each frame, so
        // the version there refreshes automatically after an update.
        if let Some(line) = self.croc_update_status_line() {
            ui.add_space(6.0);
            ui.label(RichText::new(line).size(11.0).color(TEXT_MUTED).italics());
        }

        ui.add_space(12.0);
        section_header(ui, "🚀", "Quick Actions");
        ui.add_space(4.0);

        let accent = self.engine_color();
        let total_w = ui.available_width();
        let btn_w = (total_w - 8.0) / 2.0;

        ui.horizontal(|ui| {
            if accent_button_sized(ui, "📤 Send", accent, Vec2::new(btn_w, 42.0)).clicked() {
                self.view = AppView::Send;
            }
            if accent_button_sized(ui, "📥 Receive", accent, Vec2::new(btn_w, 42.0)).clicked() {
                self.view = AppView::Receive;
            }
        });
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let sendme_avail = sendme_status.as_ref().map(|s| s.available).unwrap_or(false);
            let broadcast_btn = ui.add_enabled(
                sendme_avail,
                egui::Button::new(RichText::new("📡 Broadcast").size(12.0).color(
                    if sendme_avail {
                        Color32::BLACK
                    } else {
                        TEXT_MUTED
                    },
                ))
                .min_size(Vec2::new(btn_w, 42.0))
                .fill(if sendme_avail {
                    SENDME_COLOR
                } else {
                    Color32::from_rgb(50, 50, 55)
                })
                .corner_radius(egui::epaint::CornerRadius::same(8u8)),
            );
            if broadcast_btn.clicked() && sendme_avail {
                if self.selected_tool != SelectedTool::Sendme {
                    self.switch_tool(SelectedTool::Sendme);
                }
                self.sendme_one_shot = false;
                self.view = AppView::Send;
            }

            let croc_avail = croc_status.as_ref().map(|s| s.available).unwrap_or(false);
            let send_text_btn =
                ui.add_enabled(
                    croc_avail,
                    egui::Button::new(RichText::new("🐊 Send Text").size(12.0).color(
                        if croc_avail {
                            Color32::BLACK
                        } else {
                            TEXT_MUTED
                        },
                    ))
                    .min_size(Vec2::new(btn_w, 42.0))
                    .fill(if croc_avail {
                        CROC_COLOR
                    } else {
                        Color32::from_rgb(50, 50, 55)
                    })
                    .corner_radius(egui::epaint::CornerRadius::same(8u8)),
                );
            if send_text_btn.clicked() && croc_avail {
                if self.selected_tool != SelectedTool::Croc {
                    self.switch_tool(SelectedTool::Croc);
                }
                self.croc_text_mode = true;
                self.view = AppView::Send;
            }
        });
    }

    fn show_croc_send_setup(&mut self, ui: &mut egui::Ui, send_locked: bool) {
        let accent = self.engine_color();
        card_frame(ui, |ui| {
            ui.label(
                RichText::new("1) Choose payload")
                    .color(TEXT_PRIMARY)
                    .strong()
                    .size(12.0),
            );
            ui.add_space(4.0);
            ui.add_enabled_ui(!send_locked, |ui| {
                ui.horizontal(|ui| {
                    let file_mode = !self.croc_text_mode;
                    let file_color = if file_mode {
                        accent
                    } else {
                        Color32::from_rgb(190, 190, 190)
                    };
                    if accent_button_sized(ui, "Files/Folders", file_color, Vec2::new(110.0, 24.0))
                        .clicked()
                    {
                        self.croc_text_mode = false;
                    }
                    let text_color = if self.croc_text_mode {
                        accent
                    } else {
                        Color32::from_rgb(190, 190, 190)
                    };
                    if accent_button_sized(ui, "Text", text_color, Vec2::new(64.0, 24.0)).clicked()
                    {
                        if self.send_items.is_empty() {
                            self.croc_text_mode = true;
                        } else {
                            self.show_toast(
                                "Clear files/folders first to switch to text mode".to_string(),
                                WARNING,
                            );
                        }
                    }
                });
                if self.croc_text_mode {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Clear").clicked() {
                                self.croc_text_value.clear();
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("Text to send")
                                            .color(TEXT_SECONDARY)
                                            .size(11.0),
                                    );
                                },
                            );
                        });
                    });
                    ui.add(
                        egui::TextEdit::multiline(&mut self.croc_text_value)
                            .desired_rows(3)
                            .desired_width(ui.available_width() - 8.0)
                            .hint_text("Enter text payload"),
                    );
                } else {
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new("Add files/folders in the item list below.")
                            .size(10.5)
                            .color(TEXT_MUTED),
                    );
                }
            });
        });
        ui.add_space(6.0);

        card_frame(ui, |ui| {
            ui.label(
                RichText::new("2) Choose code mode")
                    .color(TEXT_PRIMARY)
                    .strong()
                    .size(12.0),
            );
            ui.add_space(4.0);
            ui.add_enabled_ui(!send_locked, |ui| {
                let prev_custom = self.croc_use_custom_code;
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.croc_use_custom_code, false, "Random code");
                    ui.radio_value(&mut self.croc_use_custom_code, true, "Custom code");
                });

                if self.croc_use_custom_code
                    && !prev_custom
                    && self.croc_custom_code.trim().is_empty()
                {
                    if let Some(latest) = self.croc_recent_codes.first() {
                        self.croc_custom_code = latest.clone();
                    }
                }

                if self.croc_use_custom_code {
                    ui.add_space(3.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Custom code")
                                .color(TEXT_SECONDARY)
                                .size(11.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.croc_custom_code)
                                .desired_width(220.0)
                                .hint_text("More than 6 characters"),
                        );
                        if ui.small_button("Clear input").clicked() {
                            self.croc_custom_code.clear();
                        }
                    });
                    let custom_len = self.croc_custom_code.trim().chars().count();
                    if custom_len > 0 && custom_len <= 6 {
                        ui.label(
                            RichText::new("Custom code must be more than 6 characters.")
                                .size(10.0)
                                .color(WARNING),
                        );
                    }

                    if !self.croc_recent_codes.is_empty() {
                        let recent_codes = self.croc_recent_codes.clone();
                        ui.add_space(3.0);
                        egui::ComboBox::from_label("Recent codes")
                            .selected_text("Select recent")
                            .show_ui(ui, |ui| {
                                for code in recent_codes {
                                    let text = truncate_middle(&code, 34);
                                    if ui.selectable_label(false, text).clicked() {
                                        self.croc_custom_code = code;
                                    }
                                }
                            });
                        ui.horizontal(|ui| {
                            if ui.small_button("Overwrite with latest").clicked() {
                                if let Some(latest) = self.croc_recent_codes.first() {
                                    self.croc_custom_code = latest.clone();
                                }
                            }
                            if ui.small_button("Clear recent").clicked() {
                                self.croc_recent_codes.clear();
                                self.persist_user_settings();
                            }
                        });
                    }

                    ui.label(
                        RichText::new("Must use more than 6 characters.")
                            .size(10.0)
                            .color(TEXT_MUTED),
                    );
                }
            });
        });
    }

    fn show_sendme_blob_dir_setup(&mut self, ui: &mut egui::Ui, locked: bool) {
        if self.selected_tool != SelectedTool::Sendme
            && self.selected_tool != SelectedTool::EazySendme
        {
            return;
        }

        card_frame(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Blob Cache")
                        .color(TEXT_PRIMARY)
                        .strong()
                        .size(13.0),
                );
                ui.label(
                    RichText::new(truncate_middle(&self.sendme_blob_dir_summary(), 46))
                        .color(TEXT_MUTED)
                        .size(11.0),
                );
            });
            ui.add_space(4.0);
            ui.add_enabled_ui(!locked, |ui| {
                let mut changed = false;
                changed |= ui
                    .radio_value(
                        &mut self.sendme_blob_dir_mode,
                        SendmeBlobDirMode::SystemTemp,
                        "Use OS temp folder",
                    )
                    .changed();
                changed |= ui
                    .radio_value(
                        &mut self.sendme_blob_dir_mode,
                        SendmeBlobDirMode::DownloadDir,
                        "Use same folder as downloads",
                    )
                    .changed();
                changed |= ui
                    .radio_value(
                        &mut self.sendme_blob_dir_mode,
                        SendmeBlobDirMode::Custom,
                        "Use custom folder",
                    )
                    .changed();

                if self.sendme_blob_dir_mode == SendmeBlobDirMode::Custom {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let custom_text = self
                            .sendme_blob_custom_dir
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Choose a folder".to_string());
                        ui.label(
                            RichText::new(truncate_middle(&custom_text, 40))
                                .color(TEXT_MUTED)
                                .size(10.5),
                        );
                        if accent_button_sized(
                            ui,
                            "📂",
                            self.engine_color(),
                            Vec2::new(32.0, 22.0),
                        )
                        .clicked()
                        {
                            self.launch_picker(PickerRequest::SendmeBlobFolder);
                        }
                        if self.sendme_blob_custom_dir.is_some()
                            && accent_button_sized(
                                ui,
                                "✕",
                                Color32::from_rgb(130, 85, 85),
                                Vec2::new(24.0, 22.0),
                            )
                            .clicked()
                        {
                            self.sendme_blob_custom_dir = None;
                            self.sendme_blob_dir_mode = SendmeBlobDirMode::SystemTemp;
                            changed = true;
                        }
                    });
                }

                if changed {
                    self.persist_user_settings();
                }
            });
            ui.label(
                RichText::new(
                    "Used for Sendme and EazySendme temporary blob/cache data on both send and receive.",
                )
                .color(TEXT_MUTED)
                .size(10.0),
            );
        });
    }

    fn show_send(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "📤", "Send");
        ui.add_space(4.0);
        let send_locked = self.transfer_state == TransferState::Running;
        let croc_text_mode = self.selected_tool == SelectedTool::Croc && self.croc_text_mode;
        let croc_text_conflict = croc_text_mode && !self.send_items.is_empty();

        if self.selected_tool == SelectedTool::Croc {
            self.show_croc_send_setup(ui, send_locked);
            ui.add_space(6.0);
        }

        if self.selected_tool == SelectedTool::EazySendme {
            card_frame(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🔑 Custom Code").color(TEXT_PRIMARY).strong());
                    ui.label(RichText::new("(Required)").color(ERROR).size(10.0));
                });
                ui.add_space(4.0);
                ui.add_enabled_ui(!send_locked, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Custom code")
                                .color(TEXT_SECONDARY)
                                .size(11.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.eazysendme_custom_code)
                                .desired_width(220.0)
                                .hint_text("More than 6 characters"),
                        );
                        if ui.small_button("Clear").clicked() {
                            self.eazysendme_custom_code.clear();
                        }
                    });
                    let custom_len = self.eazysendme_custom_code.trim().chars().count();
                    if custom_len > 0 && custom_len <= 6 {
                        ui.label(
                            RichText::new("Custom code must be more than 6 characters.")
                                .size(10.0)
                                .color(WARNING),
                        );
                    }

                    if !self.eazysendme_recent_codes.is_empty() {
                        let recent = self.eazysendme_recent_codes.clone();
                        ui.add_space(3.0);
                        egui::ComboBox::from_id_salt("eazy_recent_codes")
                            .selected_text("Recent codes")
                            .show_ui(ui, |ui| {
                                for code in recent {
                                    let text = truncate_middle(&code, 34);
                                    if ui.selectable_label(false, text).clicked() {
                                        self.eazysendme_custom_code = code;
                                    }
                                }
                            });
                        ui.horizontal(|ui| {
                            if ui.small_button("Overwrite with latest").clicked() {
                                if let Some(latest) = self.eazysendme_recent_codes.first() {
                                    self.eazysendme_custom_code = latest.clone();
                                }
                            }
                            if ui.small_button("Clear recent").clicked() {
                                self.eazysendme_recent_codes.clear();
                                self.persist_user_settings();
                            }
                        });
                    }
                    ui.label(
                        RichText::new("Must use more than 6 characters.")
                            .size(10.0)
                            .color(TEXT_MUTED),
                    );
                });
            });
            ui.add_space(6.0);
        }

        // ── File list ──
        self.show_sendme_blob_dir_setup(ui, send_locked);
        if self.selected_tool == SelectedTool::Sendme
            || self.selected_tool == SelectedTool::EazySendme
        {
            ui.add_space(6.0);
        }

        card_frame(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Items")
                        .color(TEXT_PRIMARY)
                        .strong()
                        .size(13.0),
                );
                if !self.send_items.is_empty() {
                    let n = self.send_items.len();
                    let total = self.total_size();
                    let total_text = if self.total_size_complete() {
                        format_file_size(total)
                    } else if total > 0 {
                        format!("{} (estimating…)", format_file_size(total))
                    } else {
                        "estimating…".to_string()
                    };
                    ui.label(
                        RichText::new(format!(
                            "{} item{} — {}",
                            n,
                            if n == 1 { "" } else { "s" },
                            total_text
                        ))
                        .color(TEXT_MUTED)
                        .size(11.0),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let accent = self.engine_color();
                    let picker_ready =
                        !self.picker_in_flight && self.animation_time >= self.picker_block_until;
                    let mut picker_used_this_frame = false;
                    // Add buttons specific to folders/files
                    if !send_locked
                        && !croc_text_mode
                        && picker_ready
                        && !picker_used_this_frame
                        && accent_button_sized(ui, "+ Folder", accent, Vec2::new(70.0, 22.0))
                            .clicked()
                    {
                        self.launch_picker(PickerRequest::SendFolder);
                        picker_used_this_frame = true;
                    }
                    if !send_locked
                        && !croc_text_mode
                        && picker_ready
                        && !picker_used_this_frame
                        && accent_button_sized(ui, "+ File", accent, Vec2::new(60.0, 22.0))
                            .clicked()
                    {
                        self.launch_picker(PickerRequest::SendFiles);
                    }
                    match self.send_items.is_empty() {
                        true => {} // No clear button if empty
                        false => {
                            if !send_locked
                                && accent_button_sized(
                                    ui,
                                    "Clear All",
                                    Color32::from_rgb(150, 75, 75),
                                    Vec2::new(70.0, 22.0),
                                )
                                .clicked()
                            {
                                if self.transfer_state != TransferState::Idle {
                                    self.reset_transfer();
                                }
                                self.send_items.clear();
                            }
                            ui.add_space(8.0);
                        }
                    }
                });
            });

            if self.send_items.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(RichText::new("📁").size(32.0).color(TEXT_MUTED));
                    ui.label(
                        RichText::new("Drag & drop files here")
                            .color(TEXT_MUTED)
                            .size(14.0),
                    );
                    ui.add_space(20.0);
                });
            } else {
                ui.add_space(4.0);
                let mut to_remove = Vec::new();
                egui::ScrollArea::vertical()
                    .max_height(150.0)
                    .show(ui, |ui| {
                        for (i, item) in self.send_items.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let icon = if item.is_dir { "📂" } else { "📄" };
                                ui.label(RichText::new(icon).size(14.0));
                                let name = item
                                    .path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| item.path.to_string_lossy().to_string());
                                let display_name = truncate_middle(&name, 52);
                                ui.label(
                                    RichText::new(display_name).color(TEXT_PRIMARY).size(13.0),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if !send_locked && ui.small_button("✕").clicked() {
                                            to_remove.push(i);
                                        }
                                        let size_label = if let Some(size) = item.size {
                                            format_file_size(size)
                                        } else if item.is_dir {
                                            "…".to_string()
                                        } else {
                                            "0 B".to_string()
                                        };
                                        ui.label(
                                            RichText::new(size_label).color(TEXT_MUTED).size(11.0),
                                        );
                                    },
                                );
                            });
                        }
                    });
                for idx in to_remove.into_iter().rev() {
                    if self.transfer_state != TransferState::Idle {
                        self.reset_transfer();
                    }
                    self.send_items.remove(idx);
                }
            }
            if croc_text_mode {
                ui.add_space(4.0);
                let msg = if croc_text_conflict {
                    "Text mode enabled: clear files/folders to send only text."
                } else {
                    "Text mode enabled: file/folder add is disabled."
                };
                ui.label(RichText::new(msg).size(10.5).color(WARNING));
            }
        });
        // Removing the old bottom buttons block entirely

        if self.selected_tool == SelectedTool::Sendme {
            ui.add_space(4.0);
            ui.add_enabled_ui(!send_locked, |ui| {
                if ui
                    .checkbox(
                        &mut self.sendme_one_shot,
                        RichText::new("Stop after single transfer").size(12.0),
                    )
                    .changed()
                {
                    self.persist_user_settings();
                }
                if !self.sendme_one_shot {
                    ui.label(
                        RichText::new(
                            "Broadcast mode: sender stays online and shows active transfers.",
                        )
                        .size(10.0)
                        .color(TEXT_MUTED),
                    );
                }
            });
        } else if self.selected_tool == SelectedTool::EazySendme {
            ui.add_space(4.0);
            ui.add_enabled_ui(!send_locked, |ui| {
                if ui
                    .checkbox(
                        &mut self.eazysendme_auto_retry,
                        RichText::new("Auto-retry on failure").size(12.0),
                    )
                    .changed()
                {
                    self.persist_user_settings();
                }
            });
        }

        ui.add_space(8.0);

        // ── Transfer action ──
        match &self.transfer_state {
            TransferState::Idle => {
                let (color, label) = match self.selected_tool {
                    SelectedTool::Croc => (
                        CROC_COLOR,
                        if croc_text_mode {
                            "🐊 Send Text".to_string()
                        } else {
                            format!(
                                "🐊 Send{}",
                                if self.send_items.len() > 1 {
                                    format!(" {} items", self.send_items.len())
                                } else {
                                    String::new()
                                }
                            )
                        },
                    ),
                    SelectedTool::Sendme => (
                        SENDME_COLOR,
                        format!(
                            "📡 Send{}",
                            if self.send_items.len() > 1 {
                                format!(" {} items", self.send_items.len())
                            } else {
                                String::new()
                            }
                        ),
                    ),
                    SelectedTool::EazySendme => (
                        EAZYSENDME_COLOR,
                        format!(
                            "⚡ EazySend{}",
                            if self.send_items.len() > 1 {
                                format!(" {} items", self.send_items.len())
                            } else {
                                String::new()
                            }
                        ),
                    ),
                };
                if croc_text_conflict {
                    ui.label(
                        RichText::new("Clear files/folders or disable text mode before sending.")
                            .size(10.5)
                            .color(ERROR),
                    );
                }
                let enabled = if self.selected_tool == SelectedTool::EazySendme {
                    self.eazysendme_custom_code.len() >= 7
                } else {
                    true
                };

                ui.add_enabled_ui(enabled, |ui| {
                    if accent_button(ui, &label, color).clicked() {
                        self.start_send(false);
                    }
                });
                if !enabled && self.selected_tool == SelectedTool::EazySendme {
                    ui.label(
                        RichText::new("Code too short (min 7 chars)")
                            .color(ERROR)
                            .size(10.0),
                    );
                }
            }
            TransferState::Running => {
                self.show_transfer_status(ui);
            }
            TransferState::Completed => {
                self.show_transfer_status(ui);
                ui.add_space(8.0);
                let accent = self.engine_color();
                if accent_button_sized(ui, "🆕 New Transfer", accent, Vec2::new(130.0, 32.0))
                    .clicked()
                {
                    self.reset_transfer();
                }
            }
            TransferState::Failed(_) => {
                self.show_transfer_status(ui);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let accent = self.engine_color();
                    if accent_button_sized(ui, "🔄 Retry", accent, Vec2::new(100.0, 32.0)).clicked()
                    {
                        self.retry_send(false);
                    }
                    if accent_button_sized(ui, "🆕 New", accent, Vec2::new(100.0, 32.0)).clicked()
                    {
                        self.reset_transfer();
                        self.send_items.clear();
                    }
                });
            }
        }
    }

    fn show_receive(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "📥", "Receive");
        ui.add_space(4.0);

        // ── Code input ──
        card_frame(ui, |ui| {
            let (label, hint) = match self.selected_tool {
                SelectedTool::Croc | SelectedTool::EazySendme => {
                    ("Code Phrase", "e.g. 1234-ocean-monkey")
                }
                SelectedTool::Sendme => ("Ticket", "paste ticket here"),
            };
            ui.label(RichText::new(label).color(TEXT_PRIMARY).strong().size(13.0));
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let running = self.transfer_state == TransferState::Running || self.transfer_state == TransferState::Completed;
                    if ui.add_enabled(!running, egui::Button::new("Clear").small()).clicked() {
                        self.receive_code.clear();
                    }
                    ui.add_enabled(
                        !running,
                        egui::TextEdit::singleline(&mut self.receive_code)
                            .hint_text(hint)
                            .desired_width(ui.available_width())
                            .font(egui::FontId::new(13.0, egui::FontFamily::Monospace)),
                    );
                });
            });

            // Recent codes for Croc / EazySendme receivers
            let show_recent = (self.selected_tool == SelectedTool::Croc
                || self.selected_tool == SelectedTool::EazySendme)
                && !self.croc_receive_recent_codes.is_empty();
            if show_recent {
                let recent = self.croc_receive_recent_codes.clone();
                ui.add_space(3.0);
                egui::ComboBox::from_id_salt("croc_receive_recent")
                    .selected_text("Recent codes")
                    .show_ui(ui, |ui| {
                        for code in recent {
                            let text = truncate_middle(&code, 34);
                            if ui.selectable_label(false, text).clicked() {
                                self.receive_code = code;
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    if ui.small_button("Overwrite with latest").clicked() {
                        if let Some(latest) = self.croc_receive_recent_codes.first() {
                            self.receive_code = latest.clone();
                        }
                    }
                    if ui.small_button("Clear recent").clicked() {
                        self.croc_receive_recent_codes.clear();
                        self.persist_user_settings();
                    }
                });
            }

            ui.add_space(3.0);
            if self.selected_tool == SelectedTool::EazySendme {
                let code_key = self.receive_code.trim().to_string();
                let (icon, msg, color) = if !code_key.is_empty() {
                    let active_ticket = self.eazysendme_ticket.clone();
                    let active_code = self.transfer_code.as_deref().unwrap_or("").trim();
                    let is_active = code_key == active_code;

                    let ticket_to_check = if is_active && active_ticket.is_some() {
                        active_ticket
                    } else {
                        self.eazysendme_code_ticket_map.get(&code_key).map(|e| e.ticket.clone())
                    };

                    if let Some(tick) = ticket_to_check {
                        let disk_size = crate::backend::get_sendme_blob_directory_size(
                            &tick,
                            self.effective_sendme_blob_dir().as_deref(),
                        );
                        if disk_size > 0 {
                            (
                                true,
                                "Local blob cache found - retry will verify and export if complete",
                                Color32::from_rgb(100, 200, 120),
                            )
                        } else {
                            (
                                false,
                                "Cached ticket found - no local blob data yet",
                                TEXT_MUTED,
                            )
                        }
                    } else {
                        (false, "No local cache for this code", TEXT_MUTED)
                    }
                } else {
                    (false, "No code entered", TEXT_MUTED)
                };
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    let center = rect.center();
                    if icon {
                        ui.painter().circle_filled(center, 3.0, color);
                    } else {
                        ui.painter()
                            .circle_stroke(center, 3.0, egui::Stroke::new(1.0, color));
                    }
                    ui.add_space(2.0);
                    ui.label(RichText::new(msg).color(color).size(10.0));
                });
            } else {
                ui.label(
                    RichText::new("Interrupted downloads can be resumed by running Receive again.")
                        .color(TEXT_MUTED)
                        .size(10.0),
                );
            }
        });

        ui.add_space(6.0);

        // ── Output dir ──
        card_frame(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Save to")
                        .color(TEXT_PRIMARY)
                        .strong()
                        .size(13.0),
                );
                let dir_text = self
                    .receive_output_dir
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Current directory".into());
                ui.label(
                    RichText::new(truncate_middle(&dir_text, 46))
                        .color(TEXT_MUTED)
                        .size(11.0),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(dir) = self.effective_receive_folder() {
                        if accent_button_sized(ui, "↗", self.engine_color(), Vec2::new(24.0, 22.0))
                            .clicked()
                        {
                            let _ = open::that(&dir);
                        }
                    }
                    if accent_button_sized(ui, "📂", self.engine_color(), Vec2::new(32.0, 22.0))
                        .clicked()
                    {
                        self.launch_picker(PickerRequest::ReceiveFolder);
                    }
                    if self.receive_output_dir.is_some()
                        && accent_button_sized(
                            ui,
                            "✕",
                            Color32::from_rgb(130, 85, 85),
                            Vec2::new(24.0, 22.0),
                        )
                        .clicked()
                    {
                        self.receive_output_dir = None;
                        self.persist_user_settings();
                    }
                });
            });
        });
        self.show_sendme_blob_dir_setup(
            ui,
            self.transfer_state == TransferState::Running,
        );
        if self.selected_tool == SelectedTool::Sendme
            || self.selected_tool == SelectedTool::EazySendme
        {
            ui.add_space(6.0);
        }
        if self.selected_tool == SelectedTool::Croc {
            let text = self
                .croc_received_text
                .clone()
                .unwrap_or_else(|| "No text received yet.".to_string());
            let has_text = self.croc_received_text.is_some();
            ui.add_space(6.0);
            card_frame(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Received Text")
                            .color(TEXT_PRIMARY)
                            .strong()
                            .size(12.0),
                    );
                    ui.add_enabled_ui(has_text, |ui| {
                        if accent_button_sized(
                            ui,
                            "📋 Copy",
                            self.engine_color(),
                            Vec2::new(70.0, 22.0),
                        )
                        .clicked()
                        {
                            ui.ctx().copy_text(text.clone());
                            self.show_toast("Text copied".to_string(), SUCCESS);
                        }
                        if accent_button_sized(
                            ui,
                            "🔍 Popup",
                            self.engine_color(),
                            Vec2::new(74.0, 22.0),
                        )
                        .clicked()
                        {
                            self.croc_text_popup_open = true;
                        }
                    });
                });
                let mut display = text;
                ui.add(
                    egui::TextEdit::multiline(&mut display)
                        .desired_rows(4)
                        .interactive(false)
                        .font(egui::FontId::new(12.0, egui::FontFamily::Monospace)),
                );
            });
        }

        if self.selected_tool == SelectedTool::EazySendme {
            ui.add_space(4.0);
            let receive_locked = !matches!(
                self.transfer_state,
                TransferState::Idle | TransferState::Failed(_) | TransferState::Completed
            );
            ui.add_enabled_ui(!receive_locked, |ui| {
                if ui
                    .checkbox(
                        &mut self.eazysendme_auto_retry,
                        RichText::new("Auto-retry on failure").size(12.0),
                    )
                    .changed()
                {
                    self.persist_user_settings();
                }
            });
        }

        ui.add_space(8.0);

        match &self.transfer_state {
            TransferState::Idle => {
                let (color, label) = match self.selected_tool {
                    SelectedTool::Croc => (CROC_COLOR, "🐊 Receive"),
                    SelectedTool::Sendme => (SENDME_COLOR, "📡 Receive"),
                    SelectedTool::EazySendme => (EAZYSENDME_COLOR, "⚡ Receive"),
                };
                if accent_button(ui, label, color).clicked() {
                    self.start_receive(false);
                }
            }
            TransferState::Running => {
                self.show_transfer_status(ui);
            }
            TransferState::Completed => {
                self.show_transfer_status(ui);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if accent_button_sized(
                        ui,
                        "🆕 New Transfer",
                        self.engine_color(),
                        Vec2::new(130.0, 32.0),
                    )
                    .clicked()
                    {
                        self.reset_transfer();
                        self.receive_code.clear();
                    }
                    if let Some(dir) = self.effective_receive_folder() {
                        if accent_button_sized(
                            ui,
                            "📂 Open Folder",
                            self.engine_color(),
                            Vec2::new(100.0, 32.0),
                        )
                        .clicked()
                        {
                            let _ = open::that(&dir);
                        }
                    }
                });
            }
            TransferState::Failed(_) => {
                self.show_transfer_status(ui);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if accent_button_sized(ui, "🆕 New", self.engine_color(), Vec2::new(100.0, 32.0))
                        .clicked()
                    {
                        self.reset_transfer();
                        self.receive_code.clear();
                    }
                    if accent_button_sized(
                        ui,
                        "🔄 Retry",
                        self.engine_color(),
                        Vec2::new(100.0, 32.0),
                    )
                    .clicked()
                    {
                        self.retry_receive(false);
                    }
                });
            }
        }
    }

    fn show_transfer_status(&mut self, ui: &mut egui::Ui) {
        let wants_cancel = false;
        let mut wants_show_qr = false;

        card_frame(ui, |ui| {
            match &self.transfer_state {
                TransferState::Idle => {}
                TransferState::Running => {
                    let accent = self.engine_color();
                    let sendme_serve_mode = self.selected_tool == SelectedTool::Sendme
                        && self.view == AppView::Send
                        && !self.sendme_one_shot;
                    let effective_progress = self.effective_progress();
                    let total = self.transfer_total_bytes.unwrap_or(0);
                    let mut done = self.transfer_done_bytes.unwrap_or(0);
                    if self.selected_tool == SelectedTool::Croc
                        && self.croc_file_progress.is_some()
                        && total > 0
                        && self.transfer_done_bytes.is_none()
                    {
                        done = ((effective_progress as f64) * total as f64) as u64;
                    }
                    ui.horizontal(|ui| {
                        let phase = (self.animation_time * 3.0) as usize % 4;
                        ui.label(
                            RichText::new(["◐", "◓", "◑", "◒"][phase])
                                .color(accent)
                                .monospace(),
                        );

                        let status_text = match self.transfer_phase {
                            TransferPhase::Preparing => "Preparing…",
                            TransferPhase::WaitingForReceiver => {
                                if sendme_serve_mode {
                                    "Broadcast mode: waiting for receivers..."
                                } else {
                                    "Waiting for receiver..."
                                }
                            }
                            TransferPhase::Transferring => {
                                if self.view == AppView::Receive {
                                    if effective_progress >= 0.999 {
                                        "Building and verifying content…"
                                    } else {
                                        "Downloading…"
                                    }
                                } else {
                                    if sendme_serve_mode {
                                        "Broadcast mode active..."
                                    } else {
                                        "Uploading on sender..."
                                    }
                                }
                            }
                            TransferPhase::EazySharingTicket => "Sharing ticket via Croc…",
                            TransferPhase::EazyWaitingForPeer => "Waiting for peer (Sendme)…",
                        };
                        ui.label(RichText::new(status_text).color(accent).strong().size(13.0));

                        if let Some(start) = self.transfer_start_time {
                            let e = self.animation_time - start;
                            ui.label(
                                RichText::new(format!("{}:{:02}", e as u64 / 60, e as u64 % 60))
                                    .color(TEXT_MUTED)
                                    .size(11.0),
                            );
                        }
                        if self.selected_tool == SelectedTool::Croc {
                            if let Some(route) = self.croc_route {
                                ui.label(
                                    RichText::new(route.label())
                                        .color(TEXT_MUTED)
                                        .size(11.0),
                                );
                            }
                        }
                    });
                    ui.add_space(4.0);

                    if !sendme_serve_mode {
                        let panel_active = self.transfer_phase == TransferPhase::Transferring
                            && matches!(
                                self.selected_tool,
                                SelectedTool::Croc | SelectedTool::Sendme | SelectedTool::EazySendme
                            );
                        if panel_active {
                            let panel = if self.selected_tool == SelectedTool::Croc {
                                self.build_croc_panel_data(
                                    accent,
                                    effective_progress,
                                    done,
                                    total,
                                )
                            } else {
                                self.build_sendme_panel_data(
                                    accent,
                                    effective_progress,
                                    done,
                                    total,
                                )
                            };
                            croc_progress_panel(ui, &panel, accent);
                        } else if self.transfer_phase == TransferPhase::Transferring {
                            animated_progress_bar(ui, effective_progress, accent);
                        } else {
                            pulsing_progress_bar(ui, self.animation_time, accent);
                        }
                        let pct_text = format!("{:>5.1}%", effective_progress * 100.0);
                        if !panel_active {
                            ui.horizontal_wrapped(|ui| match self.transfer_phase {
                            TransferPhase::Preparing
                            | TransferPhase::EazySharingTicket
                            | TransferPhase::EazyWaitingForPeer => {
                                let label = match self.transfer_phase {
                                    TransferPhase::EazySharingTicket => {
                                        "Sharing ticket via Croc..."
                                    }
                                    TransferPhase::EazyWaitingForPeer => {
                                        "Waiting for peer (Sendme)..."
                                    }
                                    _ => "Preparing ...",
                                };
                                ui.label(RichText::new(label).size(10.0).color(TEXT_SECONDARY));
                                if total > 0 {
                                    ui.label(
                                        RichText::new(format!(
                                            "Total: {:>9}",
                                            format_file_size(total)
                                        ))
                                        .size(10.0)
                                        .color(TEXT_MUTED)
                                        .monospace(),
                                    );
                                }
                            }
                            TransferPhase::WaitingForReceiver => {
                                ui.label(
                                    RichText::new("Waiting for receiver...")
                                        .size(10.0)
                                        .color(TEXT_SECONDARY),
                                );
                                if total > 0 {
                                    ui.label(
                                        RichText::new(format!(
                                            "Total: {:>9}",
                                            format_file_size(total)
                                        ))
                                        .size(10.0)
                                        .color(TEXT_MUTED)
                                        .monospace(),
                                    );
                                }
                            }
                            TransferPhase::Transferring => {
                                let progress_label = if self.view == AppView::Send {
                                    format!("Upload progress (sender): {}", pct_text)
                                } else {
                                    format!("Download progress: {}", pct_text)
                                };
                                ui.label(
                                    RichText::new(progress_label)
                                        .size(10.0)
                                        .color(TEXT_SECONDARY)
                                        .monospace(),
                                );
                                if total > 0 {
                                    let data_label = if self.view == AppView::Send {
                                        "Uploaded (sender)"
                                    } else {
                                        "Downloaded"
                                    };
                                    ui.label(
                                        RichText::new(format!(
                                            "{}: {:>9} / {:>9}",
                                            data_label,
                                            format_file_size(done),
                                            format_file_size(total)
                                        ))
                                        .size(10.0)
                                        .color(TEXT_MUTED)
                                        .monospace(),
                                    );
                                }
                                if let Some(speed) = self.transfer_speed_bps {
                                    let speed_label = if self.view == AppView::Send {
                                        "Upload speed"
                                    } else {
                                        "Download speed"
                                    };
                                    ui.label(
                                        RichText::new(format!(
                                            "{}: {:>9}/s",
                                            speed_label,
                                            format_file_size(speed as u64)
                                        ))
                                        .size(10.0)
                                        .color(TEXT_MUTED)
                                        .monospace(),
                                    );
                                }
                                if self.selected_tool == SelectedTool::Croc {
                                    if let Some((done_files, total_files)) = self.croc_file_progress
                                    {
                                        ui.label(
                                            RichText::new(format!(
                                                "Files: {}/{}",
                                                done_files, total_files
                                            ))
                                            .size(10.0)
                                            .color(TEXT_MUTED),
                                        );
                                    }
                                } else if self.selected_tool == SelectedTool::Sendme {
                                    if let Some(total_files) = self.sendme_total_items {
                                        let done_files = ((effective_progress * total_files as f32)
                                            .round()
                                            as u64)
                                            .min(total_files);
                                        ui.label(
                                            RichText::new(format!(
                                                "Files: {}/{}",
                                                done_files, total_files
                                            ))
                                            .size(10.0)
                                            .color(TEXT_MUTED),
                                        );
                                    }
                                }
                            }
                        });
                        }
                    } else {
                        let active = self.sendme_active_transfers;
                        let transfer_word = if active == 1 { "transfer" } else { "transfers" };
                        let status_line = match self.transfer_phase {
                            TransferPhase::Preparing => "Preparing broadcast session...",
                            TransferPhase::Transferring if active > 0 => "Broadcast mode active",
                            _ => "Broadcast mode waiting for receivers...",
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(status_line).size(10.0).color(TEXT_SECONDARY));
                            ui.label(
                                RichText::new(format!("{} {} in progress", active, transfer_word))
                                    .size(10.0)
                                    .color(TEXT_MUTED),
                            );
                            if total > 0 {
                                ui.label(
                                    RichText::new(format!("Payload: {}", format_file_size(total)))
                                        .size(10.0)
                                        .color(TEXT_MUTED),
                                );
                            }
                        });
                    }
                    ui.add_space(6.0);
                    // Cancel removed – use Top Bar Stop button for consistency.
                }
                TransferState::Completed => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("✅").size(14.0));
                        ui.label(RichText::new("Complete").color(SUCCESS).strong().size(13.0));
                        if let Some(start) = self.transfer_payload_start_time
                            .or(self.transfer_start_time)
                        {
                            let end = self.transfer_end_time.unwrap_or(self.animation_time);
                            let e = (end - start).max(0.0);
                            ui.label(
                                RichText::new(format!("{}:{:02}", e as u64 / 60, e as u64 % 60))
                                    .color(TEXT_MUTED)
                                    .size(11.0),
                            );
                        }
                    });
                    let done = self.transfer_done_bytes.unwrap_or(0);
                    let total = self.transfer_total_bytes.unwrap_or(0);
                    ui.horizontal_wrapped(|ui| {
                        if total > 0 {
                            let data_label = if self.view == AppView::Send {
                                "Uploaded (sender)"
                            } else {
                                "Downloaded"
                            };
                            ui.label(
                                RichText::new(format!(
                                    "{}: {:>9} / {:>9}",
                                    data_label,
                                    format_file_size(done),
                                    format_file_size(total)
                                ))
                                .size(10.0)
                                .color(TEXT_MUTED)
                                .monospace(),
                            );
                            let progress_label = if self.view == AppView::Send {
                                "Upload progress (sender)"
                            } else {
                                "Download progress"
                            };
                            ui.label(
                                RichText::new(format!(
                                    "{}: {:>5.1}%",
                                    progress_label,
                                    if total > 0 {
                                        (done as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
                                    } else {
                                        100.0
                                    }
                                ))
                                .size(10.0)
                                .color(TEXT_SECONDARY)
                                .monospace(),
                            );
                        }
                        // Show the whole-transfer average (bytes / elapsed) like
                        // getcroc's Rate; the last live sample is a wire burst and
                        // wildly overstates real throughput.
                        let speed = self
                            .average_transfer_speed_bps()
                            .or(self.transfer_speed_bps);
                        if let Some(speed) = speed {
                            let speed_label = if self.view == AppView::Send {
                                "Avg upload speed"
                            } else {
                                "Avg download speed"
                            };
                            ui.label(
                                RichText::new(format!(
                                    "{}: {:>9}/s",
                                    speed_label,
                                    format_file_size(speed as u64)
                                ))
                                .size(10.0)
                                .color(TEXT_MUTED)
                                .monospace(),
                            );
                        }
                        // Elapsed wall time next to the speed, same value as
                        // the header above (payload start if known, else send
                        // start, through completion).
                        if let Some(start) = self
                            .transfer_payload_start_time
                            .or(self.transfer_start_time)
                        {
                            let end = self.transfer_end_time.unwrap_or(self.animation_time);
                            let e = (end - start).max(0.0);
                            ui.label(
                                RichText::new(format!(
                                    "Elapsed: {}:{:02}",
                                    e as u64 / 60,
                                    e as u64 % 60
                                ))
                                .size(10.0)
                                .color(TEXT_MUTED)
                                .monospace(),
                            );
                        }
                        // Data path seen on this transfer (Croc mode only).
                        if self.selected_tool == SelectedTool::Croc {
                            if let Some(route) = self.croc_route {
                                ui.label(
                                    RichText::new(route.label())
                                        .size(10.0)
                                        .color(TEXT_MUTED)
                                        .monospace(),
                                );
                            }
                        }
                    });
                }
                TransferState::Failed(e) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("❌").size(14.0));
                        ui.label(RichText::new(e).color(ERROR).size(12.0));
                        // NOTE: failure-path elapsed is shown here for consistency;
                        // the original request was the Elapsed label in the
                        // Completed body row next to Avg speed.
                        if let Some(start) = self
                            .transfer_payload_start_time
                            .or(self.transfer_start_time)
                        {
                            let end = self.transfer_end_time.unwrap_or(self.animation_time);
                            let e = (end - start).max(0.0);
                            ui.label(
                                RichText::new(format!("{}:{:02}", e as u64 / 60, e as u64 % 60))
                                    .color(TEXT_MUTED)
                                    .size(11.0),
                            );
                        }
                    });
                }
            }

            // Code/ticket — for Croc sender, only show once past the Preparing phase
            let show_code = self.transfer_code.is_some() && {
                let sendme_receiver =
                    self.selected_tool == SelectedTool::Sendme && self.view == AppView::Receive;
                let croc_sender =
                    self.selected_tool == SelectedTool::Croc && self.view == AppView::Send;
                if sendme_receiver {
                    false
                } else if croc_sender {
                    self.transfer_phase != TransferPhase::Preparing
                } else {
                    true
                }
            };
            if show_code {
                if let Some(code) = &self.transfer_code.clone() {
                    ui.add_space(6.0);
                    let (label, color) = match self.selected_tool {
                        SelectedTool::Croc => ("Share this code:", CROC_COLOR),
                        SelectedTool::Sendme => ("Share this ticket:", SENDME_COLOR),
                        SelectedTool::EazySendme => ("EazySendme ticket:", EAZYSENDME_COLOR),
                    };
                    if code_display(ui, label, code, color) {
                        self.show_toast("Code copied".to_string(), SUCCESS);
                    }
                    ui.add_space(4.0);
                    if accent_button_sized(ui, "🔳 Show QR", color, Vec2::new(100.0, 24.0))
                        .clicked()
                    {
                        wants_show_qr = true;
                    }
                }
            }

            // Logs (collapsed)
            if !self.transfer_log.is_empty() {
                ui.add_space(6.0);
                if accent_button_sized(
                    ui,
                    "📋 Copy Log",
                    self.engine_color(),
                    Vec2::new(90.0, 22.0),
                )
                .clicked()
                {
                    ui.ctx().copy_text(self.transfer_log.join("\n"));
                    self.show_toast("Log copied".to_string(), SUCCESS);
                }
                let stick_bottom = self.transfer_state == TransferState::Running;
                egui::CollapsingHeader::new(RichText::new("Log").color(TEXT_MUTED).size(10.0))
                    .default_open(false)
                    .show(ui, |ui| {
                        log_area(ui, &self.transfer_log, 120.0, stick_bottom);
                    });
            }
        });

        // Handle button actions (deferred because we can't borrow &mut self inside card_frame closure)
        if wants_cancel {
            self.cancel_transfer();
        }
        if wants_show_qr {
            self.croc_qr_popup_open = true;
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────

fn render_qr_blocks(ui: &mut egui::Ui, code: &str, max_side: f32) {
    let Ok(qr) = QrCode::new(code.as_bytes()) else {
        ui.label("Unable to generate QR");
        return;
    };
    let colors = qr.to_colors();
    let side = qr.width();
    if side == 0 {
        ui.label("Unable to generate QR");
        return;
    }

    let module = (max_side / side as f32).max(2.0);
    let draw_side = module * side as f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(draw_side, draw_side), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, Color32::WHITE);

    for y in 0..side {
        for x in 0..side {
            let idx = y * side + x;
            if colors[idx] == QrModuleColor::Dark {
                let min = rect.min + Vec2::new(x as f32 * module, y as f32 * module);
                let max = min + Vec2::splat(module);
                painter.rect_filled(egui::Rect::from_min_max(min, max), 0.0, Color32::BLACK);
            }
        }
    }
}

fn is_masked_croc_code(code: &str) -> bool {
    let trimmed = code.trim();
    !trimmed.is_empty()
        && trimmed.contains('*')
        && !trimmed.chars().any(|c| c.is_ascii_alphanumeric())
}

fn databeam_icon() -> egui::IconData {
    let png = include_bytes!("../assets/icons/icon-256.png");
    let image = image::load_from_memory(png)
        .expect("embedded app icon must decode")
        .into_rgba8();
    let (width, height) = image.dimensions();

    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn truncate_middle(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 10 {
        return value.chars().take(max_chars).collect();
    }
    let head = (max_chars.saturating_sub(1) * 2) / 3;
    let tail = max_chars.saturating_sub(head + 1);
    let start: String = value.chars().take(head).collect();
    let end: String = value
        .chars()
        .rev()
        .take(tail)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{start}…{end}")
}

fn parse_total_size_hint(line: &str) -> Option<u64> {
    let lower = line.to_lowercase();
    if let Some(idx) = lower.find("in total,") {
        let tail = line.get(idx + "in total,".len()..)?.trim();
        return parse_croc_header_size(tail);
    }
    None
}

/// Sizes in croc's session-summary lines ("Sending 4 files (60.1 MB)") come
/// from a base-1024 humanizer, unlike the progressbar frames (base-1000).
fn parse_croc_header_size(text: &str) -> Option<u64> {
    let mut parts = text.split_whitespace();
    let num = parts.next()?.trim_matches(|c: char| c == ',' || c == '.');
    let unit = parts.next()?.trim_matches(|c: char| c == ',' || c == '.');
    let value = num.parse::<f64>().ok()?;
    if value < 0.0 {
        return None;
    }
    let lower = unit.to_lowercase();
    let mult = if lower.ends_with("ib") {
        unit_multiplier(unit)
    } else {
        match lower.as_str() {
            "b" => 1.0,
            "kb" => 1024.0,
            "mb" => 1024.0 * 1024.0,
            "gb" => 1024f64.powi(3),
            "tb" => 1024f64.powi(4),
            "pb" => 1024f64.powi(5),
            _ => return None,
        }
    };
    Some((value * mult) as u64)
}

fn parse_stage_progress(line: &str) -> Option<f32> {
    let mut scan = line;
    while let Some(open) = scan.find('[') {
        let after_open = &scan[open + 1..];
        let Some(close_rel) = after_open.find(']') else {
            break;
        };
        let inside = after_open[..close_rel].trim();
        if let Some((left, right)) = inside.split_once('/') {
            let cur = left.trim().parse::<u32>();
            let total = right.trim().parse::<u32>();
            if let (Ok(cur), Ok(total)) = (cur, total) {
                if total > 0 && cur <= total && total <= 12 {
                    return Some(cur as f32 / total as f32);
                }
            }
        }
        scan = &after_open[close_rel + 1..];
    }
    None
}

fn parse_croc_file_counter_progress(line: &str) -> Option<(u64, u64)> {
    let mut tokens: Vec<&str> = line.split_whitespace().collect();
    let token = tokens.pop()?;
    let clean =
        token.trim_matches(|c: char| c == '(' || c == ')' || c == '[' || c == ']' || c == ',');
    let (done_s, total_s) = clean.split_once('/')?;
    if !done_s.chars().all(|c| c.is_ascii_digit()) || !total_s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let done = done_s.parse::<u64>().ok()?;
    let total = total_s.parse::<u64>().ok()?;
    if total == 0 || done > total {
        return None;
    }
    Some((done, total))
}

fn parse_croc_total_size_hint(line: &str) -> Option<u64> {
    let lower = line.to_lowercase();
    let starts_ok = lower.starts_with("sending ") || lower.starts_with("receiving ");
    let looks_like_session_summary = lower.contains(" file (") || lower.contains(" files (");
    if !(starts_ok && looks_like_session_summary) {
        return None;
    }
    let open = line.rfind('(')?;
    let close = line.rfind(')')?;
    if close <= open + 1 {
        return None;
    }
    let inside = line.get(open + 1..close)?.trim();
    parse_croc_header_size(inside)
}

// ── Croc progressbar frame parsing (schollz/progressbar v3 output) ───
//
// Croc renders one progressbar per file on stderr using \r-delimited redraws.
// With stderr piped (no TTY, colors disabled) a frame looks like:
//
//   \r<filename>  56% |███████             | (56.2 MB/600.7 MB, 4.46 MB/s) [1m3s:2m3s]
//
// The final 100% render omits the [elapsed:eta] bracket. Sizes use SI units
// (base-1000 kB/MB/GB). This parser extracts exact byte counts, speed and ETA.

#[derive(Debug, Clone, PartialEq)]
pub struct CrocBarFrame {
    pub file_name: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub speed_bps: Option<f64>,
    pub eta_secs: Option<f64>,
    /// Per-file completion counter (" 1/4") appended to the final frame line.
    pub file_index: Option<u64>,
    pub file_count: Option<u64>,
}

/// Multiplier for croc's humanized units. Decimal units (kB/MB/...) are
/// base-1000 as used by progressbar's `humanizeBytes`; binary units stay at 1024.
fn croc_unit_multiplier(unit: &str) -> Option<f64> {
    fn prefix_power(u: &str) -> Option<i32> {
        match u {
            "k" => Some(1),
            "m" => Some(2),
            "g" => Some(3),
            "t" => Some(4),
            "p" => Some(5),
            "e" => Some(6),
            _ => None,
        }
    }
    let lower = unit.trim().to_lowercase();
    if lower == "b" {
        return Some(1.0);
    }
    if let Some(stripped) = lower.strip_suffix("ib") {
        return Some(1024f64.powi(prefix_power(stripped)?));
    }
    Some(1000f64.powi(prefix_power(lower.strip_suffix('b')?)?))
}

/// Parses Go `time.Duration.String()` output like "45s", "2m3s", "1h4m5s".
fn parse_go_duration(text: &str) -> Option<f64> {
    let chars: Vec<char> = text.trim().chars().collect();
    if chars.is_empty() || !chars[0].is_ascii_digit() {
        return None;
    }
    let mut total = 0.0f64;
    let mut i = 0usize;
    while i < chars.len() {
        let num_start = i;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
            i += 1;
        }
        if num_start == i {
            return None;
        }
        let value: f64 = chars[num_start..i].iter().collect::<String>().parse().ok()?;
        let unit_start = i;
        while i < chars.len() && !chars[i].is_ascii_digit() && chars[i] != '.' {
            i += 1;
        }
        let unit: String = chars[unit_start..i].iter().collect();
        let mul = match unit.as_str() {
            "h" => 3600.0,
            "m" => 60.0,
            "s" => 1.0,
            "ms" => 0.001,
            "us" | "µs" => 0.000001,
            "ns" => 0.000000001,
            _ => return None,
        };
        total += value * mul;
    }
    Some(total)
}

/// Parses "(done/total[, speed])" stats where sizes are either "A/B UNIT"
/// (shared suffix, e.g. "56.2/600.7 MB") or "A UNIT_A/B UNIT_B"
/// (different suffixes, e.g. "210 MB/5 GB").
fn parse_croc_size_pair(part: &str) -> Option<(u64, u64)> {
    let tokens: Vec<&str> = part.split_whitespace().collect();
    let (done, total) = match tokens.len() {
        2 => {
            let mult = croc_unit_multiplier(tokens[1])?;
            let (a, b) = tokens[0].split_once('/')?;
            (
                a.trim().parse::<f64>().ok()? * mult,
                b.trim().parse::<f64>().ok()? * mult,
            )
        }
        3 => {
            let (unit_a, total_num) = tokens[1].split_once('/')?;
            let mult_a = croc_unit_multiplier(unit_a)?;
            let mult_b = croc_unit_multiplier(tokens[2])?;
            (
                tokens[0].parse::<f64>().ok()? * mult_a,
                total_num.parse::<f64>().ok()? * mult_b,
            )
        }
        _ => return None,
    };
    if done < 0.0 || total <= 0.0 {
        return None;
    }
    Some((done as u64, total as u64))
}

fn parse_croc_speed_part(part: &str) -> Option<f64> {
    let speed = part.trim();
    let value_part = speed.strip_suffix("/s")?;
    let mut tokens = value_part.split_whitespace();
    let num: f64 = tokens.next()?.parse().ok()?;
    let unit = tokens.next()?;
    if tokens.next().is_some() {
        return None;
    }
    let mult = croc_unit_multiplier(unit)?;
    if num < 0.0 {
        return None;
    }
    Some(num * mult)
}

/// Finds the `%` token of a bar frame and validates the `|...|` bar after it.
/// Returns the byte index of `%` and the start index of its digit run.
fn find_croc_percent_token(s: &str) -> Option<(usize, usize)> {
    for (idx, ch) in s.char_indices() {
        if ch != '%' {
            continue;
        }
        let before = &s[..idx];
        let digits: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if digits.is_empty() || digits.len() > 3 {
            continue;
        }
        let after = s[idx + 1..].trim_start();
        let Some(inner) = after.strip_prefix('|') else {
            continue;
        };
        let Some(close) = inner.find('|') else {
            continue;
        };
        // Bar content is saucer/padding glyphs only; reject prose like "|note|".
        if !inner[..close].is_empty()
            && !inner[..close]
                .chars()
                .any(|c| c.is_ascii_alphanumeric() && c != '=')
        {
            return Some((idx, idx - digits.len()));
        }
    }
    None
}

/// Parses one redraw of croc's per-file progressbar into structured data.
pub fn parse_croc_bar_frame(line: &str) -> Option<CrocBarFrame> {
    let s = line.trim();
    let (pct_idx, digits_start) = find_croc_percent_token(s)?;
    let percent: u32 = s[digits_start..pct_idx].parse().ok()?;
    if percent > 100 {
        return None;
    }

    let after_bar = s[pct_idx + 1..].trim_start();
    let inner = after_bar.strip_prefix('|')?;
    let close = inner.find('|')?;
    let rest = inner[close + 1..].trim_start();

    let stats = rest.strip_prefix('(')?;
    let stats_close = stats.rfind(')')?;
    let body = &stats[..stats_close];
    let tail = stats[stats_close + 1..].trim();

    let mut parts = body.split(',');
    let size_part = parts.next()?.trim();
    let speed_bps = parts.next().and_then(parse_croc_speed_part);
    if parts.next().is_some() {
        return None;
    }

    let (done_bytes, total_bytes) = parse_croc_size_pair(size_part)?;

    // Optional "[elapsed:eta]" tail; absent on the finished 100% render, where
    // croc instead appends the per-file completion counter (" 1/4").
    let mut rest = tail;
    let mut eta_secs = None;
    if rest.starts_with('[') {
        if let Some(close) = rest.find(']') {
            eta_secs = rest[1..close]
                .split_once(':')
                .and_then(|(_, remaining)| parse_go_duration(remaining));
            rest = rest[close + 1..].trim();
        }
    }
    let counter = rest.split_whitespace().next().unwrap_or("");
    let (file_index, file_count) = match counter.split_once('/') {
        Some((a, b))
            if !a.is_empty()
                && !b.is_empty()
                && a.chars().all(|c| c.is_ascii_digit())
                && b.chars().all(|c| c.is_ascii_digit()) =>
        {
            match (a.parse::<u64>(), b.parse::<u64>()) {
                (Ok(i), Ok(n)) => (Some(i), Some(n)),
                _ => (None, None),
            }
        }
        _ => (None, None),
    };

    Some(CrocBarFrame {
        file_name: s[..digits_start].trim().to_string(),
        done_bytes,
        total_bytes,
        speed_bps,
        eta_secs,
        file_index,
        file_count,
    })
}

/// Formats seconds like croc's UI: "45s", "2m 3s", "1h 4m".
pub fn format_eta_compact(secs: f64) -> String {
    if !secs.is_finite() || secs <= 0.0 {
        return "--".to_string();
    }
    let secs = secs.round() as u64;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn parse_sendme_imported_size_hint(line: &str) -> Option<u64> {
    let lower = line.to_lowercase();
    if !(lower.starts_with("imported directory ") || lower.starts_with("imported file ")) {
        return None;
    }
    let mut parts = line.split(',');
    let _path_part = parts.next()?;
    let size_part = parts.next()?.trim();
    parse_size_prefix(size_part)
}

fn parse_sendme_total_files_hint(line: &str) -> Option<u64> {
    let lower = line.to_lowercase();
    let start = lower.find("importing ")?;
    let tail = lower.get(start + "importing ".len()..)?;
    let mut parts = tail.split_whitespace();
    let total = parts.next()?.parse::<u64>().ok()?;
    let unit = parts.next().unwrap_or_default();
    if !unit.starts_with("file") {
        return None;
    }
    Some(total)
}

fn parse_sendme_item_index(line: &str) -> Option<u64> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for pair in tokens.windows(2) {
        if pair[0] == "i" {
            if let Ok(raw) = pair[1].parse::<u64>() {
                return Some(raw.saturating_add(1));
            }
        }
    }
    None
}

fn parse_sendme_r_counter(line: &str) -> Option<(u64, u64)> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for pair in tokens.windows(2) {
        if pair[0] == "r" {
            let raw = pair[1]
                .trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')' || c == ',');
            let (done_s, total_s) = raw.split_once('/')?;
            let done = done_s.parse::<u64>().ok()?;
            let total = total_s.parse::<u64>().ok()?;
            return Some((done, total));
        }
    }
    None
}

fn parse_sendme_r_payload_progress(line: &str) -> Option<(u64, u64)> {
    let (done, total) = parse_sendme_r_counter(line)?;
    if total == 0 || done > total {
        return None;
    }
    Some((done, total))
}

fn is_sendme_sender_request_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("n ")
        && trimmed.contains(" r ")
        && trimmed.contains(" i ")
        && trimmed.contains(" # ")
        && parse_payload_progress(trimmed).is_some()
}

fn is_cli_progress_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    (lower.contains("download") || lower.contains("upload"))
        && line.contains('/')
        && (line.contains("B/s")
            || line.contains("iB/s")
            || line.contains("KB/s")
            || line.contains("MB/s")
            || line.contains("GB/s"))
}

fn compact_cli_progress_line(line: &str) -> String {
    let trimmed = line.trim();
    if let Some(idx) = trimmed.rfind("[3/4]") {
        return trimmed[idx..].trim().to_string();
    }
    if let Some(idx) = trimmed.rfind("[4/4]") {
        return trimmed[idx..].trim().to_string();
    }
    if trimmed.chars().count() > 220 {
        return trimmed
            .chars()
            .rev()
            .take(220)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
    }
    trimmed.to_string()
}

fn parse_payload_progress(line: &str) -> Option<(u64, u64)> {
    let words: Vec<&str> = line.split_whitespace().collect();

    // Pattern A: "11/11 MB"
    for w in words.windows(2) {
        let ratio =
            w[0].trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')' || c == ',');
        let unit =
            w[1].trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')' || c == ',');
        let Some((done_str, total_str)) = ratio.split_once('/') else {
            continue;
        };
        if !is_size_unit(unit) {
            continue;
        }
        let Ok(done_num) = done_str.parse::<f64>() else {
            continue;
        };
        let Ok(total_num) = total_str.parse::<f64>() else {
            continue;
        };
        if done_num < 0.0 || total_num <= 0.0 {
            continue;
        }
        let mul = unit_multiplier(unit);
        return Some(((done_num * mul) as u64, (total_num * mul) as u64));
    }

    // Pattern B: "8.98 MiB/19.02 MiB"
    for w in words.windows(3) {
        let done_clean = w[0]
            .trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')' || c == ',')
            .to_string();
        let Ok(done_num) = done_clean.parse::<f64>() else {
            continue;
        };
        let mid =
            w[1].trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')' || c == ',');
        let total_unit =
            w[2].trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')' || c == ',');

        let Some((done_unit, total_num_str)) = mid.split_once('/') else {
            continue;
        };
        if !is_size_unit(done_unit) || !is_size_unit(total_unit) {
            continue;
        }
        let Ok(total_num) = total_num_str.parse::<f64>() else {
            continue;
        };
        if done_num < 0.0 || total_num <= 0.0 {
            continue;
        }
        let done = (done_num * unit_multiplier(done_unit)) as u64;
        let total = (total_num * unit_multiplier(total_unit)) as u64;
        if total > 0 {
            return Some((done, total));
        }
    }

    // Pattern C: "8.98 MiB / 19.02 MiB"
    for w in words.windows(5) {
        let done_str =
            w[0].trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')' || c == ',');
        let done_unit =
            w[1].trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')' || c == ',');
        let slash =
            w[2].trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')' || c == ',');
        let total_str =
            w[3].trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')' || c == ',');
        let total_unit =
            w[4].trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')' || c == ',');
        if slash != "/" {
            continue;
        }
        if !is_size_unit(done_unit) || !is_size_unit(total_unit) {
            continue;
        }
        let Ok(done_num) = done_str.parse::<f64>() else {
            continue;
        };
        let Ok(total_num) = total_str.parse::<f64>() else {
            continue;
        };
        if done_num < 0.0 || total_num <= 0.0 {
            continue;
        }
        let done = (done_num * unit_multiplier(done_unit)) as u64;
        let total = (total_num * unit_multiplier(total_unit)) as u64;
        if total > 0 {
            return Some((done, total));
        }
    }
    None
}

fn parse_speed_hint(line: &str) -> Option<f64> {
    let words: Vec<&str> = line.split_whitespace().collect();
    for token in &words {
        if let Some(speed) = parse_speed_token(token) {
            return Some(speed);
        }
    }
    for pair in words.windows(2) {
        let n = pair[0].trim_matches(|c: char| c == '(' || c == ')' || c == '[' || c == ']');
        let u = pair[1].trim_matches(|c: char| c == '(' || c == ')' || c == '[' || c == ']');
        if let Ok(value) = n.parse::<f64>() {
            if let Some(unit) = u.strip_suffix("/s") {
                if !is_size_unit(unit) {
                    continue;
                }
                return Some(value * unit_multiplier(unit));
            }
        }
    }
    None
}

fn parse_speed_token(token: &str) -> Option<f64> {
    let cleaned = token.trim_matches(|c: char| c == '(' || c == ')' || c == '[' || c == ']');
    let slash = cleaned.find('/')?;
    let (num_part, unit_part) = cleaned.split_at(slash);
    let unit = unit_part.strip_prefix('/')?;
    if !unit.eq_ignore_ascii_case("s")
        && !unit.eq_ignore_ascii_case("b/s")
        && !unit.eq_ignore_ascii_case("kb/s")
        && !unit.eq_ignore_ascii_case("kib/s")
        && !unit.eq_ignore_ascii_case("mb/s")
        && !unit.eq_ignore_ascii_case("mib/s")
        && !unit.eq_ignore_ascii_case("gb/s")
        && !unit.eq_ignore_ascii_case("gib/s")
    {
        return None;
    }
    let amount = num_part.parse::<f64>().ok()?;
    if amount < 0.0 {
        return None;
    }
    if unit.eq_ignore_ascii_case("s") {
        return None;
    }
    Some(amount * unit_multiplier(unit.trim_end_matches("/s")))
}

fn parse_size_prefix(text: &str) -> Option<u64> {
    let mut parts = text.split_whitespace();
    let num = parts.next()?.trim_matches(|c: char| c == ',' || c == '.');
    let unit = parts.next()?.trim_matches(|c: char| c == ',' || c == '.');
    let value = num.parse::<f64>().ok()?;
    if value < 0.0 {
        return None;
    }
    Some((value * unit_multiplier(unit)) as u64)
}

fn unit_multiplier(unit: &str) -> f64 {
    let lower = unit.to_lowercase();
    // Binary units are base-1024; decimal units (used by croc's CLI and
    // progressbar) are base-1000.
    match lower.as_str() {
        "b" => 1.0,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        "pib" => 1024f64.powi(5),
        "eib" => 1024f64.powi(6),
        "kb" => 1000.0,
        "mb" => 1000.0 * 1000.0,
        "gb" => 1000.0 * 1000.0 * 1000.0,
        "tb" => 1000f64.powi(4),
        "pb" => 1000f64.powi(5),
        "eb" => 1000f64.powi(6),
        _ => 1.0,
    }
}

fn is_size_unit(unit: &str) -> bool {
    matches!(
        unit.to_lowercase().as_str(),
        "b" | "kb" | "kib" | "mb" | "mib" | "gb" | "gib" | "tb" | "tib"
    )
}

fn extract_croc_received_text(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();

    for marker in [
        "received text:",
        "text received:",
        "received text ",
        "message:",
        "received message:",
        "received:",
        "text message:",
        "text payload:",
    ] {
        if let Some(pos) = lower.find(marker) {
            let start = pos + marker.len();
            let text = trimmed.get(start..)?.trim().to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }

    if lower.starts_with("text:") {
        let text = trimmed[5..].trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }

    if let Some(stripped) = trimmed.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        let candidate = stripped.trim();
        if !candidate.is_empty() && candidate.len() < 2000 {
            return Some(candidate.to_string());
        }
    }

    None
}

fn extract_croc_received_text_from_logs(log: &[String]) -> Option<String> {
    for (idx, line) in log.iter().enumerate().rev() {
        if let Some(text) = extract_croc_received_text(line) {
            return Some(text);
        }
        let lower = line.to_lowercase();
        if (lower.contains("receiv") && lower.contains("text")) || lower.contains("received text") {
            for next in log.iter().skip(idx + 1).take(6) {
                let candidate = next.trim().trim_matches('"');
                if candidate.is_empty() || candidate.len() > 2000 {
                    continue;
                }
                let c = candidate.to_lowercase();
                let looks_like_status = c.contains("connecting")
                    || c.contains("securing")
                    || c.contains("receiving")
                    || c.contains("sending")
                    || c.contains("code is")
                    || c.contains("croc_secret")
                    || c.contains("download")
                    || c.contains("upload")
                    || c.contains("mb/s")
                    || c.contains("kb/s")
                    || c.contains("error")
                    || c.contains("failed");
                if !looks_like_status {
                    return Some(candidate.to_string());
                }
            }
        }
    }
    None
}

fn normalize_sendme_ticket(text: &str) -> Option<String> {
    let trimmed = text.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        return None;
    }
    // Accept either a raw "blob..." ticket or a full command line like
    // "sendme receive blob...".
    for token in trimmed.split_whitespace() {
        let t = token
            .trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c == ';')
            .trim();
        if t.starts_with("blob")
            && t.len() > 20
            && t.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            return Some(t.to_string());
        }
    }
    None
}

fn is_probable_croc_text_payload_line(line: &str) -> bool {
    let candidate = line.trim().trim_matches('"');
    if candidate.is_empty() || candidate.len() > 2000 {
        return false;
    }
    let lower = candidate.to_lowercase();
    let looks_like_status = lower.contains("connecting")
        || lower.contains("securing")
        || lower.contains("receiving")
        || lower.contains("sending")
        || lower.contains("code is")
        || lower.contains("croc_secret")
        || lower.contains("download")
        || lower.contains("upload")
        || lower.contains("mb/s")
        || lower.contains("kb/s")
        || lower.contains("b/s")
        || lower.contains("error")
        || lower.contains("failed")
        || lower.contains("files")
        || lower.contains("file");
    !looks_like_status
}

fn looks_like_file_drop_payload(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains("file://") {
        return true;
    }
    if !trimmed.contains('\n') && !trimmed.contains('\r') {
        return false;
    }

    for raw in trimmed.lines() {
        let line = raw.trim().trim_matches('"');
        if line.eq_ignore_ascii_case("copy") || line.eq_ignore_ascii_case("cut") {
            return true;
        }
        if PathBuf::from(line).is_absolute() {
            return true;
        }
    }

    false
}

fn parse_file_drop_payload(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim().trim_matches('\0').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.eq_ignore_ascii_case("copy") || line.eq_ignore_ascii_case("cut") {
            continue;
        }
        if line.starts_with("file://") {
            if let Some(path) = path_from_file_uri(line) {
                if path.exists() {
                    out.push(path);
                }
            }
            continue;
        }

        let path = PathBuf::from(line.trim_matches('"'));
        if path.is_absolute() && path.exists() {
            out.push(path);
        }
    }

    out
}

fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    let uri_body = uri
        .strip_prefix("file://")?
        .split('#')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("");
    if uri_body.is_empty() {
        return None;
    }

    #[cfg(target_os = "windows")]
    {
        // file:///C:/path -> C:/path
        // file://localhost/C:/path -> C:/path
        let decoded = percent_decode_uri_component(uri_body);
        let cleaned = decoded.strip_prefix("localhost/").unwrap_or(&decoded);
        let trimmed = cleaned.trim_start_matches('/');
        if trimmed.is_empty() {
            return None;
        }
        return Some(PathBuf::from(trimmed));
    }

    #[cfg(not(target_os = "windows"))]
    {
        // file:///path or file://localhost/path or file://host/path
        let normalized = if uri_body.starts_with('/') {
            uri_body.to_string()
        } else if let Some(rest) = uri_body.strip_prefix("localhost/") {
            format!("/{rest}")
        } else if let Some((_host, rest)) = uri_body.split_once('/') {
            format!("/{rest}")
        } else {
            return None;
        };
        let decoded = percent_decode_uri_component(&normalized);
        if decoded.is_empty() {
            return None;
        }
        return Some(PathBuf::from(decoded));
    }
}

fn percent_decode_uri_component(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h1 = (bytes[i + 1] as char).to_digit(16);
            let h2 = (bytes[i + 2] as char).to_digit(16);
            if let (Some(a), Some(b)) = (h1, h2) {
                out.push(((a << 4) as u8) | (b as u8));
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Compute path size once (cached in SendItem, not called every frame)
fn cached_path_size(path: &std::path::Path) -> Option<u64> {
    if path.is_file() {
        Some(std::fs::metadata(path).map(|m| m.len()).unwrap_or(0))
    } else {
        None
    }
}

/// Recursively compute directory size in a background worker.
fn dir_size_capped(path: &std::path::Path, depth: u32, max_depth: u32) -> u64 {
    if depth > max_depth {
        return 0;
    }
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| {
                    let p = e.path();
                    if p.is_file() {
                        std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0)
                    } else if p.is_dir() {
                        dir_size_capped(&p, depth + 1, max_depth)
                    } else {
                        0
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

// ── Entry Point ────────────────────────────────────────────────────

#[cfg(all(
    feature = "release-single-instance",
    any(target_os = "windows", target_os = "linux")
))]
fn enforce_single_instance_release() -> Option<single_instance::SingleInstance> {
    match single_instance::SingleInstance::new("com.vinaywinai.databeam.release") {
        Ok(instance) => {
            if !instance.is_single() {
                eprintln!("DataBeam is already running; exiting duplicate release instance.");
                std::process::exit(0);
            }
            Some(instance)
        }
        Err(err) => {
            eprintln!("Failed to create single-instance lock: {err}");
            None
        }
    }
}

fn main() -> eframe::Result {
    #[cfg(all(
        feature = "release-single-instance",
        any(target_os = "windows", target_os = "linux")
    ))]
    let _single_instance_guard = enforce_single_instance_release();

        let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([WINDOW_DEFAULT_W, WINDOW_DEFAULT_H])
            .with_min_inner_size([WINDOW_MIN_W, WINDOW_MIN_H])
            .with_title("DataBeam — Secure & Fast Transfer")
            .with_icon(databeam_icon())
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "DataBeam",
        options,
        Box::new(|cc| Ok(Box::new(DataBeamApp::new(cc)))),
    )
}

#[cfg(test)]
mod parse_tests {
    use super::{
        extract_croc_received_text, extract_croc_received_text_from_logs, format_eta_compact,
        normalize_sendme_ticket, parse_croc_bar_frame, parse_croc_file_counter_progress,
        parse_croc_total_size_hint, parse_go_duration, parse_payload_progress,
        parse_sendme_imported_size_hint, parse_sendme_item_index, parse_sendme_total_files_hint,
        parse_stage_progress, AppView, DataBeamApp, SelectedTool, TransferMsg, TransferPhase,
        TransferState,
    };
    use eframe::egui::Color32;
    use std::sync::mpsc;

    #[test]
    fn stage_progress_parses_bracket_with_spinner() {
        let line = "[1/4]⠁ Connecting ... [00:00:00]";
        let p = parse_stage_progress(line).expect("stage progress");
        assert!((p - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn relay_dns_failure_line_matches_resolver_errors() {
        // Exact line shape reported against Quad9 DNS.
        assert!(DataBeamApp::is_relay_dns_failure_line(
            "relay connection failed: could not connect to 3.getcroc.com:9009: 3.getcroc.com:9009: comm.NewConnection failed: dial tcp: lookup 3.getcroc.com: i/o timeout"
        ));
        assert!(DataBeamApp::is_relay_dns_failure_line(
            "dial tcp: lookup 1.getcroc.com: no such host"
        ));
        assert!(DataBeamApp::is_relay_dns_failure_line(
            "temporary failure in name resolution"
        ));
        assert!(!DataBeamApp::is_relay_dns_failure_line(
            "peer error: refusing files"
        ));
        assert!(!DataBeamApp::is_relay_dns_failure_line(
            "Sending (->peer)"
        ));
        assert!(!DataBeamApp::is_relay_dns_failure_line(
            "Process exited with code: exit code: 1"
        ));
    }

    #[test]
    fn payload_progress_parses_mib_ratio() {
        let line = "Downloading ... 8.98 MiB/19.02 MiB 35.34 MiB/s";
        let (done, total) = parse_payload_progress(line).expect("payload progress");
        assert!(done > 0);
        assert!(total > done);
    }

    #[test]
    fn payload_progress_parses_spaced_ratio() {
        let line = "progress 11 MB / 22 MB speed 5 MB/s";
        let (done, total) = parse_payload_progress(line).expect("payload progress");
        assert_eq!(done * 2, total);
    }

    #[test]
    fn payload_progress_parses_mixed_units() {
        let line = "Downloading ... [##>-----] 57.0 MB/6.3 GB 10.08 MiB/s";
        let (done, total) = parse_payload_progress(line).expect("payload progress");
        assert!(done > 50 * 1024 * 1024);
        assert!(total > 6_000_000_000);
        assert!(total > done);
    }

    #[test]
    fn croc_file_counter_progress_parses() {
        let line = "f_99.bin  100% |████████████████████| (16/16 kB, 1.5 MB/s) 300/300";
        let (done, total) = parse_croc_file_counter_progress(line).expect("counter progress");
        assert_eq!(done, 300);
        assert_eq!(total, 300);
    }

    #[test]
    fn croc_file_counter_progress_parses_small_totals() {
        let line = "tiny.bin 100% |████████████████████| (6/6 kB, 1.0 MB/s) 3/3";
        let (done, total) = parse_croc_file_counter_progress(line).expect("counter progress");
        assert_eq!(done, 3);
        assert_eq!(total, 3);
    }

    #[test]
    fn croc_file_counter_progress_ignores_stage_ratio() {
        let line = "[1/4] Getting sizes... [00:00:00]";
        assert!(parse_croc_file_counter_progress(line).is_none());
    }

    #[test]
    fn croc_total_size_hint_parses() {
        // croc's session header uses base-1024 units: 6.4 GB -> 6.4 * 1024^3
        let line = "Sending 57865 files (6.4 GB)";
        let total = parse_croc_total_size_hint(line).expect("croc total");
        assert!(total > 6_800_000_000u64, "unexpected total: {total}");
        assert!(total < 6_900_000_000u64, "unexpected total: {total}");
    }

    #[test]
    fn croc_total_size_hint_uses_binary_units() {
        // Real capture: payload of 62,974,560 bytes printed as "60.1 MB".
        let line = "Receiving 4 files (60.1 MB)";
        let total = parse_croc_total_size_hint(line).expect("croc total");
        assert_eq!(total, 63_019_417);
    }

    #[test]
    fn croc_total_size_hint_ignores_per_file_lines() {
        let line = "sending databeam_QWIXer/readme.txt (6.0 KB)";
        assert!(parse_croc_total_size_hint(line).is_none());
    }

    #[test]
    fn croc_bar_frame_parses_mid_transfer() {
        let line =
            "\rscan641.pdf  56% |███████             | (56.2/600.7 MB, 4.46 MB/s) [1m3s:2m3s]";
        let frame = parse_croc_bar_frame(line).expect("frame");
        assert_eq!(frame.file_name, "scan641.pdf");
        assert_eq!(frame.done_bytes, 56_200_000);
        assert_eq!(frame.total_bytes, 600_700_000);
        assert_eq!(frame.speed_bps, Some(4_460_000.0));
        assert_eq!(frame.eta_secs, Some(123.0));
    }

    #[test]
    fn croc_bar_frame_parses_single_digit_percent() {
        let line = "\rtiny.bin   9% |█                   | (16/16 kB, 1.5 MB/s) [0s:0s]";
        let frame = parse_croc_bar_frame(line).expect("frame");
        assert_eq!(frame.file_name, "tiny.bin");
        assert_eq!(frame.done_bytes, 16_000);
        assert_eq!(frame.total_bytes, 16_000);
        assert_eq!(frame.speed_bps, Some(1_500_000.0));
        assert_eq!(frame.eta_secs, Some(0.0));
    }

    #[test]
    fn croc_bar_frame_final_frame_has_no_brackets() {
        let line = "\rf.bin 100% |████████████████████| (1500/1500 B, 1.0 MB/s)";
        let frame = parse_croc_bar_frame(line).expect("frame");
        assert_eq!(frame.done_bytes, 1500);
        assert_eq!(frame.total_bytes, 1500);
        assert_eq!(frame.speed_bps, Some(1_000_000.0));
        assert_eq!(frame.eta_secs, None);
    }

    #[test]
    fn croc_bar_frame_parses_mixed_suffixes() {
        let line =
            "\rbig.iso  42% |████████            | (210 MB/5 GB, 12.5 MB/s) [4m10s:5m50s]";
        let frame = parse_croc_bar_frame(line).expect("frame");
        assert_eq!(frame.done_bytes, 210_000_000);
        assert_eq!(frame.total_bytes, 5_000_000_000);
        assert_eq!(frame.speed_bps, Some(12_500_000.0));
        assert_eq!(frame.eta_secs, Some(350.0));
    }

    #[test]
    fn croc_bar_frame_without_speed_is_ok() {
        let line = "\rf.bin  10% |██                  | (60.07/600.7 MB)";
        let frame = parse_croc_bar_frame(line).expect("frame");
        assert_eq!(frame.done_bytes, 60_070_000);
        assert_eq!(frame.total_bytes, 600_700_000);
        assert_eq!(frame.speed_bps, None);
        assert_eq!(frame.eta_secs, None);
    }

    #[test]
    fn croc_bar_frame_rejects_non_frames() {
        assert!(parse_croc_bar_frame("").is_none());
        assert!(parse_croc_bar_frame("[1/4] Getting sizes... [00:00:00]").is_none());
        assert!(parse_croc_bar_frame("Sending 2 files (6.4 GB)").is_none());
        assert!(
            parse_croc_bar_frame("n abc r 123/0 i 474 # hash [] 6.7 KiB/6.7 KiB").is_none(),
            "sendme lines must not match"
        );
        assert!(parse_croc_bar_frame("50% off sale |buy now| today").is_none());
    }

    #[test]
    fn go_duration_parses_variants() {
        assert_eq!(parse_go_duration("45s"), Some(45.0));
        assert_eq!(parse_go_duration("2m3s"), Some(123.0));
        assert_eq!(parse_go_duration("1h4m5s"), Some(3845.0));
        assert_eq!(parse_go_duration("1m30.5s"), Some(90.5));
        assert_eq!(parse_go_duration("0s"), Some(0.0));
        assert_eq!(parse_go_duration(""), None);
        assert_eq!(parse_go_duration("00:00:00"), None);
        assert_eq!(parse_go_duration("abc"), None);
    }

    #[test]
    fn eta_formatting_compacts() {
        assert_eq!(format_eta_compact(45.0), "45s");
        assert_eq!(format_eta_compact(123.0), "2m 3s");
        assert_eq!(format_eta_compact(3723.0), "1h 2m");
        assert_eq!(format_eta_compact(0.0), "--");
        assert_eq!(format_eta_compact(-1.0), "--");
    }

    #[test]
    fn sendme_panel_shows_eta_and_files() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Receive;
        app.transfer_speed_bps = Some(100_000.0);
        app.sendme_total_items = Some(5);
        let panel = app.build_sendme_panel_data(Color32::BLUE, 0.5, 500_000, 1_500_000);
        assert_eq!(panel.title, "Receiving\u{2026}");
        assert_eq!(panel.detail_left.as_deref(), Some("5 files"));
        assert_eq!(panel.rate_value, "97.7 KB/s");
        assert_eq!(panel.eta_value, "10s");
    }

    #[test]
    fn average_speed_uses_payload_window() {
        let mut app = DataBeamApp::default();
        app.transfer_payload_start_time = Some(10.0);
        app.transfer_end_time = Some(130.0);
        app.transfer_done_bytes = Some(600_700_000);
        let avg = app.average_transfer_speed_bps().expect("avg speed");
        assert!((avg - 600_700_000.0 / 120.0).abs() < 1.0);

        // Falls back to the session start when no payload timestamp exists.
        let mut app = DataBeamApp::default();
        app.transfer_start_time = Some(0.0);
        app.transfer_end_time = Some(10.0);
        app.transfer_done_bytes = Some(1000);
        assert!((app.average_transfer_speed_bps().unwrap() - 100.0).abs() < 1e-6);

        assert!(DataBeamApp::default().average_transfer_speed_bps().is_none());
    }

    #[test]
    fn croc_frames_accumulate_across_files() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Croc;
        app.view = AppView::Receive;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::Transferring;
        // Real total: 2 x 1536 B = 3072 B, printed base-1024 as "3.0 kB".
        app.update_transfer_metrics_from_log("Receiving 2 files (3.0 kB)");
        assert_eq!(app.transfer_total_bytes, Some(3072));

        app.animation_time = 1.0;
        app.update_transfer_metrics_from_log(
            "\ra.bin  50% |██████████          | (768/1536 B, 1.0 MB/s) [0s:2s]",
        );
        assert_eq!(app.croc_completed_bytes, 0);
        assert_eq!(app.transfer_done_bytes, Some(768));
        assert!((app.transfer_progress - 0.25).abs() < 1e-4);

        app.animation_time = 2.0;
        app.update_transfer_metrics_from_log(
            "\ra.bin 100% |████████████████████| (1536/1536 B, 1.0 MB/s)",
        );

        app.animation_time = 3.0;
        app.update_transfer_metrics_from_log(
            "\rb.bin   5% |█                   | (75/1536 B, 900 kB/s) [0s:2s]",
        );
        assert_eq!(app.croc_completed_bytes, 1536);
        assert_eq!(app.transfer_done_bytes, Some(1611));
        let expected = 1611.0f32 / 3072.0;
        assert!((app.transfer_progress - expected).abs() < 1e-4);
    }

    #[test]
    fn croc_bar_frame_parses_trailing_file_counter() {
        let line = "\ra.txt 100% |████████████████████| (30/30 kB, 11 MB/s) 1/4";
        let frame = parse_croc_bar_frame(line).expect("frame");
        assert_eq!(frame.file_index, Some(1));
        assert_eq!(frame.file_count, Some(4));
        assert_eq!(frame.eta_secs, None);
        assert_eq!(frame.done_bytes, 30_000);
        assert_eq!(frame.total_bytes, 30_000);
    }

    #[test]
    fn croc_frames_fold_duplicate_filenames() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Croc;
        app.view = AppView::Receive;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::Transferring;
        app.transfer_total_bytes = Some(90_000);

        app.animation_time = 1.0;
        app.update_transfer_metrics_from_log(
            "\rdup.txt   0% |                    | ( 0 B/15 kB) [0s:0s]",
        );
        app.animation_time = 2.0;
        app.update_transfer_metrics_from_log(
            "\rdup.txt 100% |████████████████████| (15/15 kB, 5.7 MB/s) 1/2",
        );
        assert_eq!(app.croc_completed_bytes, 0);
        assert_eq!(app.croc_file_progress, Some((1, 2)));

        // Second file with the SAME name: its blank 0% frame must trigger the fold.
        app.animation_time = 3.0;
        app.update_transfer_metrics_from_log(
            "\rdup.txt   0% |                    | ( 0 B/15 kB) [0s:0s]",
        );
        assert_eq!(app.croc_completed_bytes, 15_000);

        app.animation_time = 4.0;
        app.update_transfer_metrics_from_log(
            "\rdup.txt 100% |████████████████████| (15/15 kB, 5.7 MB/s) 2/2",
        );
        assert_eq!(app.croc_completed_bytes, 15_000);
        assert_eq!(app.transfer_done_bytes, Some(30_000));
        assert_eq!(app.croc_file_progress, Some((2, 2)));
    }

    #[test]
    fn croc_panel_shows_finalizing_after_frame_silence() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Croc;
        app.view = AppView::Receive;
        app.croc_bar_file = Some("big.bin".to_string());
        app.croc_last_frame_at = Some(10.0);

        app.animation_time = 11.0;
        let panel = app.build_croc_panel_data(Color32::BLUE, 0.5, 50, 100);
        assert!(!panel.title.contains("Finalizing"));

        app.animation_time = 20.0;
        let panel = app.build_croc_panel_data(Color32::BLUE, 0.5, 50, 100);
        assert!(panel.title.contains("Finalizing"));
        assert_eq!(panel.eta_value, "--");
        assert_eq!(panel.rate_value, "--");
    }

    #[test]
    fn sendme_imported_size_hint_parses() {
        let line = "imported directory /tmp/sample, 6.4 GiB, hash abcdef";
        let total = parse_sendme_imported_size_hint(line).expect("sendme imported size");
        assert!(total > 6 * 1024 * 1024 * 1024);
    }

    #[test]
    fn sendme_total_files_hint_parses() {
        let line = "importing 500 files [00:00:00]";
        let total = parse_sendme_total_files_hint(line).expect("total files");
        assert_eq!(total, 500);
    }

    #[test]
    fn sendme_item_index_parses() {
        let line = "n abc r 123/0 i 474 # hash [] 6.7 KiB/6.7 KiB";
        let idx = parse_sendme_item_index(line).expect("item index");
        assert_eq!(idx, 475);
    }

    #[test]
    fn croc_waiting_ignores_per_file_progress_lines() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Croc;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::WaitingForReceiver;
        app.transfer_total_bytes = Some((6.4_f64 * 1024.0 * 1024.0 * 1024.0) as u64);

        app.update_transfer_metrics_from_log("file_download.py 2628/57865");

        assert_eq!(app.transfer_phase, TransferPhase::WaitingForReceiver);
        assert!(app.transfer_done_bytes.is_none());
        assert!(app.transfer_progress <= f32::EPSILON);
    }

    #[test]
    fn croc_switches_to_transferring_on_peer_line_then_uses_file_counter_progress() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Croc;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::WaitingForReceiver;

        app.update_transfer_metrics_from_log("Sending 57865 files (6.4 GB)");
        assert_eq!(app.transfer_phase, TransferPhase::WaitingForReceiver);
        let total = app
            .transfer_total_bytes
            .expect("total bytes from summary line");
        assert_eq!(total, 6_871_947_673);

        app.update_transfer_metrics_from_log("Sending (->192.168.1.60:56052)");
        assert_eq!(app.transfer_phase, TransferPhase::Transferring);

        // Before any progressbar frame arrives, the file-counter model drives
        // overall progress.
        app.update_transfer_metrics_from_log("file_download.py 2628/57865");

        let p = app.effective_progress();
        assert!(p > 0.04 && p < 0.05, "unexpected overall progress: {p}");
        let done = app.transfer_done_bytes.expect("derived done bytes");
        assert!(done > 200 * 1024 * 1024, "unexpected done bytes: {done}");
    }

    #[test]
    fn croc_frame_progress_supersedes_file_counter_model() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Croc;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::Transferring;
        app.croc_file_progress = Some((2628, 57865));
        app.transfer_total_bytes = Some(6_400_000_000);

        app.animation_time = 1.0;
        app.update_transfer_metrics_from_log(
            "\rfile_download.py  56% |███████             | (3.4/6.1 GB, 44.6 MB/s) [1m3s:1m13s]",
        );

        assert_eq!(app.croc_bar_done, 3_400_000_000);
        assert_eq!(app.transfer_speed_bps, Some(44_600_000.0));
        let p = app.effective_progress();
        let expected = 3_400_000_000f32 / 6_400_000_000.0;
        assert!((p - expected).abs() < 1e-4, "unexpected progress: {p}");
    }

    #[test]
    fn sendme_waiting_ignores_zero_payload_lines_on_sender() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::WaitingForReceiver;
        app.transfer_total_bytes = Some((6.4_f64 * 1024.0 * 1024.0 * 1024.0) as u64);

        app.update_transfer_metrics_from_log("Downloading ... [----] 0 B/6.4 GB 0 B/s");

        assert_eq!(app.transfer_phase, TransferPhase::WaitingForReceiver);
        assert!(app.transfer_done_bytes.is_none());
        assert!(app.transfer_progress <= f32::EPSILON);
    }

    #[test]
    fn sendme_waiting_switches_to_transferring_when_payload_bytes_appear() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::WaitingForReceiver;
        app.transfer_total_bytes = Some((6.4_f64 * 1024.0 * 1024.0 * 1024.0) as u64);

        app.update_transfer_metrics_from_log(
            "Downloading ... [##>------] 168.6 MB/6.4 GB 10.0 MB/s",
        );

        assert_eq!(app.transfer_phase, TransferPhase::Transferring);
        let done = app.transfer_done_bytes.expect("done bytes");
        assert!(done > 150 * 1024 * 1024);
    }

    #[test]
    fn sendme_waiting_switches_to_transferring_on_downloading_stage_zero_bytes() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::WaitingForReceiver;
        app.transfer_total_bytes = Some((6.4_f64 * 1024.0 * 1024.0 * 1024.0) as u64);

        app.update_transfer_metrics_from_log(
            "[3/4] Downloading ... [------------------------] 0 B/6.4 GB 0 B/s",
        );

        assert_eq!(app.transfer_phase, TransferPhase::Transferring);
        assert_eq!(app.transfer_done_bytes.unwrap_or(0), 0);
    }

    #[test]
    fn sendme_receive_ignores_non_downloading_payload_lines() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Receive;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::Transferring;
        app.transfer_total_bytes = Some((6.8_f64 * 1024.0 * 1024.0 * 1024.0) as u64);

        app.update_transfer_metrics_from_log(
            "n 99549f9de3 r 31724666896/0 i 0 # f706408fbb [] 15.66 KiB/15.66 KiB",
        );

        assert!(app.transfer_done_bytes.is_none());
        assert!(app.transfer_progress <= f32::EPSILON);
    }

    #[test]
    fn sendme_receive_progress_is_monotonic() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Receive;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::Transferring;
        app.transfer_total_bytes = Some((6.85_f64 * 1024.0 * 1024.0 * 1024.0) as u64);

        app.update_transfer_metrics_from_log(
            "Downloading ... [00:05:49] [###########>--] 5.73 GiB/6.85 GiB 20.08 MiB/s",
        );
        let first_done = app.transfer_done_bytes.expect("first done");

        app.update_transfer_metrics_from_log(
            "Downloading ... [00:05:50] [##########>---] 4.60 GiB/6.85 GiB 12.00 MiB/s",
        );
        let second_done = app.transfer_done_bytes.expect("second done");

        assert!(second_done >= first_done);
    }

    #[test]
    fn croc_text_marker_without_colon_parses() {
        let line = "received text hello from croc";
        let text = extract_croc_received_text(line).expect("text parsed");
        assert_eq!(text, "hello from croc");
    }

    #[test]
    fn croc_text_fallback_from_log_sequence_parses() {
        let log = vec![
            "Connecting ...".to_string(),
            "Receiving 'text'".to_string(),
            "hello popup".to_string(),
        ];
        let text = extract_croc_received_text_from_logs(&log).expect("fallback text");
        assert_eq!(text, "hello popup");
    }

    #[test]
    fn normalize_sendme_ticket_parses_raw_and_command() {
        let raw = "blobabc123def456ghi789jkl";
        assert_eq!(
            normalize_sendme_ticket(raw).as_deref(),
            Some("blobabc123def456ghi789jkl")
        );

        let command = "sendme receive blobabc123def456ghi789jkl";
        assert_eq!(
            normalize_sendme_ticket(command).as_deref(),
            Some("blobabc123def456ghi789jkl")
        );
    }

    #[test]
    fn sendme_waiting_switches_to_transferring_on_n_line_output() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::WaitingForReceiver;
        app.transfer_code = Some("blobxyz".to_string());
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        // This test simulates output from a command, but doesn't actually run one.
        // The `cmd.current_dir` change is for actual command execution, not this test setup.
        // Therefore, no change is needed here.
        tx.send(TransferMsg::Output(
            "n 99549f9de3 r 31724666896/0 i 0 # f706408fbb".to_string(),
        ))
        .expect("send output");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.transfer_phase, TransferPhase::Transferring);
    }

    #[test]
    fn sendme_sender_activity_switches_to_transferring() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.sendme_one_shot = true;
        app.transfer_phase = TransferPhase::WaitingForReceiver;
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::SenderTransferActivity)
            .expect("send activity");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.transfer_phase, TransferPhase::Transferring);
        assert!(app.sendme_had_transfer);
    }

    #[test]
    fn sendme_one_shot_incomplete_on_early_peer_disconnect() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.sendme_one_shot = true;
        app.transfer_phase = TransferPhase::Transferring;
        app.sendme_had_transfer = true;
        app.transfer_total_bytes = Some(4096);
        app.transfer_done_bytes = Some(2048); // 50% — early disconnect
        app.transfer_progress = 0.5;
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::PeerDisconnected)
            .expect("send peer disconnected");
        drop(tx);

        app.poll_transfer();

        assert!(
            matches!(app.transfer_state, TransferState::Failed(_)),
            "expected Failed for 50% progress disconnect, got {:?}",
            app.transfer_state
        );
    }

    #[test]
    fn sendme_one_shot_completes_on_peer_disconnected_signal() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.sendme_one_shot = true;
        app.transfer_phase = TransferPhase::Transferring;
        app.sendme_had_transfer = true;
        app.transfer_total_bytes = Some(4096);
        app.transfer_done_bytes = Some(4096); // 100% — normal completion
        app.transfer_progress = 1.0;
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::PeerDisconnected)
            .expect("send peer disconnected");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.transfer_state, TransferState::Completed);
    }

    #[test]
    fn eazy_local_check_watch_stays_armed_during_verification_silence() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::EazySendme;
        app.view = AppView::Receive;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::Transferring;
        app.eazy_local_check_started_at = Some(12.0);
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::Output(
            "[Local] still checking cached blobs".to_string(),
        ))
        .expect("send output");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.eazy_local_check_started_at, Some(12.0));
    }

    #[test]
    fn eazy_local_check_watch_clears_when_local_export_starts() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::EazySendme;
        app.view = AppView::Receive;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::Transferring;
        app.eazy_local_check_started_at = Some(12.0);
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::Output(
            "[local] Blobs complete locally, exporting...".to_string(),
        ))
        .expect("send output");
        drop(tx);

        app.poll_transfer();

        assert!(app.eazy_local_check_started_at.is_none());
    }

    #[test]
    fn eazy_local_check_watch_clears_on_local_export_progress() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::EazySendme;
        app.view = AppView::Receive;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::Transferring;
        app.eazy_local_check_started_at = Some(12.0);
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::Progress(0.25))
            .expect("send progress");
        drop(tx);

        app.poll_transfer();

        assert!(app.eazy_local_check_started_at.is_none());
    }

    /// Guards `DATABEAM_SETTINGS_FILE` so a panic cannot leak the override
    /// into other tests running in the same process.
    struct SettingsFileGuard;
    impl SettingsFileGuard {
        fn set(path: &std::path::Path) -> Self {
            std::env::set_var("DATABEAM_SETTINGS_FILE", path);
            SettingsFileGuard
        }
    }
    impl Drop for SettingsFileGuard {
        fn drop(&mut self) {
            std::env::remove_var("DATABEAM_SETTINGS_FILE");
        }
    }

    #[test]
    fn pick_restore_hwnd_prefers_startup_capture() {
        use super::DataBeamApp;
        // Startup capture always wins: it is taken unconditionally, while the
        // cached one only exists when a prior tray init ran. Toggle-time init
        // with no prior init used to pass None (silent dead restores).
        assert_eq!(DataBeamApp::pick_restore_hwnd(Some(123), Some(456)), Some(123));
        assert_eq!(DataBeamApp::pick_restore_hwnd(Some(123), None), Some(123));
        assert_eq!(DataBeamApp::pick_restore_hwnd(None, Some(456)), Some(456));
        assert_eq!(DataBeamApp::pick_restore_hwnd(None, None), None);
        // Default has no capture yet (filled in new()).
        assert_eq!(DataBeamApp::default().startup_hwnd, None);
    }

    #[test]
    fn restore_size_override_only_fires_below_minimum() {
        use super::restore_size_override;
        // Healthy sizes (default, user-resized, exactly minimum): untouched.
        assert_eq!(restore_size_override([620.0, 820.0], [800.0, 600.0]), None);
        assert_eq!(restore_size_override([800.0, 600.0], [800.0, 600.0]), None);
        assert_eq!(restore_size_override([460.0, 400.0], [620.0, 820.0]), None);
        // Shrunken restore states: forced to last good.
        assert_eq!(
            restore_size_override([160.0, 31.0], [620.0, 820.0]),
            Some([620.0, 820.0])
        );
        assert_eq!(
            restore_size_override([0.0, 0.0], [800.0, 600.0]),
            Some([800.0, 600.0])
        );
        assert_eq!(
            restore_size_override([620.0, 399.0], [620.0, 820.0]),
            Some([620.0, 820.0])
        );
    }

    #[test]
    fn tray_toggle_setter_drives_init_drop_and_persist() {
        // Redirect settings so this test never touches real user data.
        // Regression test: the checkbox must go through the setter with the
        // not-yet-applied value. Binding `&mut app.minimize_to_tray`
        // directly flips the field first, making the setter's changed-guard
        // always true — toggle ON then silently never initialized the tray.
        let dir = std::env::temp_dir().join(format!(
            "databeam-test-settings-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test settings dir");
        let file = dir.join("settings.json");
        let _guard = SettingsFileGuard::set(&file);

        let mut app = DataBeamApp::default();
        assert!(app.minimize_to_tray);
        let ctx = eframe::egui::Context::default();

        // OFF: flips the field and persists (no icon was ever created).
        // May briefly show a tray icon on machines with a tray; it is
        // dropped with `app` at test end.
        app.set_minimize_to_tray(&ctx, false);
        assert!(!app.minimize_to_tray);
        assert!(app.tray_state.is_none());
        let raw = std::fs::read_to_string(&file).expect("settings written");
        assert!(raw.contains("\"minimize_to_tray\": false"));

        // ON: flips back and ATTEMPTS init (graceful None headless).
        app.set_minimize_to_tray(&ctx, true);
        assert!(app.minimize_to_tray);

        // Same value: no-op, still on.
        app.set_minimize_to_tray(&ctx, true);
        assert!(app.minimize_to_tray);

        // Persisted file reflects the final value.
        let raw = std::fs::read_to_string(&file).expect("settings written");
        assert!(raw.contains("\"minimize_to_tray\": true"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
