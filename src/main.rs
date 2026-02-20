#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod backend;
mod theme;
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
struct UserSettings {
    #[serde(default)]
    croc_recent_codes: Vec<String>,
    #[serde(default)]
    selected_tool: Option<String>,
    #[serde(default)]
    receive_output_dir: Option<String>,
    #[serde(default = "default_sendme_one_shot")]
    sendme_one_shot: bool,
    #[serde(skip)]
    eazysendme_custom_code: String,
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
    croc_show_qr: bool,
    croc_text_mode: bool,
    croc_text_value: String,

    // Receive
    receive_code: String,
    receive_output_dir: Option<PathBuf>,

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
    croc_received_text: Option<String>,
    croc_expect_text_payload: bool,
    transfer_phase: TransferPhase,
    preparing_progress: f32,
    transfer_payload_start_time: Option<f64>,
    transfer_end_time: Option<f64>,
    croc_qr_popup_open: bool,
    croc_text_popup_open: bool,

    // EazySendme
    eazysendme_custom_code: String,
    eazysendme_ticket: Option<String>,
    eazysendme_croc_handle: Option<ProcessHandle>,
    eazysendme_croc_rx: Option<mpsc::Receiver<TransferMsg>>,

    // UI
    toast_msg: Option<(String, f64, Color32)>,
    animation_time: f64,
    drag_hover: bool,
    picker_block_until: f64,
    picker_in_flight: bool,
    sendme_one_shot: bool,
    sendme_peer_connected: bool,
    sendme_had_transfer: bool,
    sendme_waiting_after_cycle: bool,
    sendme_last_activity: Option<f64>,
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
            croc_show_qr: false,
            croc_text_mode: false,
            croc_text_value: String::new(),
            receive_code: String::new(),
            receive_output_dir: None,
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
            croc_received_text: None,
            croc_expect_text_payload: false,
            transfer_phase: TransferPhase::Preparing,
            preparing_progress: 0.0,
            transfer_payload_start_time: None,
            transfer_end_time: None,
            croc_qr_popup_open: false,
            croc_text_popup_open: false,
            toast_msg: None,
            animation_time: 0.0,
            drag_hover: false,
            picker_block_until: 0.0,
            picker_in_flight: false,
            sendme_one_shot: true,
            sendme_peer_connected: false,
            sendme_had_transfer: false,
            sendme_waiting_after_cycle: false,
            sendme_last_activity: None,
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
            eazysendme_ticket: None,
            eazysendme_croc_handle: None,
            eazysendme_croc_rx: None,
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

        let sendme_available = app
            .tool_statuses
            .iter()
            .any(|s| s.tool == Tool::Sendme && s.available);
        let croc_available = app
            .tool_statuses
            .iter()
            .any(|s| s.tool == Tool::Croc && s.available);
        match app.selected_tool {
            SelectedTool::Sendme if sendme_available => {}
            SelectedTool::Croc if croc_available => {}
            _ if sendme_available => app.selected_tool = SelectedTool::Sendme,
            _ if croc_available => app.selected_tool = SelectedTool::Croc,
            _ => {}
        }

