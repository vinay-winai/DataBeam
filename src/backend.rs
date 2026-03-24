use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{BufReader, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

use flate2::read::GzDecoder;
use sendme_native::{
    check_and_export_local_in as native_check_and_export_local_in,
    cleanup_sendme_receive_artifacts_for_ticket_in as native_cleanup_sendme_receive_artifacts_for_ticket_in,
    download as native_sendme_download,
    local_ticket_size_on_disk as native_local_ticket_size_on_disk,
    local_ticket_size_on_disk_in as native_local_ticket_size_on_disk_in,
    start_share as native_sendme_start_share,
    cleanup_sendme_receive_artifacts_for_ticket as native_cleanup_sendme_receive_artifacts_for_ticket,
    AddrInfoOptions as NativeAddrInfoOptions, AppHandle as NativeSendmeAppHandle,
    EventEmitter as NativeSendmeEventEmitter, ReceiveOptions as NativeReceiveOptions,
    RelayModeOption as NativeRelayModeOption, SendOptions as NativeSendOptions,
};
pub use sendme_native::ticket_to_hex_hash as native_ticket_to_hex_hash;
use serde::Deserialize;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

// ── Embedded Binaries ──────────────────────────────────────────────
// Embedded payloads are intentionally disabled.
// DataBeam uses managed downloads or system-installed binaries.

const CROC_GZ: &[u8] = &[];

/// Get the cache directory for extracted binaries
fn bundled_bin_dir() -> PathBuf {
    let base = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("databeam").join("bin")
}

fn new_hidden_command(program: impl AsRef<OsStr>) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new(program);
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        cmd
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new(program)
    }
}

/// Extract a gzip-compressed binary to the cache directory.
/// Returns the path to the extracted binary, or None if data is empty.
fn extract_bundled_binary(name: &str, gz_data: &[u8]) -> Option<PathBuf> {
    if gz_data.is_empty() {
        return None;
    }

    let bin_dir = bundled_bin_dir();
    fs::create_dir_all(&bin_dir).ok()?;

    let bin_path = bin_dir.join(name);

    // If already extracted, verify it exists and is executable
    if bin_path.exists() {
        // Check file size matches (simple freshness check)
        if let Ok(meta) = fs::metadata(&bin_path) {
            if meta.len() > 0 {
                return Some(bin_path);
            }
        }
    }

    // Decompress and write
    let mut decoder = GzDecoder::new(gz_data);
    let mut decompressed = Vec::new();
    if decoder.read_to_end(&mut decompressed).is_err() {
        return None;
    }

    let mut file = fs::File::create(&bin_path).ok()?;
    file.write_all(&decompressed).ok()?;
    drop(file);

    // Make executable on unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o755));
    }

    Some(bin_path)
}

// ── Tool Detection ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Tool {
    Croc,
    Sendme,
}

impl Tool {
    pub fn name(&self) -> &str {
        match self {
            Tool::Croc => "croc",
            Tool::Sendme => "sendme",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolStatus {
    pub tool: Tool,
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

fn native_sendme_version() -> Option<&'static str> {
    let mut in_package = false;
    for line in include_str!("../third_party/sendme/Cargo.toml").lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "version" {
            continue;
        }
        let version = value.trim().trim_matches('"');
        if !version.is_empty() {
            return Some(version);
        }
    }
    None
}

fn detect_native_sendme() -> ToolStatus {
    let version = native_sendme_version()
        .map(|version| format!("sendme {version} (native)"))
        .or_else(|| Some("sendme (native)".to_string()));

    ToolStatus {
        tool: Tool::Sendme,
        available: true,
        path: None,
        version,
    }
}

fn managed_binary_name(tool: &Tool) -> String {
    if cfg!(windows) {
        format!("{}.exe", tool.name())
    } else {
        tool.name().to_string()
    }
}

fn managed_binary_path(tool: &Tool) -> PathBuf {
    bundled_bin_dir().join(managed_binary_name(tool))
}

fn set_executable_permissions(_path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(_path, fs::Permissions::from_mode(0o755));
    }
}

fn github_repo(tool: &Tool) -> &'static str {
    match tool {
        Tool::Croc => "schollz/croc",
        Tool::Sendme => "n0-computer/sendme",
    }
}

fn fetch_latest_release(tool: &Tool) -> Result<GitHubRelease, String> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        github_repo(tool)
    );
    let json = download_bytes(&url)?;
    serde_json::from_slice::<GitHubRelease>(&json)
        .map_err(|e| format!("Failed to parse GitHub release JSON: {e}"))
}

fn find_asset_by_markers<'a>(
    release: &'a GitHubRelease,
    prefix: &str,
    markers: &[&str],
) -> Option<&'a GitHubAsset> {
    for marker in markers {
        if let Some(asset) = release.assets.iter().find(|asset| {
            asset.name.starts_with(prefix)
                && asset.name.contains(marker)
                && (asset.name.ends_with(".tar.gz") || asset.name.ends_with(".zip"))
        }) {
            return Some(asset);
        }
    }
    None
}

fn sendme_asset_markers() -> Option<&'static [&'static str]> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some(&["-darwin-aarch64"]),
        ("macos", "x86_64") => Some(&["-darwin-x86_64"]),
        ("linux", "aarch64") => Some(&["-linux-aarch64"]),
        ("linux", "x86_64") => Some(&["-linux-x86_64"]),
        ("windows", "x86_64") => Some(&["-windows-x86_64"]),
        // Sendme currently ships Windows x86_64 only. On ARM64 Windows, prefer native if it
        // appears in a future release and otherwise fall back to x86_64.
        ("windows", "aarch64") => Some(&["-windows-aarch64", "-windows-x86_64"]),
        _ => None,
    }
}

fn croc_asset_markers() -> Option<&'static [&'static str]> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some(&["macOS-ARM64", "macOS-64bit"]),
        ("macos", "x86_64") => Some(&["macOS-64bit"]),
        ("linux", "aarch64") => Some(&["Linux-ARM64"]),
        ("linux", "x86_64") => Some(&["Linux-64bit"]),
        ("linux", "arm") => Some(&["Linux-ARM"]),
        ("windows", "aarch64") => Some(&["Windows-ARM64", "Windows-64bit"]),
        ("windows", "x86_64") => Some(&["Windows-64bit"]),
        ("windows", "arm") => Some(&["Windows-ARM", "Windows-64bit"]),
        _ => None,
    }
}

fn find_release_asset<'a>(tool: &Tool, release: &'a GitHubRelease) -> Option<&'a GitHubAsset> {
    match tool {
        Tool::Sendme => find_asset_by_markers(release, "sendme-", sendme_asset_markers()?),
        Tool::Croc => find_asset_by_markers(release, "croc_", croc_asset_markers()?),
    }
}