        app
    }

    fn settings_file_path() -> Option<PathBuf> {
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

        let mut deduped = Vec::new();
        for code in settings.croc_recent_codes {
            let clean = code.trim().to_string();
            if clean.chars().count() <= 6 || deduped.iter().any(|c| c == &clean) {
                continue;
            }
            deduped.push(clean);
            if deduped.len() >= 5 {
                break;
            }
        }
        self.croc_recent_codes = deduped;
        if let Some(tool) = settings.selected_tool {
            let lower = tool.to_lowercase();
            if lower == "croc" {
                self.selected_tool = SelectedTool::Croc;
            } else if lower == "sendme" {
                self.selected_tool = SelectedTool::Sendme;
            }
        }
        self.receive_output_dir = settings
            .receive_output_dir
            .as_ref()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty());
        self.sendme_one_shot = settings.sendme_one_shot;
        self.eazysendme_custom_code = settings.eazysendme_custom_code;
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
            selected_tool: Some(match self.selected_tool {
                SelectedTool::Croc => "croc".to_string(),
                SelectedTool::Sendme => "sendme".to_string(),
                SelectedTool::EazySendme => "eazysendme".to_string(),
            }),
            receive_output_dir: self
                .receive_output_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            sendme_one_shot: self.sendme_one_shot,
            eazysendme_custom_code: self.eazysendme_custom_code.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let _ = fs::write(path, json);
        }
    }

    fn get_tool_binary(&self, tool: &Tool) -> Option<String> {
        resolve_tool_path(tool, &self.tool_statuses)
    }

    fn engine_color(&self) -> Color32 {
        match self.selected_tool {
            SelectedTool::Croc => CROC_COLOR,
            SelectedTool::Sendme => SENDME_COLOR,
            SelectedTool::EazySendme => Color32::from_rgb(255, 165, 0), // Orange-ish
        }
    }

    fn effective_receive_folder(&self) -> Option<PathBuf> {
        self.receive_output_dir
            .clone()
            .or_else(|| std::env::current_dir().ok())
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
        self.croc_received_text = None;
        self.croc_expect_text_payload = false;
        self.transfer_phase = TransferPhase::Preparing;
        self.preparing_progress = 0.0;
        self.transfer_payload_start_time = None;
        self.transfer_end_time = None;
        self.croc_qr_popup_open = false;
        self.eazysendme_ticket = None;
        self.eazysendme_croc_handle = None;
        self.eazysendme_croc_rx = None;
        self.croc_text_popup_open = false;
        self.sendme_peer_connected = false;
        self.sendme_had_transfer = false;
        self.sendme_waiting_after_cycle = false;
        self.sendme_last_activity = None;
        self.picker_block_until = 0.0;
        self.picker_in_flight = false;
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
        self.sendme_sender_payload_complete = false;
        if let Some(handle) = &self.transfer_handle {
            handle.request_cancel();
        }
    }

    fn cancel_transfer(&mut self) {
        if let Some(handle) = &self.transfer_handle {
            handle.request_cancel();
        }
        self.transfer_end_time = Some(self.animation_time);
        self.transfer_state = TransferState::Failed("Transfer cancelled".to_string());
    }

    fn retry_send(&mut self) {
        if let Some(handle) = &self.transfer_handle {
            handle.request_cancel();
        }
        self.reset_transfer();
        self.start_send();
    }

    fn retry_receive(&mut self) {
        if let Some(handle) = &self.transfer_handle {
            handle.request_cancel();
        }
        self.reset_transfer();
        self.start_receive();
    }

    fn update_transfer_metrics_from_log(&mut self, line: &str) {
        let lower = line.to_lowercase();
        let eazy_receive_pre_ticket = self.selected_tool == SelectedTool::EazySendme
            && self.view == AppView::Receive
            && self.eazysendme_ticket.is_none();
        if eazy_receive_pre_ticket {
            return;
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
        if is_cli_progress_line(line) || sendme_raw_progress_line {
            if !(self.selected_tool == SelectedTool::Croc && croc_counter_any.is_some()) {
                self.latest_cli_progress_line = Some(compact_cli_progress_line(line));
            }
        }
        if self.selected_tool == SelectedTool::Croc
            && self.transfer_phase != TransferPhase::Transferring
            && (lower.contains("sending (->") || lower.contains("receiving (<-"))
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
            if self.view == AppView::Send {
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
                            // Progress reset for a reused item slot: count prior file as complete,
                            // then start accumulating the next file from its new done value.
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
                    // Fallback for noisy/truncated sender lines that miss `i <idx>` tokens:
                    // treat a done drop as rollover into the next file in a sequential stream.
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
                        let stream_done =
                            if let (Some(cur_done), Some(cur_total)) =
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
                        let est_done = self
                            .sendme_done_bytes_est
                            .max(stream_done)
                            .min(total_bytes);
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
                if let Some(text) = extract_croc_received_text_from_logs(&self.transfer_log) {
                    self.croc_received_text = Some(text);
                    // self.croc_text_popup_open = true; // Don't pop up for EazySendme, just use it
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

    fn poll_transfer(&mut self) {
        // Take the receiver out to satisfy borrow checker
        if let Some(rx) = self.transfer_rx.take() {
            let mut processed = 0usize;
            let mut restart_sendme_serve = false;
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
                                let sendme_counter_activity =
                                    parse_sendme_r_counter(&line).is_some();
                                if self.transfer_state == TransferState::Running {
                                    self.update_transfer_metrics_from_log(&line);
                                }
                                if (self.selected_tool == SelectedTool::Sendme
                                    || self.selected_tool == SelectedTool::EazySendme)
                                    && self.view == AppView::Send
                                    && self.transfer_state == TransferState::Running
                                    && self.sendme_had_transfer
                                    && self.sendme_sender_payload_complete
                                {
                                    if self.sendme_one_shot {
                                        self.mark_sendme_one_shot_completed();
                                    } else {
                                        self.mark_sendme_sender_waiting();
                                    }
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
                                        if self.sendme_had_transfer {
                                            if self.sendme_one_shot {
                                                self.mark_sendme_one_shot_completed();
                                            } else {
                                                self.mark_sendme_sender_waiting();
                                            }
                                        } else if self.transfer_phase == TransferPhase::Preparing {
                                            self.transfer_phase = TransferPhase::WaitingForReceiver;
                                        }
                                    }
                                    if (self.selected_tool == SelectedTool::Sendme
                                        || self.selected_tool == SelectedTool::EazySendme)
                                        && lower.contains("to get this data, use")
                                        && self.sendme_had_transfer
                                    {
                                        if self.sendme_one_shot {
                                            self.mark_sendme_one_shot_completed();
                                        } else {
                                            self.mark_sendme_sender_waiting();
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
                                        && (lower.contains("sending (->")
                                            || lower.contains("receiving (<-"))
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

                                    if sendme_sender
                                        && self.sendme_one_shot
                                        && sendme_cycle_finished
                                    {
                                        self.mark_sendme_one_shot_completed();
                                    } else if sendme_sender
                                        && !self.sendme_one_shot
                                        && sendme_cycle_finished
                                    {
                                        self.mark_sendme_sender_waiting();
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
                                if self.selected_tool == SelectedTool::Croc
                                    && self.view == AppView::Send
                                    && self.croc_show_qr
                                    && self.transfer_code.is_some()
                                {
                                    self.croc_qr_popup_open = true;
                                }
                                if self.selected_tool == SelectedTool::Sendme
                                    && self.view == AppView::Send
                                    && self.transfer_phase == TransferPhase::Preparing
                                {
                                    self.transfer_phase = TransferPhase::WaitingForReceiver;
                                    self.sendme_waiting_after_cycle = false;
                                }
                            }
                            TransferMsg::WaitingForReceiver => {
                                let sendme_sender = (self.selected_tool == SelectedTool::Sendme
                                    || self.selected_tool == SelectedTool::EazySendme)
                                    && self.view == AppView::Send
                                    && self.transfer_state == TransferState::Running;
                                if sendme_sender {
                                    if self.sendme_had_transfer {
                                        if self.sendme_one_shot {
                                            self.mark_sendme_one_shot_completed();
                                        } else {
                                            self.mark_sendme_sender_waiting();
                                        }
                                    } else {
                                        self.transfer_phase = TransferPhase::WaitingForReceiver;
                                        self.sendme_waiting_after_cycle = false;
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
                            TransferMsg::Completed => {
                                self.transfer_end_time = Some(self.animation_time);
                                if self.selected_tool == SelectedTool::Croc
                                    && self.view == AppView::Send
                                    && self.transfer_code.is_none()
                                {
                                    self.transfer_state = TransferState::Failed(
                                        "Croc send ended before starting. Check options and retry."
                                            .to_string(),
                                    );
                                } else {
                                    if self.selected_tool == SelectedTool::EazySendme
                                        && self.view == AppView::Receive
                                        && self.transfer_phase == TransferPhase::Preparing
                                        && self.transfer_rx.is_none()
                                    {
                                        // Receiver side: Croc finished receiving the ticket
                                        if let Some(ticket) = self.croc_received_text.clone() {
                                            self.transfer_log.push(
                                                "Ticket received via Croc. Starting Sendme..."
                                                    .to_string(),
                                            );
                                            let normalized_ticket =
                                                normalize_sendme_ticket(&ticket).unwrap_or(ticket);
                                            self.eazysendme_ticket =
                                                Some(normalized_ticket.clone());

                                            // Start sendme_receive
                                            if let Some(binary) =
                                                self.get_tool_binary(&Tool::Sendme)
                                            {
                                                let opts = SendmeReceiveOptions {
                                                    ticket: normalized_ticket,
                                                    output_dir: self.receive_output_dir.clone(),
                                                };
                                                let (new_rx, new_handle) =
                                                    sendme_receive(opts, &binary);
                                                self.transfer_rx = Some(new_rx);
                                                self.transfer_handle = Some(new_handle);
                                                self.transfer_phase = TransferPhase::Preparing;
                                                self.transfer_start_time =
                                                    Some(self.animation_time);
                                                return; // Exit poll_transfer, will resume next frame with new_rx
                                            } else {
                                                self.transfer_state = TransferState::Failed(
                                                    "Sendme binary not found".to_string(),
                                                );
                                            }
                                        } else {
                                            self.transfer_state = TransferState::Failed(
                                                "Croc finished but no ticket found".to_string(),
                                            );
                                        }
                                    } else {
                                        self.transfer_state = TransferState::Completed;
                                        self.transfer_progress = 1.0;
                                        self.preparing_progress = 1.0;
                                        if let Some(total) = self.transfer_total_bytes {
                                            self.transfer_done_bytes = Some(total);
                                        }
                                    }
                                }
                            }
                            TransferMsg::PeerDisconnected => {
                                if self.transfer_state == TransferState::Completed {
                                    continue;
                                }
                                self.transfer_log
                                    .push("Peer disconnected (transfer finished)".to_string());
                                if (self.selected_tool == SelectedTool::Sendme
                                    || self.selected_tool == SelectedTool::EazySendme)
                                    && self.sendme_one_shot
                                {
                                    let done = self.transfer_done_bytes.unwrap_or(0);
                                    let has_payload = done > 0 || self.sendme_had_transfer;
                                    if has_payload {
                                        self.mark_sendme_one_shot_completed();
                                    } else {
                                        self.transfer_state = TransferState::Failed(
                                            "Peer disconnected before transfer started".to_string(),
                                        );
                                        self.transfer_end_time = Some(self.animation_time);
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
                                    }
                                }
                            }
                            TransferMsg::Error(e) => {
                                if self.transfer_state == TransferState::Completed {
                                    // Ignore late process-shutdown errors after completion.
                                } else if e == "Transfer cancelled" {
                                    // Keep explicit completion from caller paths; otherwise stay idle-ish failed.
                                    if self.transfer_state != TransferState::Completed {
                                        self.transfer_state =
                                            TransferState::Failed("Transfer cancelled".to_string());
                                        self.transfer_end_time = Some(self.animation_time);
                                    }
                                } else if e == "Send session ended before transfer completed"
                                    && (self.selected_tool == SelectedTool::Sendme
                                        || self.selected_tool == SelectedTool::EazySendme)
                                    && self.sendme_one_shot
                                    && (self.sendme_had_transfer
                                        || self.transfer_done_bytes.unwrap_or(0) > 0)
                                {
                                    let done = self.transfer_done_bytes.unwrap_or(0);
                                    let has_payload = done > 0 || self.sendme_had_transfer;
                                    if has_payload {
                                        self.mark_sendme_one_shot_completed();
                                    } else {
                                        self.transfer_state = TransferState::Failed(
                                            "Send session ended before payload started".to_string(),
                                        );
                                        self.transfer_end_time = Some(self.animation_time);
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
                                } else {
                                    self.transfer_state = TransferState::Failed(e);
                                    self.transfer_end_time = Some(self.animation_time);
                                }
                            }
                            TransferMsg::Started => {
                                self.transfer_state = TransferState::Running;
                                self.transfer_start_time = Some(self.animation_time);
                                self.transfer_phase = if self.selected_tool == SelectedTool::Sendme
                                    && self.view == AppView::Send
                                    && !self.sendme_one_shot
                                {
                                    TransferPhase::Preparing
                                } else if self.selected_tool == SelectedTool::Sendme
                                    && self.view == AppView::Receive
                                {
                                    TransferPhase::Preparing
                                } else if self.selected_tool == SelectedTool::EazySendme {
                                    TransferPhase::Preparing
                                } else {
                                    TransferPhase::Preparing
                                };
                                self.preparing_progress = 0.0;
                                self.transfer_payload_start_time = None;
                                self.transfer_end_time = None;
                                self.transfer_speed_bps = None;
                                self.transfer_speed_samples.clear();
                                self.transfer_done_bytes = None;
                                self.sendme_had_transfer = false;
                                self.sendme_waiting_after_cycle = false;
                            }
                        }
                    }
                    Err(_) => break, // Empty or disconnected
                }
            }
            if restart_sendme_serve {
                self.start_send();
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
                                self.transfer_log.push(format!("[Croc] {}", line));
                                if self.transfer_log.len() > 500 {
                                    self.transfer_log.drain(0..100);
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

        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        let dropped = if dropped.is_empty() {
            ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::Paste(s) if s.contains("file://") => Some(s.as_str()),
                        _ => None,
                    })
                    .flat_map(parse_file_uri_list)
                    .collect::<Vec<PathBuf>>()
            })
        } else {
            dropped
        };

        if !dropped.is_empty() {
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

    fn start_send(&mut self) {
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
                let wants_custom =
                    self.croc_use_custom_code || !self.croc_custom_code.trim().is_empty();
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
                    if self.croc_show_qr {
                        self.croc_qr_popup_open = true;
                    }
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
                };
                let (rx, handle) = sendme_send(opts, &binary);
                self.transfer_rx = Some(rx);
                self.transfer_handle = Some(handle);
            }
            SelectedTool::EazySendme => {
                if self.eazysendme_custom_code.len() < 7 {
                    self.show_toast("Code must be at least 7 characters".to_string(), WARNING);
                    return;
                }
                // Step 1: Start sendme_send to get a ticket
                // EazySendme MUST be one-shot because tickets are single-use.
                self.sendme_one_shot = true;
                self.transfer_phase = TransferPhase::Preparing;

                let opts = SendmeSendOptions {
                    paths: self.send_paths(),
                    one_shot: true,
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

    fn start_receive(&mut self) {
        if self.receive_code.trim().is_empty() {
            self.show_toast("Enter a code or ticket".to_string(), WARNING);
            return;
        }

        self.reset_transfer();

        let binary = match self.selected_tool {
            SelectedTool::Croc | SelectedTool::EazySendme => match self.get_tool_binary(&Tool::Croc)
            {
                Some(b) => b,
                None => {
                    self.show_toast("Croc not found".to_string(), ERROR);
                    return;
                }
            },
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
                };
                let (rx, handle) = sendme_receive(opts, &binary);
                self.transfer_rx = Some(rx);
                self.transfer_handle = Some(handle);
            }
            SelectedTool::EazySendme => {
                // Step 1: Start croc_receive to get the ticket
                let opts = CrocReceiveOptions {
                    code: self.receive_code.trim().to_string(),
                    output_dir: None, // Ticket is text, no dir needed
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
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            "Drop files here",
            egui::FontId::new(20.0, egui::FontFamily::Proportional),
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
        if !self.initialized_once {
            self.view = AppView::Home;
            self.initialized_once = true;
            self.show_toast(
                format!("Running DataBeam v{} (Fixed Build)", APP_VERSION),
                SUCCESS,
            );
        }

        if let Some((_, ref mut start, _)) = self.toast_msg {
            if *start == 0.0 {
                *start = self.animation_time;
            }
        }

        self.handle_dropped_files(ctx);
        self.poll_size_updates();
        self.poll_transfer();

        if self.transfer_state == TransferState::Running {
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

                        for (label, view) in &[
                            ("🏠 Home", AppView::Home),
                            ("📤 Send", AppView::Send),
                            ("📥 Receive", AppView::Receive),
                        ] {
                            let active = self.view == *view;
                            let c = if active {
                                Color32::BLACK
                            } else {
                                TEXT_SECONDARY
                            };
                            if ui
                                .selectable_label(active, RichText::new(*label).size(12.0).color(c))
                                .clicked()
                                && self.view != *view
                            {
                                self.view = *view;
                                // Clear state to prevent confusion
                                self.send_items.clear();
                                self.receive_code.clear();
                                self.reset_transfer();
                            }
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (tc, tn) = match self.selected_tool {
                            SelectedTool::Croc => (CROC_COLOR, "🐊 croc"),
                            SelectedTool::Sendme => (SENDME_COLOR, "📡 sendme"),
                            SelectedTool::EazySendme => {
                                (Color32::from_rgb(255, 165, 0), "⚡ eazysendme")
                            }
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
                            RichText::new(["⠋", "⠙", "⠹", "⠸"][phase])
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

        section_header(ui, "🔧", "Engines");
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

        if let Some(sendme) = &sendme_status {
            if tool_card(
                ui,
                "📡 Sendme",
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
                "🐊 Croc",
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
        ui.add_space(3.0);
        if tool_card(
            ui,
            "⚡ EazySendme",
            "Sendme performance + Croc-like automatic ticket sharing",
            croc_status.as_ref().map(|s| s.available).unwrap_or(false)
                && sendme_status.as_ref().map(|s| s.available).unwrap_or(false),
            None,
            Color32::from_rgb(255, 165, 0),
            self.selected_tool == SelectedTool::EazySendme,
        )
        .clicked()
            && croc_status.as_ref().map(|s| s.available).unwrap_or(false)
            && sendme_status.as_ref().map(|s| s.available).unwrap_or(false)
            && self.selected_tool != SelectedTool::EazySendme
        {
            self.switch_tool(SelectedTool::EazySendme);
        }

        ui.add_space(16.0);
        section_header(ui, "🚀", "Quick Actions");
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            let accent = self.engine_color();
            let w = (ui.available_width() - 8.0) / 2.0;
            if accent_button_sized(ui, "📤 Send", accent, Vec2::new(w, 42.0)).clicked() {
                self.view = AppView::Send;
            }
            if accent_button_sized(ui, "📥 Receive", accent, Vec2::new(w, 42.0)).clicked() {
                self.view = AppView::Receive;
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
                    ui.label(
                        RichText::new("Text to send")
                            .color(TEXT_SECONDARY)
                            .size(11.0),
                    );
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
                    ui.checkbox(
                        &mut self.croc_show_qr,
                        RichText::new("Show QR popup").size(11.0),
                    );
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
                if send_locked {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.eazysendme_custom_code)
                            .hint_text("Random code")
                            .interactive(false),
                    );
                } else {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.eazysendme_custom_code)
                            .hint_text("Random code"),
                    );
                    if response.changed() && !self.eazysendme_custom_code.is_empty() {
                        // Validation or persistence could go here
                    }
                }
                ui.label(
                    RichText::new("Must use more than 6 characters.")
                        .size(10.0)
                        .color(TEXT_MUTED),
                );
            });
            ui.add_space(6.0);
        }

        // ── File list ──
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
                        self.picker_in_flight = true;
                        self.picker_block_until = self.animation_time + 0.8;
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            self.add_path(p);
                        }
                        self.picker_in_flight = false;
                        self.picker_block_until = self.animation_time + 0.5;
                        picker_used_this_frame = true;
                    }
                    if !send_locked
                        && !croc_text_mode
                        && picker_ready
                        && !picker_used_this_frame
                        && accent_button_sized(ui, "+ File", accent, Vec2::new(60.0, 22.0))
                            .clicked()
                    {
                        self.picker_in_flight = true;
                        self.picker_block_until = self.animation_time + 0.8;
                        if let Some(paths) = rfd::FileDialog::new().pick_files() {
                            for p in paths {
                                self.add_path(p);
                            }
                        }
                        self.picker_in_flight = false;
                        self.picker_block_until = self.animation_time + 0.5;
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
                let changed = ui
                    .checkbox(
                        &mut self.sendme_one_shot,
                        RichText::new("Stop after single transfer").size(12.0),
                    )
                    .changed();
                if changed {
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
                        Color32::from_rgb(255, 165, 0),
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
                        self.start_send();
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
                        self.retry_send();
                    }
                    if accent_button_sized(
                        ui,
                        "✕ Cancel",
                        Color32::from_rgb(150, 75, 75),
                        Vec2::new(100.0, 32.0),
                    )
                    .clicked()
                    {
                        self.reset_transfer();
                        self.send_items.clear();
                    }
                    if accent_button_sized(ui, "🆕 New", accent, Vec2::new(80.0, 32.0)).clicked()
                    {
                        self.reset_transfer();
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
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).color(TEXT_PRIMARY).strong().size(13.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if accent_button_sized(
                        ui,
                        "📋 Paste",
                        self.engine_color(),
                        Vec2::new(70.0, 22.0),
                    )
                    .clicked()
                    {
                        if let Some(clip) = ui.ctx().input(|i| {
                            i.events.iter().find_map(|e| {
                                if let egui::Event::Paste(s) = e {
                                    Some(s.clone())
                                } else {
                                    None
                                }
                            })
                        }) {
                            self.receive_code = clip;
                        }
                    }
                });
            });
            ui.add_space(3.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.receive_code)
                    .hint_text(hint)
                    .desired_width(ui.available_width() - 4.0)
                    .font(egui::FontId::new(13.0, egui::FontFamily::Monospace)),
            );
            ui.add_space(3.0);
            ui.label(
                RichText::new("Interrupted downloads can be resumed by running Receive again.")
                    .color(TEXT_MUTED)
                    .size(10.0),
            );
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
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            self.receive_output_dir = Some(p);
                            self.persist_user_settings();
                        }
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

        ui.add_space(8.0);

        match &self.transfer_state {
            TransferState::Idle => {
                let (color, label) = match self.selected_tool {
                    SelectedTool::Croc => (CROC_COLOR, "🐊 Receive"),
                    SelectedTool::Sendme => (SENDME_COLOR, "📡 Receive"),
                    SelectedTool::EazySendme => {
                        (Color32::from_rgb(255, 165, 0), "⚡ Receive (via Croc)")
                    }
                };
                if accent_button(ui, label, color).clicked() {
                    self.start_receive();
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
                    if accent_button_sized(ui, "🆕 New", self.engine_color(), Vec2::new(80.0, 32.0))
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
                        self.retry_receive();
                    }
                    if accent_button_sized(
                        ui,
                        "✕ Cancel",
                        Color32::from_rgb(150, 75, 75),
                        Vec2::new(100.0, 32.0),
                    )
                    .clicked()
                    {
                        self.reset_transfer();
                        self.receive_code.clear();
                    }
                });
            }
        }
    }

    fn show_transfer_status(&mut self, ui: &mut egui::Ui) {
        let mut wants_cancel = false;
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
                    {
                        done = ((effective_progress as f64) * total as f64) as u64;
                    }
                    ui.horizontal(|ui| {
                        let phase = (self.animation_time * 3.0) as usize % 4;
                        ui.label(
                            RichText::new(["⠋", "⠙", "⠹", "⠸"][phase])
                                .color(accent)
                                .size(14.0),
                        );

                        let status_text = match self.transfer_phase {
                            TransferPhase::Preparing => "Preparing…",
                            TransferPhase::WaitingForReceiver => "Waiting for receiver…",
                            TransferPhase::Transferring => {
                                if self.view == AppView::Receive {
                                    if effective_progress >= 0.999 {
                                        "Building and verifying content…"
                                    } else {
                                        "Downloading…"
                                    }
                                } else {
                                    "Transferring…"
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
                    });
                    ui.add_space(4.0);

                    if !sendme_serve_mode
                        || self.transfer_phase == TransferPhase::Transferring
                        || self.transfer_phase == TransferPhase::Preparing
                    {
                        if self.transfer_phase == TransferPhase::Transferring {
                            animated_progress_bar(ui, effective_progress, accent);
                        } else {
                            pulsing_progress_bar(ui, self.animation_time, accent);
                        }
                        if let Some(raw) = &self.latest_cli_progress_line {
                            ui.add_space(3.0);
                            ui.label(RichText::new(raw).monospace().size(10.0).color(TEXT_MUTED));
                        }
                        let pct_text = format!("{:.1}%", effective_progress * 100.0);
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
                                            "Total: {}",
                                            format_file_size(total)
                                        ))
                                        .size(10.0)
                                        .color(TEXT_MUTED),
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
                                            "Total: {}",
                                            format_file_size(total)
                                        ))
                                        .size(10.0)
                                        .color(TEXT_MUTED),
                                    );
                                }
                            }
                            TransferPhase::Transferring => {
                                let progress_label = format!("Progress: {}", pct_text);
                                ui.label(
                                    RichText::new(progress_label)
                                        .size(10.0)
                                        .color(TEXT_SECONDARY),
                                );
                                if total > 0 {
                                    ui.label(
                                        RichText::new(format!(
                                            "Data: {}/{}",
                                            format_file_size(done),
                                            format_file_size(total)
                                        ))
                                        .size(10.0)
                                        .color(TEXT_MUTED),
                                    );
                                }
                                if let Some(speed) = self.transfer_speed_bps {
                                    ui.label(
                                        RichText::new(format!(
                                            "Speed: {}/s",
                                            format_file_size(speed as u64)
                                        ))
                                        .size(10.0)
                                        .color(TEXT_MUTED),
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
                    } else {
                        ui.horizontal_wrapped(|ui| match self.transfer_phase {
                            TransferPhase::Preparing
                            | TransferPhase::WaitingForReceiver
                            | TransferPhase::EazySharingTicket
                            | TransferPhase::EazyWaitingForPeer => {
                                let label = match self.transfer_phase {
                                    TransferPhase::EazySharingTicket => {
                                        "Sharing ticket via Croc..."
                                    }
                                    TransferPhase::EazyWaitingForPeer => {
                                        "Waiting for peer (Sendme)..."
                                    }
                                    _ => "Waiting for receiver...",
                                };
                                ui.label(RichText::new(label).size(10.0).color(TEXT_SECONDARY));
                            }
                            TransferPhase::Transferring => {
                                ui.label(
                                    RichText::new("Transferring...")
                                        .size(10.0)
                                        .color(TEXT_SECONDARY),
                                );
                            }
                        });
                    }

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if accent_button_sized(ui, "✕ Cancel", ERROR, Vec2::new(90.0, 26.0))
                            .clicked()
                        {
                            wants_cancel = true;
                        }
                    });
                }
                TransferState::Completed => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("✅").size(14.0));
                        ui.label(RichText::new("Complete").color(SUCCESS).strong().size(13.0));
                        if let Some(start) = self.transfer_start_time {
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
                            ui.label(
                                RichText::new(format!(
                                    "Data: {}/{}",
                                    format_file_size(done),
                                    format_file_size(total)
                                ))
                                .size(10.0)
                                .color(TEXT_MUTED),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Progress: {:.1}%",
                                    if total > 0 {
                                        (done as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
                                    } else {
                                        100.0
                                    }
                                ))
                                .size(10.0)
                                .color(TEXT_SECONDARY),
                            );
                        }
                        if let Some(speed) = self.transfer_speed_bps {
                            ui.label(
                                RichText::new(format!(
                                    "Speed: {}/s",
                                    format_file_size(speed as u64)
                                ))
                                .size(10.0)
                                .color(TEXT_MUTED),
                            );
                        }
                    });
                }
                TransferState::Failed(e) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("❌").size(14.0));
                        ui.label(RichText::new(e).color(ERROR).size(12.0));
                    });
                }
            }

            // Code/ticket
            if let Some(code) = &self.transfer_code {
                ui.add_space(6.0);
                let (label, color) = match self.selected_tool {
                    SelectedTool::Croc => ("Share this code:", CROC_COLOR),
                    SelectedTool::Sendme => ("Share this ticket:", SENDME_COLOR),
                    SelectedTool::EazySendme => {
                        ("EazySendme ticket:", Color32::from_rgb(255, 165, 0))
                    }
                };
                if code_display(ui, label, code, color) {
                    self.show_toast("Code copied".to_string(), SUCCESS);
                }
                ui.add_space(4.0);
                if accent_button_sized(ui, "🔳 Show QR", color, Vec2::new(100.0, 24.0)).clicked()
                {
                    wants_show_qr = true;
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
    let width = 64u32;
    let height = 64u32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    let set_px = |buf: &mut [u8], x: u32, y: u32, r: u8, g: u8, b: u8, a: u8| {
        let idx = ((y * width + x) * 4) as usize;
        buf[idx] = r;
        buf[idx + 1] = g;
        buf[idx + 2] = b;
        buf[idx + 3] = a;
    };

    let cx = 32.0f32;
    let cy = 32.0f32;
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d2 = dx * dx + dy * dy;
            if d2 <= 30.0 * 30.0 {
                set_px(&mut rgba, x, y, 214, 66, 36, 255);
            }
            if d2 <= 26.0 * 26.0 {
                set_px(&mut rgba, x, y, 20, 20, 20, 255);
            }
            if d2 <= 22.0 * 22.0 {
                set_px(&mut rgba, x, y, 214, 66, 36, 255);
            }
        }
    }

    for y in 12..52 {
        for x in 18..46 {
            let p1 = (x as i32) - (y as i32) + 8;
            let p2 = (x as i32) - (y as i32) - 6;
            let p3 = (x as i32) + (y as i32) - 54;
            let p4 = (x as i32) + (y as i32) - 70;
            let in_top = p1 >= 0 && p2 <= 0 && y <= 34;
            let in_bottom = p3 >= 0 && p4 <= 0 && y >= 28;
            if in_top || in_bottom {
                set_px(&mut rgba, x, y, 12, 12, 12, 255);
            }
        }
    }

    egui::IconData {
        rgba,
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
        return parse_size_prefix(tail);
    }
    None
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
    parse_size_prefix(inside)
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
    match unit.to_lowercase().as_str() {
        "b" => 1.0,
        "kb" | "kib" => 1024.0,
        "mb" | "mib" => 1024.0 * 1024.0,
        "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "tb" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
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

fn parse_file_uri_list(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with("file://") {
            continue;
        }
        let uri_body = line
            .strip_prefix("file://")
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("");
        if uri_body.is_empty() {
            continue;
        }
        let decoded = percent_decode_uri_component(uri_body);
        #[cfg(target_os = "windows")]
        let path = {
            // file:///C:/path -> C:/path
            let p = decoded.trim_start_matches('/');
            PathBuf::from(p)
        };
        #[cfg(not(target_os = "windows"))]
        let path = PathBuf::from(decoded);
        if path.exists() {
            out.push(path);
        }
    }
    out
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

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 640.0])
            .with_min_inner_size([420.0, 360.0])
            .with_title("DataBeam")
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
        extract_croc_received_text, extract_croc_received_text_from_logs,
        normalize_sendme_ticket, parse_croc_file_counter_progress, parse_croc_total_size_hint,
        parse_payload_progress, parse_sendme_imported_size_hint, parse_sendme_item_index,
        parse_sendme_total_files_hint, parse_stage_progress, AppView, DataBeamApp, SelectedTool,
        TransferMsg, TransferPhase, TransferState,
    };
    use std::sync::mpsc;

    #[test]
    fn stage_progress_parses_bracket_with_spinner() {
        let line = "[1/4]⠁ Connecting ... [00:00:00]";
        let p = parse_stage_progress(line).expect("stage progress");
        assert!((p - 0.25).abs() < f32::EPSILON);
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
        assert!(total > 6 * 1024 * 1024 * 1024);
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
        let line = "Sending 57865 files (6.4 GB)";
        let total = parse_croc_total_size_hint(line).expect("croc total");
        assert!(total > 6 * 1024 * 1024 * 1024);
    }

    #[test]
    fn croc_total_size_hint_ignores_per_file_lines() {
        let line = "sending databeam_QWIXer/readme.txt (6.0 KB)";
        assert!(parse_croc_total_size_hint(line).is_none());
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

        app.update_transfer_metrics_from_log(
            "file_download.py 100% |====| (83/83 kB, 58 MB/s) 2628/57865",
        );

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
        assert!(total > 6 * 1024 * 1024 * 1024);

        app.update_transfer_metrics_from_log("Sending (->192.168.1.60:56052)");
        assert_eq!(app.transfer_phase, TransferPhase::Transferring);

        app.update_transfer_metrics_from_log(
            "file_download.py 100% |====| (83/83 kB, 58 MB/s) 2628/57865",
        );

        let p = app.effective_progress();
        assert!(p > 0.04 && p < 0.05, "unexpected overall progress: {p}");
        let done = app.transfer_done_bytes.expect("derived done bytes");
        assert!(done > 200 * 1024 * 1024, "unexpected done bytes: {done}");
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
    fn sendme_serve_mode_waiting_banner_resets_to_waiting() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.sendme_one_shot = false;
        app.transfer_phase = TransferPhase::Transferring;
        app.transfer_progress = 0.42;
        app.transfer_done_bytes = Some(1024);
        app.sendme_had_transfer = true;
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::WaitingForReceiver)
            .expect("send output");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.transfer_state, TransferState::Running);
        assert_eq!(app.transfer_phase, TransferPhase::WaitingForReceiver);
        assert!(app.transfer_done_bytes.is_none());
        assert!(app.transfer_progress <= f32::EPSILON);
    }

    #[test]
    fn sendme_one_shot_waiting_banner_marks_complete() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.sendme_one_shot = true;
        app.transfer_phase = TransferPhase::Transferring;
        app.transfer_progress = 0.42;
        app.transfer_done_bytes = Some(1024);
        app.transfer_total_bytes = Some(2048);
        app.sendme_had_transfer = true;
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::WaitingForReceiver)
            .expect("send output");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.transfer_state, TransferState::Completed);
        assert!((app.transfer_progress - 1.0).abs() < f32::EPSILON);
        assert_eq!(app.transfer_done_bytes, Some(2048));
    }

    #[test]
    fn sendme_one_shot_waiting_banner_marks_complete_after_seen_transfer_flag() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.sendme_one_shot = true;
        app.transfer_phase = TransferPhase::WaitingForReceiver;
        app.sendme_had_transfer = true;
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::WaitingForReceiver)
            .expect("send output");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.transfer_state, TransferState::Completed);
        assert!((app.transfer_progress - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn sendme_waiting_signal_does_not_complete_one_shot_before_transfer() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.sendme_one_shot = true;
        app.transfer_phase = TransferPhase::Preparing;
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::WaitingForReceiver)
            .expect("send waiting");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.transfer_state, TransferState::Running);
        assert_eq!(app.transfer_phase, TransferPhase::WaitingForReceiver);
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
    fn sendme_sender_rollover_adds_previous_file_total() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::Transferring;
        app.transfer_total_bytes = Some((500_f64 * 1024.0 * 1024.0) as u64);

        app.update_transfer_metrics_from_log(
            "n deadbeef r 1/1 i 0 # abc [] 150.00 MiB/200.00 MiB",
        );
        let first = app.transfer_done_bytes.unwrap_or(0);
        assert!(first > 149 * 1024 * 1024);

        app.update_transfer_metrics_from_log(
            "n deadbeef r 1/1 i 0 # abc [] 10.00 MiB/100.00 MiB",
        );
        let second = app.transfer_done_bytes.unwrap_or(0);
        assert!(
            second >= (210_f64 * 1024.0 * 1024.0) as u64,
            "expected rollover accounting to include prior file total, got {second}"
        );
    }

    #[test]
    fn sendme_sender_payload_complete_marks_one_shot_complete() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.transfer_phase = TransferPhase::Transferring;
        app.sendme_one_shot = true;
        app.transfer_total_bytes = Some((300_f64 * 1024.0 * 1024.0) as u64);
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::Output(
            "n deadbeef r 1/1 i 0 # abc [] 200.00 MiB/200.00 MiB".to_string(),
        ))
        .expect("send output");
        tx.send(TransferMsg::Output(
            "n deadbeef r 1/1 i 1 # abc [] 100.00 MiB/100.00 MiB".to_string(),
        ))
        .expect("send output");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.transfer_state, TransferState::Completed);
        assert_eq!(
            app.transfer_done_bytes,
            Some((300_f64 * 1024.0 * 1024.0) as u64)
        );
    }

    #[test]
    fn sendme_stale_activity_after_waiting_cycle_is_ignored() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.sendme_one_shot = false;
        app.transfer_phase = TransferPhase::Transferring;
        app.sendme_had_transfer = true;
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::WaitingForReceiver)
            .expect("send waiting");
        tx.send(TransferMsg::SenderTransferActivity)
            .expect("send stale activity");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.transfer_state, TransferState::Running);
        assert_eq!(app.transfer_phase, TransferPhase::WaitingForReceiver);
        assert!(!app.sendme_had_transfer);
    }

    #[test]
    fn sendme_repeat_ticket_line_finishes_serve_cycle() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.sendme_one_shot = false;
        app.transfer_phase = TransferPhase::Transferring;
        app.sendme_had_transfer = true;
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::Output("sendme receive blobabc123".to_string()))
            .expect("send output");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.transfer_state, TransferState::Running);
        assert_eq!(app.transfer_phase, TransferPhase::WaitingForReceiver);
    }

    #[test]
    fn sendme_repeat_ticket_line_finishes_one_shot_cycle() {
        let mut app = DataBeamApp::default();
        app.selected_tool = SelectedTool::Sendme;
        app.view = AppView::Send;
        app.transfer_state = TransferState::Running;
        app.sendme_one_shot = true;
        app.transfer_phase = TransferPhase::Transferring;
        app.sendme_had_transfer = true;
        let (tx, rx) = mpsc::channel();
        app.transfer_rx = Some(rx);
        tx.send(TransferMsg::Output("to get this data, use".to_string()))
            .expect("send output");
        drop(tx);

        app.poll_transfer();

        assert_eq!(app.transfer_state, TransferState::Completed);
    }
}