#[cfg(not(target_os = "windows"))]
fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let output = new_hidden_command("curl")
        .args(["-fsSL", "-A", "databeam", url])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to execute curl: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "curl download failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(output.stdout)
}

#[cfg(target_os = "windows")]
fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    fn escape_ps_single_quote(value: &str) -> String {
        value.replace('\'', "''")
    }

    let tmp =
        tempfile::NamedTempFile::new().map_err(|e| format!("Failed to allocate temp file: {e}"))?;
    let tmp_path = tmp.into_temp_path(); // Close file handle, keep temp path
    let tmp_path_str = tmp_path.to_string_lossy().to_string();

    let script = format!(
        "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -UseBasicParsing -Headers @{{'User-Agent'='Mozilla/5.0 (Windows NT 10.0; Win64; x64) DataBeam/{}'; 'Accept'='application/vnd.github+json, application/octet-stream'}} -Uri '{}' -OutFile '{}'",
        env!("CARGO_PKG_VERSION"),
        escape_ps_single_quote(url),
        escape_ps_single_quote(&tmp_path_str)
    );

    let output = new_hidden_command("powershell")
        .args(["-NoProfile", "-Command", &script])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to execute PowerShell download: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "PowerShell download failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    fs::read(&tmp_path).map_err(|e| format!("Failed to read downloaded file: {e}"))
}

fn extract_binary_from_tar_gz(
    archive_bytes: &[u8],
    expected_name: &str,
    output_path: &Path,
) -> Result<(), String> {
    let cursor = Cursor::new(archive_bytes);
    let decoder = GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(decoder);

    for entry_res in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry_res.map_err(|e| e.to_string())?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let entry_name = entry
            .path()
            .map_err(|e| e.to_string())?
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if entry_name.eq_ignore_ascii_case(expected_name) {
            let mut out = fs::File::create(output_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            set_executable_permissions(output_path);
            return Ok(());
        }
    }

    Err(format!(
        "Archive did not contain expected binary '{}'",
        expected_name
    ))
}

fn extract_binary_from_zip(
    archive_bytes: &[u8],
    expected_name: &str,
    output_path: &Path,
) -> Result<(), String> {
    let cursor = Cursor::new(archive_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.name().ends_with('/') {
            continue;
        }
        let entry_name = Path::new(entry.name())
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if entry_name.eq_ignore_ascii_case(expected_name) {
            let mut out = fs::File::create(output_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            set_executable_permissions(output_path);
            return Ok(());
        }
    }

    Err(format!(
        "Archive did not contain expected binary '{}'",
        expected_name
    ))
}

fn install_managed_binary(tool: &Tool) -> Option<PathBuf> {
    let output_path = managed_binary_path(tool);
    if output_path.exists()
        && fs::metadata(&output_path)
            .map(|m| m.len() > 0)
            .unwrap_or(false)
    {
        return Some(output_path);
    }

    fs::create_dir_all(bundled_bin_dir()).ok()?;

    let release = match fetch_latest_release(tool) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} latest-release lookup failed: {}", tool.name(), e);
            return None;
        }
    };

    let asset = match find_release_asset(tool, &release) {
        Some(asset) => asset,
        None => {
            eprintln!(
                "{} has no release asset for {}-{} in {}",
                tool.name(),
                std::env::consts::OS,
                std::env::consts::ARCH,
                release.tag_name
            );
            return None;
        }
    };

    let bytes = match download_bytes(&asset.browser_download_url) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("{} download failed: {}", tool.name(), e);
            return None;
        }
    };

    let extract_result = if asset.name.ends_with(".tar.gz") {
        extract_binary_from_tar_gz(&bytes, &managed_binary_name(tool), &output_path)
    } else if asset.name.ends_with(".zip") {
        extract_binary_from_zip(&bytes, &managed_binary_name(tool), &output_path)
    } else {
        Err(format!("Unsupported archive format: {}", asset.name))
    };

    if let Err(e) = extract_result {
        let _ = fs::remove_file(&output_path);
        eprintln!("{} extraction failed: {}", tool.name(), e);
        return None;
    }

    Some(output_path)
}

/// Initialize managed binaries:
/// 1) Extract embedded binaries when available.
/// 2) Otherwise download platform-specific binaries from official GitHub releases.
pub fn init_bundled_binaries() -> (Option<PathBuf>, Option<PathBuf>) {
    let mut croc_path = extract_bundled_binary(&managed_binary_name(&Tool::Croc), CROC_GZ);

    // Prefer managed Croc binaries on every platform for consistent behavior.
    // If managed download is unavailable, detection later falls back to system PATH.
    if croc_path.is_none() {
        croc_path = install_managed_binary(&Tool::Croc);
    }

    // Sendme is linked as a native Rust library (third_party/sendme), so no external
    // sendme binary is required for transfers.
    let sendme_path = None;

    (croc_path, sendme_path)
}

/// Detect a tool — first check managed path, then fall back to system PATH.
fn detect_tool_with_bundled(tool: &Tool, bundled_path: Option<&PathBuf>) -> ToolStatus {
    // Try managed binary first
    if let Some(bp) = bundled_path {
        if bp.exists() {
            let mut cmd = new_hidden_command(bp);

            let version = cmd
                .arg("--version")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .ok()
                .and_then(|o| {
                    let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    let ver = if stdout.is_empty() { stderr } else { stdout };
                    if ver.is_empty() {
                        None
                    } else {
                        Some(format!("{} (managed)", ver))
                    }
                });

            return ToolStatus {
                tool: tool.clone(),
                available: true,
                path: Some(bp.to_string_lossy().to_string()),
                version,
            };
        }
    }

    // Fall back to system PATH
    let name = tool.name();
    let sys_path = which::which(name).ok();
    let path_str = sys_path.as_ref().map(|p| p.to_string_lossy().to_string());
    let available = path_str.is_some();

    let version = if let Some(path) = sys_path {
        let mut cmd = new_hidden_command(path);

        cmd.arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .ok()
            .and_then(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                let ver = if stdout.is_empty() { stderr } else { stdout };
                if ver.is_empty() {
                    None
                } else {
                    Some(format!("{} (system)", ver))
                }
            })
    } else {
        None
    };

    ToolStatus {
        tool: tool.clone(),
        available,
        path: path_str,
        version,
    }
}

pub fn detect_all_tools(
    bundled_croc: Option<&PathBuf>,
    _bundled_sendme: Option<&PathBuf>,
) -> Vec<ToolStatus> {
    vec![
        detect_tool_with_bundled(&Tool::Croc, bundled_croc),
        detect_native_sendme(),
    ]
}

/// Resolve the binary path for a tool — prefer bundled, fall back to system
pub fn resolve_tool_path(tool: &Tool, statuses: &[ToolStatus]) -> Option<String> {
    statuses
        .iter()
        .find(|s| s.tool == *tool && s.available)
        .and_then(|s| s.path.clone())
}

// ── Transfer Messages ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum TransferMsg {
    Output(String),
    Progress(f32),
    Code(String),
    Completed,
    Error(String),
    Started,
    PeerDisconnected,
    WaitingForReceiver,
    SenderTransferActivity,
    ActiveTransfers(usize),
}

// ── Transfer Options ───────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CrocSendOptions {
    pub paths: Vec<PathBuf>,
    pub custom_code: Option<String>,
    pub text_mode: bool,
    pub text_value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CrocReceiveOptions {
    pub code: String,
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct SendmeSendOptions {
    pub paths: Vec<PathBuf>,
    pub one_shot: bool,
    pub blob_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct SendmeReceiveOptions {
    pub ticket: String,
    pub output_dir: Option<PathBuf>,
    pub blob_dir: Option<PathBuf>,
    pub overwrite: bool,
}

fn sender_blob_base_dir(base_dir: Option<&Path>) -> Option<PathBuf> {
    base_dir.map(|dir| dir.join("databeam_sender_stuff"))
}

fn derive_payload_root_name(paths: &[PathBuf]) -> Result<String, String> {
    if paths.is_empty() {
        return Err("No paths provided".to_string());
    }

    let mut largest_item = &paths[0];
    let mut largest_size = 0u64;

    for p in paths {
        let size = if p.is_dir() {
            get_dir_size(p)
        } else {
            fs::metadata(p).map(|m| m.len()).unwrap_or(0)
        };
        if size > largest_size {
            largest_size = size;
            largest_item = p;
        }
    }

    let base_name = largest_item
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("files");

    let mut name_slug: String = base_name.chars().take(20).collect();
    if paths.len() > 1 {
        name_slug.push_str("_and_more");
    }
    name_slug.push_str("_databeam");
    Ok(name_slug)
}

#[derive(Debug, Clone, Deserialize)]
struct NativeSenderRequestStarted {
    endpoint_id: String,
    connection_id: u64,
    request_id: u64,
    item_index: u64,
    hash_short: String,
    total_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct NativeSenderRequestProgress {
    connection_id: u64,
    request_id: u64,
    done_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct NativeSenderRequestCompleted {
    connection_id: u64,
    request_id: u64,
}

#[derive(Debug, Clone)]
enum NativeSendmeEvent {
    TransferStarted,
    TransferProgress {
        done: u64,
        total: u64,
        speed_bps: f64,
    },
    SenderRequestStarted(NativeSenderRequestStarted),
    SenderRequestProgress(NativeSenderRequestProgress),
    SenderRequestCompleted(NativeSenderRequestCompleted),
    TransferCompleted,
    TransferFailed,
    ActiveConnectionCount(usize),
    ReceiveStarted,
    ReceiveProgress {
        done: u64,
        total: u64,
        speed_bps: f64,
    },
    ReceiveExportStarted,
    ReceiveExportProgress {
        done: u64,
        total: u64,
    },
    ReceiveCompleted,
}

#[derive(Clone)]
struct NativeSendmeEmitter {
    tx: mpsc::Sender<NativeSendmeEvent>,
}

impl NativeSendmeEmitter {
    fn new(tx: mpsc::Sender<NativeSendmeEvent>) -> Self {
        Self { tx }
    }
}

impl NativeSendmeEventEmitter for NativeSendmeEmitter {
    fn emit_event(&self, event_name: &str) -> Result<(), String> {
        let evt = match event_name {
            "transfer-started" => NativeSendmeEvent::TransferStarted,
            "transfer-completed" => NativeSendmeEvent::TransferCompleted,
            "transfer-failed" => NativeSendmeEvent::TransferFailed,
            "receive-started" => NativeSendmeEvent::ReceiveStarted,
            "receive-completed" => NativeSendmeEvent::ReceiveCompleted,
            _ => return Ok(()),
        };
        self.tx
            .send(evt)
            .map_err(|e| format!("native event channel closed: {e}"))
    }

    fn emit_event_with_payload(&self, event_name: &str, payload: &str) -> Result<(), String> {
        let evt = match event_name {
            "transfer-progress" => {
                let Some((done, total, speed_bps)) = parse_native_progress_payload(payload) else {
                    return Ok(());
                };
                NativeSendmeEvent::TransferProgress {
                    done,
                    total,
                    speed_bps,
                }
            }
            "sender-request-started" => {
                let Ok(payload) = serde_json::from_str::<NativeSenderRequestStarted>(payload)
                else {
                    return Ok(());
                };
                NativeSendmeEvent::SenderRequestStarted(payload)
            }
            "sender-request-progress" => {
                let Ok(payload) = serde_json::from_str::<NativeSenderRequestProgress>(payload)
                else {
                    return Ok(());
                };
                NativeSendmeEvent::SenderRequestProgress(payload)
            }
            "sender-request-completed" => {
                let Ok(payload) = serde_json::from_str::<NativeSenderRequestCompleted>(payload)
                else {
                    return Ok(());
                };
                NativeSendmeEvent::SenderRequestCompleted(payload)
            }
            "receive-progress" => {
                let Some((done, total, speed_bps)) = parse_native_progress_payload(payload) else {
                    return Ok(());
                };
                NativeSendmeEvent::ReceiveProgress {
                    done,
                    total,
                    speed_bps,
                }
            }
            "active-connection-count" => {
                let count = payload.trim().parse::<usize>().unwrap_or(0);
                NativeSendmeEvent::ActiveConnectionCount(count)
            }
            "receive-export-started" => NativeSendmeEvent::ReceiveExportStarted,
            "receive-export-progress" => {
                let mut parts = payload.split(':');
                if let (Some(d_str), Some(t_str)) = (parts.next(), parts.next()) {
                    if let (Ok(done), Ok(total)) =
                        (d_str.trim().parse::<u64>(), t_str.trim().parse::<u64>())
                    {
                        NativeSendmeEvent::ReceiveExportProgress { done, total }
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            }
            _ => return Ok(()),
        };
        self.tx
            .send(evt)
            .map_err(|e| format!("native event channel closed: {e}"))
    }
}

fn parse_native_progress_payload(payload: &str) -> Option<(u64, u64, f64)> {
    let mut parts = payload.split(':');
    let done = parts.next()?.trim().parse::<u64>().ok()?;
    let total = parts.next()?.trim().parse::<u64>().ok()?;
    let speed_scaled = parts.next()?.trim().parse::<i64>().ok()?;
    let speed_bps = (speed_scaled as f64) / 1000.0;
    Some((done, total, speed_bps))
}

#[derive(Debug, Clone)]
struct SenderRequestRenderState {
    _endpoint_id: String,
    _connection_id: u64,
    _request_id: u64,
    _item_index: u64,
    hash_short: String,
    total_bytes: u64,
    done_bytes: u64,
    started_at: Instant,
}

pub fn format_size_unit(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.2} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.2} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn format_speed_unit(speed_bps: f64) -> String {
    if speed_bps <= 0.0 {
        return "0 B/s".to_string();
    }
    let speed = speed_bps.round().max(0.0) as u64;
    format!("{}/s", format_size_unit(speed))
}

fn format_elapsed_precise(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn format_sender_request_bar(done: u64, total: u64, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let total = total.max(1);
    let done = done.min(total);
    if done >= total {
        return "#".repeat(width);
    }
    let filled = ((done as f64 / total as f64) * width as f64).floor() as usize;
    let mut bar = String::with_capacity(width);
    for idx in 0..width {
        if idx < filled {
            bar.push('#');
        } else if idx == filled {
            bar.push('>');
        } else {
            bar.push('-');
        }
    }
    bar
}

fn format_sender_request_line(state: &SenderRequestRenderState) -> String {
    let _spinner = match (state.done_bytes / (256 * 1024)).wrapping_rem(4) {
        0 => '-',
        1 => '\\',
        2 => '|',
        _ => '/',
    };
    format!(
        "{} [{}]  [{}] {}/{}",
        state.hash_short,
        format_elapsed_precise(state.started_at.elapsed()),
        format_sender_request_bar(state.done_bytes, state.total_bytes, 36),
        format_size_unit(state.done_bytes),
        format_size_unit(state.total_bytes),
    )
}

// ── Byte-level reader for \r-delimited progress ────────────────────

fn read_lines_cr_aware(
    reader: impl Read + Send + 'static,
    tx: mpsc::Sender<TransferMsg>,
    parser: fn(&str, &mpsc::Sender<TransferMsg>, &Arc<AtomicBool>),
    cancel: Arc<AtomicBool>,
) {
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::with_capacity(512);

    loop {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let mut byte = [0u8; 1];
        match reader.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\n' || byte[0] == b'\r' {
                    if !buf.is_empty() {
                        let line = String::from_utf8_lossy(&buf).to_string();
                        parser(&line, &tx, &cancel);
                        buf.clear();
                    }
                } else {
                    buf.push(byte[0]);
                    // Check for prompts that don't end in newline (like "Overwrite? (y/N)")
                    if buf.len() >= 9 {
                        let partial = String::from_utf8_lossy(&buf);
                        let lower = partial.to_lowercase();
                        if lower.contains("overwrite") || lower.contains("--overwrite") {
                            parser(&partial, &tx, &cancel);
                            buf.clear();
                        }
                    }
                }
            }
            Err(_) => break,
        }
    }
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf).to_string();
        parser(&line, &tx, &cancel);
    }
}

// ── Croc Send ──────────────────────────────────────────────────────

pub fn croc_send(
    opts: CrocSendOptions,
    binary_path: &str,
) -> (mpsc::Receiver<TransferMsg>, ProcessHandle) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel2 = cancel.clone();
    let child_pid = Arc::new(std::sync::Mutex::new(None));
    let pid_handle = child_pid.clone();
    let binary = binary_path.to_string();

    let _worker = thread::spawn(move || {
        let mut cmd = new_hidden_command(&binary);

        // --yes is a GLOBAL flag, must come before the subcommand
        cmd.arg("--yes");
        cmd.arg("send");

        if let Some(code) = &opts.custom_code {
            cmd.env("CROC_SECRET", code);
        }
        if opts.text_mode {
            cmd.arg("--text");
            if let Some(text) = &opts.text_value {
                cmd.arg(text);
            }
        } else {
            for path in &opts.paths {
                cmd.arg(path);
            }
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        // Use temp dir for execution to ensure croc can write/remove its internal temp files
        cmd.current_dir(std::env::temp_dir());

        let _ = tx.send(TransferMsg::Started);

        match cmd.spawn() {
            Ok(mut child) => {
                if let Ok(mut guard) = pid_handle.lock() {
                    *guard = Some(child.id());
                }

                let stderr = child.stderr.take();
                let stdout = child.stdout.take();

                let tx2 = tx.clone();
                let c2 = cancel2.clone();
                let stderr_thread = stderr.map(|stderr| {
                    thread::spawn(move || {
                        read_lines_cr_aware(stderr, tx2, parse_croc_output, c2);
                    })
                });

                let tx3 = tx.clone();
                let c3 = cancel2.clone();
                let stdout_thread = stdout.map(|stdout| {
                    thread::spawn(move || {
                        read_lines_cr_aware(stdout, tx3, parse_croc_output, c3);
                    })
                });

                let status = child.wait();
                if let Some(h) = stderr_thread {
                    let _ = h.join();
                }
                if let Some(h) = stdout_thread {
                    let _ = h.join();
                }
                if cancel2.load(Ordering::Relaxed) {
                    let _ = tx.send(TransferMsg::Error("Transfer cancelled".to_string()));
                    return;
                }
                match status {
                    Ok(s) if s.success() => {
                        let _ = tx.send(TransferMsg::Completed);
                    }
                    Ok(s) => {
                        let _ = tx.send(TransferMsg::Error(format!(
                            "Process exited with code: {}",
                            s
                        )));
                    }
                    Err(e) => {
                        let _ = tx.send(TransferMsg::Error(format!("Process error: {}", e)));
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(TransferMsg::Error(format!("Failed to start croc: {}", e)));
            }
        }
    });

    (rx, ProcessHandle { cancel, child_pid })
}

// ── Croc Receive ───────────────────────────────────────────────────

pub fn croc_receive(
    opts: CrocReceiveOptions,
    binary_path: &str,
) -> (mpsc::Receiver<TransferMsg>, ProcessHandle) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel2 = cancel.clone();
    let child_pid = Arc::new(std::sync::Mutex::new(None));
    let pid_handle = child_pid.clone();
    let binary = binary_path.to_string();

    let _worker = thread::spawn(move || {
        let mut cmd = new_hidden_command(&binary);

        // croc v10.3.1: code must be passed via CROC_SECRET env var
        // (passing code as CLI arg is no longer supported in non-classic mode)
        cmd.env("CROC_SECRET", &opts.code);
        cmd.arg("--yes");

        if let Some(dir) = &opts.output_dir {
            cmd.arg("--out").arg(dir);
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        // Use temp dir for execution to ensure croc can write/remove its internal temp files safely
        cmd.current_dir(std::env::temp_dir());

        let _ = tx.send(TransferMsg::Started);

        match cmd.spawn() {
            Ok(mut child) => {
                if let Ok(mut guard) = pid_handle.lock() {
                    *guard = Some(child.id());
                }

                let stderr = child.stderr.take();
                let stdout = child.stdout.take();

                let tx2 = tx.clone();
                let c2 = cancel2.clone();
                let stderr_thread = stderr.map(|stderr| {
                    thread::spawn(move || {
                        read_lines_cr_aware(stderr, tx2, parse_croc_output, c2);
                    })
                });

                let tx3 = tx.clone();
                let c3 = cancel2.clone();
                let stdout_thread = stdout.map(|stdout| {
                    thread::spawn(move || {
                        read_lines_cr_aware(stdout, tx3, parse_croc_output, c3);
                    })
                });

                let status = child.wait();
                if let Some(h) = stderr_thread {
                    let _ = h.join();
                }
                if let Some(h) = stdout_thread {
                    let _ = h.join();
                }
                if cancel2.load(Ordering::Relaxed) {
                    let _ = tx.send(TransferMsg::Error("Transfer cancelled".to_string()));
                    return;
                }
                match status {
                    Ok(s) if s.success() => {
                        let _ = tx.send(TransferMsg::Completed);
                    }
                    Ok(s) => {
                        let _ = tx.send(TransferMsg::Error(format!(
                            "Process exited with code: {}",
                            s
                        )));
                    }
                    Err(e) => {
                        let _ = tx.send(TransferMsg::Error(format!("Process error: {}", e)));
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(TransferMsg::Error(format!("Failed to start croc: {}", e)));
            }
        }
    });

    (rx, ProcessHandle { cancel, child_pid })
}

// ── Sendme Send ────────────────────────────────────────────────────

pub fn sendme_send(
    opts: SendmeSendOptions,
    _binary_path: &str,
) -> (mpsc::Receiver<TransferMsg>, ProcessHandle) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel2 = cancel.clone();
    let child_pid = Arc::new(std::sync::Mutex::new(None));

    let _worker = thread::spawn(move || {
        let sender_blob_dir = sender_blob_base_dir(opts.blob_dir.as_deref());
        let payload_root_name = match derive_payload_root_name(&opts.paths) {
            Ok(name) => name,
            Err(e) => {
                let _ = tx.send(TransferMsg::Error(format!("Failed to prepare payload: {}", e)));
                return;
            }
        };

        let paths = opts.paths.clone();

        let _ = tx.send(TransferMsg::Started);

        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(TransferMsg::Error(format!(
                    "Failed to create native sendme runtime: {}",
                    e
                )));
                return;
            }
        };

        let (evt_tx, evt_rx) = mpsc::channel::<NativeSendmeEvent>();
        let app_handle: NativeSendmeAppHandle = Some(Arc::new(NativeSendmeEmitter::new(evt_tx)));
        let share = runtime.block_on(native_sendme_start_share(
            paths,
            payload_root_name,
            NativeSendOptions {
                relay_mode: NativeRelayModeOption::Default,
                ticket_type: NativeAddrInfoOptions::RelayAndAddresses,
                magic_ipv4_addr: None,
                magic_ipv6_addr: None,
                blob_dir: sender_blob_dir.clone(),
            },
            app_handle,
        ));

        let share = match share {
            Ok(result) => result,
            Err(e) => {
                let _ = tx.send(TransferMsg::Error(format!("Failed to start sendme: {}", e)));
                return;
            }
        };

        let _ = tx.send(TransferMsg::Code(share.ticket.clone()));
        let _ = tx.send(TransferMsg::Output(format!(
            "imported {} native, {}",
            share.entry_type,
            format_size_unit(share.size)
        )));
        let _ = tx.send(TransferMsg::Output(format!(
            "sendme receive {}",
            share.ticket
        )));
        let _ = tx.send(TransferMsg::WaitingForReceiver);

        let mut had_transfer = false;
        let mut sender_activity_emitted = false;
        let mut last_progress_emit = Instant::now();
        let mut last_request_emit = Instant::now();
        let mut sender_progress_max: f32 = 0.0;
        let mut last_logged_done: u64 = 0;
        let mut last_seen_total: u64 = 0;
        let mut last_seen_speed_bps: f64 = 0.0;
        let mut waiting_for_new_connection = false;
        let mut waiting_seen_zero_connections = false;
        let mut sender_requests: HashMap<(u64, u64), SenderRequestRenderState> = HashMap::new();

        loop {
            if cancel2.load(Ordering::Relaxed) {
                let _ = tx.send(TransferMsg::Error("Transfer cancelled".to_string()));
                break;
            }

            match evt_rx.recv_timeout(Duration::from_millis(150)) {
                Ok(NativeSendmeEvent::TransferStarted) => {
                    if !opts.one_shot && waiting_for_new_connection {
                        continue;
                    }
                    had_transfer = true;
                    if !sender_activity_emitted {
                        let _ = tx.send(TransferMsg::SenderTransferActivity);
                        sender_activity_emitted = true;
                        let _ = tx.send(TransferMsg::Progress(0.0));
                    }
                }
                Ok(NativeSendmeEvent::TransferProgress {
                    done,
                    total,
                    speed_bps,
                }) => {
                    if !opts.one_shot && waiting_for_new_connection {
                        continue;
                    }
                    had_transfer = true;
                    if !sender_activity_emitted {
                        let _ = tx.send(TransferMsg::SenderTransferActivity);
                        sender_activity_emitted = true;
                    }

                    let current_total = total.max(1);
                    let aggregate_done = done.min(current_total);
                    last_seen_total = current_total;
                    last_seen_speed_bps = speed_bps;

                    let progress = (aggregate_done as f32 / current_total as f32).clamp(0.0, 1.0);
                    if progress > sender_progress_max || progress >= 0.999 {
                        sender_progress_max = sender_progress_max.max(progress);
                        let _ = tx.send(TransferMsg::Progress(sender_progress_max));
                    }

                    let should_log = aggregate_done > last_logged_done || aggregate_done == 0;
                    if should_log && last_progress_emit.elapsed() >= Duration::from_millis(200) {
                        let line = format!(
                            "[3/4] Uploading ... {} / {} {}",
                            format_size_unit(aggregate_done),
                            format_size_unit(current_total),
                            format_speed_unit(speed_bps)
                        );
                        let _ = tx.send(TransferMsg::Output(line));
                        last_logged_done = aggregate_done;
                        last_progress_emit = Instant::now();
                    }
                }
                Ok(NativeSendmeEvent::SenderRequestStarted(started)) => {
                    if !opts.one_shot && waiting_for_new_connection {
                        continue;
                    }
                    had_transfer = true;
                    if !sender_activity_emitted {
                        let _ = tx.send(TransferMsg::SenderTransferActivity);
                        sender_activity_emitted = true;
                    }

                    let key = (started.connection_id, started.request_id);
                    if let Some(mut prev) = sender_requests.remove(&key) {
                        prev.done_bytes = prev.total_bytes;
                        let _ = tx.send(TransferMsg::Output(format_sender_request_line(&prev)));
                    }
                    let state = SenderRequestRenderState {
                        _endpoint_id: started.endpoint_id,
                        _connection_id: started.connection_id,
                        _request_id: started.request_id,
                        _item_index: started.item_index,
                        hash_short: started.hash_short,
                        total_bytes: started.total_bytes.max(1),
                        done_bytes: 0,
                        started_at: Instant::now(),
                    };
                    let line = format_sender_request_line(&state);
                    sender_requests.insert(key, state);
                    let _ = tx.send(TransferMsg::Output(line));
                }
                Ok(NativeSendmeEvent::SenderRequestProgress(progress_evt)) => {
                    if !opts.one_shot && waiting_for_new_connection {
                        continue;
                    }
                    had_transfer = true;
                    if !sender_activity_emitted {
                        let _ = tx.send(TransferMsg::SenderTransferActivity);
                        sender_activity_emitted = true;
                    }

                    let key = (progress_evt.connection_id, progress_evt.request_id);
                    let state =
                        sender_requests
                            .entry(key)
                            .or_insert_with(|| SenderRequestRenderState {
                                _endpoint_id: "?".to_string(),
                                _connection_id: progress_evt.connection_id,
                                _request_id: progress_evt.request_id,
                                _item_index: 0,
                                hash_short: "?".to_string(),
                                total_bytes: progress_evt.done_bytes.max(1),
                                done_bytes: 0,
                                started_at: Instant::now(),
                            });
                    let next_done = progress_evt.done_bytes.min(state.total_bytes);
                    state.done_bytes = state.done_bytes.max(next_done);

                    if state.done_bytes == state.total_bytes
                        || state.done_bytes == 0
                        || last_request_emit.elapsed() >= Duration::from_millis(90)
                    {
                        let _ = tx.send(TransferMsg::Output(format_sender_request_line(state)));
                        last_request_emit = Instant::now();
                    }
                }
                Ok(NativeSendmeEvent::SenderRequestCompleted(completed_evt)) => {
                    if !opts.one_shot && waiting_for_new_connection {
                        continue;
                    }
                    let key = (completed_evt.connection_id, completed_evt.request_id);
                    if let Some(mut state) = sender_requests.remove(&key) {
                        state.done_bytes = state.total_bytes;
                        let _ = tx.send(TransferMsg::Output(format_sender_request_line(&state)));
                    }
                }
                Ok(NativeSendmeEvent::ActiveConnectionCount(count)) => {
                    let _ = tx.send(TransferMsg::ActiveTransfers(count));
                    if !opts.one_shot && waiting_for_new_connection {
                        if count == 0 {
                            waiting_seen_zero_connections = true;
                        } else if waiting_seen_zero_connections {
                            waiting_for_new_connection = false;
                            waiting_seen_zero_connections = false;
                            had_transfer = true;
                            if !sender_activity_emitted {
                                let _ = tx.send(TransferMsg::SenderTransferActivity);
                                sender_activity_emitted = true;
                            }
                        }
                        continue;
                    }
                    if count > 0 {
                        had_transfer = true;
                        if !sender_activity_emitted {
                            let _ = tx.send(TransferMsg::SenderTransferActivity);
                            sender_activity_emitted = true;
                        }
                    }
                }
                Ok(NativeSendmeEvent::TransferCompleted) => {
                    if last_seen_total > 0 {
                        let final_line = format!(
                            "[3/4] Uploading ... {} / {} {}",
                            format_size_unit(last_seen_total),
                            format_size_unit(last_seen_total),
                            format_speed_unit(last_seen_speed_bps)
                        );
                        let _ = tx.send(TransferMsg::Output(final_line));
                    }
                    let _ = tx.send(TransferMsg::Progress(1.0));
                    let _ = tx.send(TransferMsg::Output(
                        "finished sending to client".to_string(),
                    ));
                    if opts.one_shot {
                        let _ = tx.send(TransferMsg::PeerDisconnected);
                        break;
                    } else {
                        let _ = tx.send(TransferMsg::WaitingForReceiver);
                        waiting_for_new_connection = true;
                        waiting_seen_zero_connections = false;
                        had_transfer = false;
                        sender_activity_emitted = false;
                        sender_progress_max = 0.0;
                        last_logged_done = 0;
                        last_seen_total = 0;
                        last_seen_speed_bps = 0.0;
                        sender_requests.clear();
                    }
                }
                Ok(NativeSendmeEvent::TransferFailed) => {
                    if opts.one_shot {
                        if had_transfer {
                            let _ = tx.send(TransferMsg::PeerDisconnected);
                        } else {
                            let _ = tx.send(TransferMsg::Error(
                                "Sendme transfer failed on sender side".to_string(),
                            ));
                        }
                        break;
                    } else {
                        if waiting_for_new_connection {
                            continue;
                        }
                        let _ = tx.send(TransferMsg::Output(
                            "sender cycle ended unexpectedly; waiting for receiver".to_string(),
                        ));
                        let _ = tx.send(TransferMsg::WaitingForReceiver);
                        waiting_for_new_connection = true;
                        waiting_seen_zero_connections = false;
                        had_transfer = false;
                        sender_activity_emitted = false;
                        sender_progress_max = 0.0;
                        last_logged_done = 0;
                        last_seen_total = 0;
                        last_seen_speed_bps = 0.0;
                        sender_requests.clear();
                        continue;
                    }
                }
                Ok(NativeSendmeEvent::ReceiveStarted)
                | Ok(NativeSendmeEvent::ReceiveProgress { .. })
                | Ok(NativeSendmeEvent::ReceiveCompleted)
                | Ok(NativeSendmeEvent::ReceiveExportStarted)
                | Ok(NativeSendmeEvent::ReceiveExportProgress { .. }) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if opts.one_shot {
                        if had_transfer {
                            let _ = tx.send(TransferMsg::PeerDisconnected);
                        } else {
                            let _ = tx.send(TransferMsg::Error(
                                "Send session ended before transfer started".to_string(),
                            ));
                        }
                        break;
                    } else {
                        if waiting_for_new_connection {
                            continue;
                        }
                        let _ = tx.send(TransferMsg::Output(
                            "sender event stream ended; waiting for receiver".to_string(),
                        ));
                        let _ = tx.send(TransferMsg::WaitingForReceiver);
                        waiting_for_new_connection = true;
                        waiting_seen_zero_connections = false;
                        had_transfer = false;
                        sender_activity_emitted = false;
                        sender_progress_max = 0.0;
                        last_logged_done = 0;
                        last_seen_total = 0;
                        last_seen_speed_bps = 0.0;
                        sender_requests.clear();
                        thread::sleep(Duration::from_millis(200));
                        continue;
                    }
                }
            }
        }

        let _ = fs::remove_dir_all(&share.blobs_data_dir);
        if let Some(blob_base) = sender_blob_dir.as_ref() {
            cleanup_sendme_send_dirs(blob_base);
            cleanup_sendme_temp_artifacts(blob_base);
        }
        cleanup_sendme_send_dirs(&std::env::temp_dir());
        cleanup_sendme_temp_artifacts(&std::env::temp_dir());
    });

    (rx, ProcessHandle { cancel, child_pid })
}
/// Create a deterministic staging directory with all items copied/linked into it for sendme
fn get_dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += get_dir_size(&p);
            } else if let Ok(m) = entry.metadata() {
                total += m.len();
            }
        }
    }
    total
}

fn cleanup_sendme_send_dirs(base_dir: &Path) {
    let Ok(entries) = fs::read_dir(base_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with(".sendme-send-") {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn cleanup_sendme_temp_artifacts(temp_base: &Path) {
    let Ok(entries) = fs::read_dir(&temp_base) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let managed = name.starts_with(".sendme-send-")
            || name.starts_with("databeam_sendme_")
            || name.starts_with("databeam_sendme_recv_");
        if !managed {
            continue;
        }
        if path.is_dir() {
            let _ = fs::remove_dir_all(path);
        } else {
            let _ = fs::remove_file(path);
        }
    }
    let _ = fs::remove_dir(temp_base);
}

// ── Sendme Receive ─────────────────────────────────────────────────

pub fn sendme_receive(
    opts: SendmeReceiveOptions,
    _binary_path: &str,
) -> (mpsc::Receiver<TransferMsg>, ProcessHandle) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel2 = cancel.clone();
    let child_pid = Arc::new(std::sync::Mutex::new(None));

    let _worker = thread::spawn(move || {
        let _ = tx.send(TransferMsg::Started);

        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(TransferMsg::Error(format!(
                    "Failed to create native sendme runtime: {}",
                    e
                )));
                return;
            }
        };

        let (evt_tx, evt_rx) = mpsc::channel::<NativeSendmeEvent>();
        let app_handle: NativeSendmeAppHandle = Some(Arc::new(NativeSendmeEmitter::new(evt_tx)));
        let task = runtime.spawn(native_sendme_download(
            opts.ticket.clone(),
            NativeReceiveOptions {
                output_dir: opts.output_dir.clone(),
                relay_mode: NativeRelayModeOption::Default,
                magic_ipv4_addr: None,
                magic_ipv6_addr: None,
                blob_dir: opts.blob_dir.clone(),
                overwrite: opts.overwrite,
            },
            app_handle,
        ));

        loop {
            if cancel2.load(Ordering::Relaxed) {
                task.abort();
                let _ = tx.send(TransferMsg::Error("Transfer cancelled".to_string()));
                return;
            }

            match evt_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(NativeSendmeEvent::ReceiveStarted) => {
                    let _ = tx.send(TransferMsg::Output("[2/4] Downloading...".to_string()));
                }
                Ok(NativeSendmeEvent::ReceiveProgress {
                    done,
                    total,
                    speed_bps,
                }) => {
                    if total > 0 {
                        let progress = (done as f32 / total as f32).clamp(0.0, 1.0);
                        let _ = tx.send(TransferMsg::Progress(progress));
                    }
                    let _ = tx.send(TransferMsg::Output(format!(
                        "[3/4] Downloading ... {} / {} {}",
                        format_size_unit(done),
                        format_size_unit(total.max(done).max(1)),
                        format_speed_unit(speed_bps)
                    )));
                    let _ = tx.send(TransferMsg::Output(format!(
                        "n native r {}/{} i 0 # recv",
                        done,
                        total.max(done).max(1)
                    )));
                }
                Ok(NativeSendmeEvent::ReceiveCompleted) => {
                    let _ = tx.send(TransferMsg::Progress(1.0));
                }
                Ok(NativeSendmeEvent::ReceiveExportStarted) => {
                    let _ = tx.send(TransferMsg::Output(
                        "[4/4] Writing files to disk...".to_string(),
                    ));
                }
                Ok(NativeSendmeEvent::ReceiveExportProgress { done, total }) => {
                    let _ = tx.send(TransferMsg::Output(format!(
                        "[4/4] Writing... {} / {}",
                        format_size_unit(done),
                        format_size_unit(total.max(done).max(1))
                    )));
                    if total > 0 {
                        let progress = (done as f32 / total as f32).clamp(0.0, 1.0);
                        let _ = tx.send(TransferMsg::Progress(progress));
                    }
                }
                Ok(NativeSendmeEvent::TransferStarted)
                | Ok(NativeSendmeEvent::TransferProgress { .. })
                | Ok(NativeSendmeEvent::SenderRequestStarted(_))
                | Ok(NativeSendmeEvent::SenderRequestProgress(_))
                | Ok(NativeSendmeEvent::SenderRequestCompleted(_))
                | Ok(NativeSendmeEvent::TransferCompleted)
                | Ok(NativeSendmeEvent::TransferFailed)
                | Ok(NativeSendmeEvent::ActiveConnectionCount(_)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
            }

            if task.is_finished() {
                break;
            }
        }

        while let Ok(evt) = evt_rx.try_recv() {
            if let NativeSendmeEvent::ReceiveProgress { done, total, .. } = evt {
                if total > 0 {
                    let progress = (done as f32 / total as f32).clamp(0.0, 1.0);
                    let _ = tx.send(TransferMsg::Progress(progress));
                }
            }
        }

        let result = runtime.block_on(async move { task.await });
        match result {
            Ok(Ok(receive)) => {
                let _ = tx.send(TransferMsg::Output(receive.message.to_lowercase()));
                let _ = tx.send(TransferMsg::Progress(1.0));
                let _ = tx.send(TransferMsg::Completed);
            }
            Ok(Err(e)) => {
                let _ = tx.send(TransferMsg::Error(format!("{e}")));
            }
            Err(e) => {
                let _ = tx.send(TransferMsg::Error(format!(
                    "Receive task ended unexpectedly: {}",
                    e
                )));
            }
        }
    });

    (rx, ProcessHandle { cancel, child_pid })
}

/// Tries to complete the receive by exporting blobs that are already fully cached locally.
/// Returns a channel that sends `Completed` if successful or `Error("blobs-incomplete")` if not.
/// Callers should fall back to a full `sendme_receive` if they get `blobs-incomplete`.
pub fn sendme_check_local(
    opts: SendmeReceiveOptions,
) -> (mpsc::Receiver<TransferMsg>, ProcessHandle) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel2 = cancel.clone();
    let child_pid = Arc::new(std::sync::Mutex::new(None));

    let _worker = thread::spawn(move || {
        let _ = tx.send(TransferMsg::Started);

        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                let _ =
                    tx.send(TransferMsg::Error(format!("Failed to create runtime: {}", e)));
                return;
            }
        };

        let (evt_tx, evt_rx) = mpsc::channel::<NativeSendmeEvent>();
        let app_handle: NativeSendmeAppHandle =
            Some(Arc::new(NativeSendmeEmitter::new(evt_tx)));

        let ticket = opts.ticket.clone();
        let output_dir = opts.output_dir.clone();
        let blob_dir = opts.blob_dir.clone();
        let overwrite = opts.overwrite;
        let (abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
        let task = runtime.spawn(async move {
            native_check_and_export_local_in(
                &ticket,
                output_dir,
                app_handle,
                blob_dir,
                overwrite,
                Some(abort_rx),
            )
            .await
        });

        // Forward events to the transfer channel while task runs
        loop {
            if cancel2.load(Ordering::Relaxed) {
                let _ = abort_tx.send(());
                task.abort();
                let _ = tx.send(TransferMsg::Error("Transfer cancelled".to_string()));
                return;
            }
            match evt_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(NativeSendmeEvent::ReceiveStarted) => {
                    let _ = tx.send(TransferMsg::Output(
                        "[local] Blobs complete locally, exporting...".to_string(),
                    ));
                }
                Ok(NativeSendmeEvent::ReceiveExportStarted) => {
                    let _ = tx.send(TransferMsg::Output(
                        "[4/4] Writing files to disk...".to_string(),
                    ));
                }
                Ok(NativeSendmeEvent::ReceiveExportProgress { done, total }) => {
                    let _ = tx.send(TransferMsg::Output(format!(
                        "[4/4] Writing... {} / {}",
                        format_size_unit(done),
                        format_size_unit(total.max(done).max(1))
                    )));
                    if total > 0 {
                        let _ = tx.send(TransferMsg::Progress(
                            (done as f32 / total as f32).clamp(0.0, 1.0),
                        ));
                    }
                }
                Ok(NativeSendmeEvent::ReceiveCompleted) => {
                    let _ = tx.send(TransferMsg::Progress(1.0));
                }
                _ => {}
            }
            if task.is_finished() {
                break;
            }
        }

        let result = runtime.block_on(async { task.await });
        match result {
            Ok(Ok(true)) => {
                // Successfully exported from local blobs
                let _ = tx.send(TransferMsg::Progress(1.0));
                let _ = tx.send(TransferMsg::Completed);
            }
            Ok(Ok(false)) => {
                // Blobs not complete locally — caller should do full receive
                let _ = tx.send(TransferMsg::Error("blobs-incomplete".to_string()));
            }
            Ok(Err(e)) => {
                let _ = tx.send(TransferMsg::Error(format!("{e}")));
            }
            Err(e) => {
                let _ = tx.send(TransferMsg::Error(format!("Local check task failed: {e}")));
            }
        }
    });

    (rx, ProcessHandle { cancel, child_pid })
}




pub fn cleanup_sendme_receive_artifacts_for_ticket(ticket_str: &str, blob_dir: Option<&Path>) {
    match blob_dir {
        Some(path) => native_cleanup_sendme_receive_artifacts_for_ticket_in(
            ticket_str,
            Some(path.to_path_buf()),
        ),
        None => native_cleanup_sendme_receive_artifacts_for_ticket(ticket_str),
    }
}

pub fn get_sendme_blob_directory_size(ticket_str: &str, blob_dir: Option<&Path>) -> u64 {
    match blob_dir {
        Some(path) => native_local_ticket_size_on_disk_in(ticket_str, Some(path.to_path_buf())),
        None => native_local_ticket_size_on_disk(ticket_str),
    }
}




fn parse_croc_output(line: &str, tx: &mpsc::Sender<TransferMsg>, cancel: &Arc<AtomicBool>) {
    let cleaned = strip_ansi(line);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return;
    }

    let lower = trimmed.to_lowercase();
    if lower.contains("overwrite") || lower.contains("--overwrite") {
        let _ = tx.send(TransferMsg::Error("conflict-detected".to_string()));
        cancel.store(true, Ordering::Relaxed);
        return;
    }

    if let Some(code) = extract_croc_code(trimmed) {
        let _ = tx.send(TransferMsg::Code(code));
    }

    let _ = tx.send(TransferMsg::Output(trimmed.to_string()));
}

fn extract_croc_code(trimmed: &str) -> Option<String> {
    let lower = trimmed.to_lowercase();

    if let Some(idx) = lower.find("code is") {
        let suffix = trimmed.get(idx..)?.trim();
        if let Some((_, rest)) = suffix.split_once(':') {
            let code = rest.trim();
            if !code.is_empty() {
                return Some(code.to_string());
            }
        }
    }

    if let Some(idx) = lower.find("croc_secret=") {
        let suffix = trimmed.get(idx + "croc_secret=".len()..)?.trim();
        if let Some(stripped) = suffix.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                let code = &stripped[..end];
                if !code.is_empty() {
                    return Some(code.to_string());
                }
            }
        } else if let Some(token) = suffix.split_whitespace().next() {
            let token = token.trim_matches('"');
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }

    None
}

/// Strip ANSI escape sequences from a string
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip escape sequence
            if let Some(next) = chars.next() {
                if next == '[' {
                    // CSI sequence: skip until a letter is found
                    for c2 in chars.by_ref() {
                        if c2.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                // else skip just the one char after ESC
            }
        } else if c.is_ascii_control() && c != '\t' && c != '\n' {
            // Skip other control characters
        } else {
            result.push(c);
        }
    }
    result
}

// ── Process Handle ─────────────────────────────────────────────────

pub struct ProcessHandle {
    pub cancel: Arc<AtomicBool>,
    pub child_pid: Arc<std::sync::Mutex<Option<u32>>>,
}

impl ProcessHandle {
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        // Actually kill the child process
        if let Ok(guard) = self.child_pid.lock() {
            if let Some(pid) = *guard {
                #[cfg(unix)]
                {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                }
                #[cfg(not(unix))]
                {
                    let mut cmd = new_hidden_command("taskkill");

                    let _ = cmd.args(["/PID", &pid.to_string(), "/F", "/T"]).output();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cleanup_sendme_send_dirs;
    use std::fs;

    #[test]
    fn cleanup_removes_only_sendme_send_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = tmp.path();

        let sendme_a = base.join(".sendme-send-a");
        let sendme_b = base.join(".sendme-send-b");
        let keep_dir = base.join("keep-dir");
        let keep_file = base.join(".sendme-recv-a");

        fs::create_dir_all(&sendme_a).expect("mkdir sendme a");
        fs::create_dir_all(&sendme_b).expect("mkdir sendme b");
        fs::create_dir_all(&keep_dir).expect("mkdir keep dir");
        fs::write(&keep_file, b"keep").expect("write keep file");

        cleanup_sendme_send_dirs(base);

        assert!(!sendme_a.exists());
        assert!(!sendme_b.exists());
        assert!(keep_dir.exists());
        assert!(keep_file.exists());
    }
}
