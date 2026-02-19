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

use flate2::read::GzDecoder;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

// ── Embedded Binaries ──────────────────────────────────────────────
// Embedded payloads are intentionally disabled.
// DataBeam uses managed downloads or system-installed binaries.

const CROC_GZ: &[u8] = &[];
const SENDME_GZ: &[u8] = &[];

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
        "0.1.0",
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
    let mut sendme_path = extract_bundled_binary(&managed_binary_name(&Tool::Sendme), SENDME_GZ);

    if croc_path.is_none() && which::which(Tool::Croc.name()).is_err() {
        croc_path = install_managed_binary(&Tool::Croc);
    }
    if sendme_path.is_none() && which::which(Tool::Sendme.name()).is_err() {
        sendme_path = install_managed_binary(&Tool::Sendme);
    }

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
    bundled_sendme: Option<&PathBuf>,
) -> Vec<ToolStatus> {
    vec![
        detect_tool_with_bundled(&Tool::Croc, bundled_croc),
        detect_tool_with_bundled(&Tool::Sendme, bundled_sendme),
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
}

#[derive(Debug, Clone)]
pub struct SendmeReceiveOptions {
    pub ticket: String,
    pub output_dir: Option<PathBuf>,
}

// ── Byte-level reader for \r-delimited progress ────────────────────

fn read_lines_cr_aware(
    reader: impl Read + Send + 'static,
    tx: mpsc::Sender<TransferMsg>,
    parser: fn(&str, &mpsc::Sender<TransferMsg>),
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
                        parser(&line, &tx);
                        buf.clear();
                    }
                } else {
                    buf.push(byte[0]);
                }
            }
            Err(_) => break,
        }
    }
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf).to_string();
        parser(&line, &tx);
    }
}

fn run_sendme_with_pty(
    binary: &str,
    args: &[String],
    runtime_dir: &Path,
    current_dir: Option<&Path>,
    tx: &mpsc::Sender<TransferMsg>,
    cancel: Arc<AtomicBool>,
    pid_handle: Arc<std::sync::Mutex<Option<u32>>>,
    parser: fn(&str, &mpsc::Sender<TransferMsg>),
) -> Result<(bool, String), String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Failed to create PTY: {e}"))?;

    let mut cmd = CommandBuilder::new(binary);
    cmd.env("RUST_LOG", "info");
    cmd.env("IROH_DATA_DIR", runtime_dir.to_string_lossy().to_string());
    for arg in args {
        cmd.arg(arg);
    }
    if let Some(dir) = current_dir {
        cmd.cwd(dir.to_string_lossy().to_string());
    }

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to start sendme in PTY: {e}"))?;

    if let Ok(mut guard) = pid_handle.lock() {
        *guard = child.process_id();
    }

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Failed to attach PTY reader: {e}"))?;
    let tx_reader = tx.clone();
    let cancel_reader = cancel.clone();
    let reader_thread = thread::spawn(move || {
        read_lines_cr_aware(reader, tx_reader, parser, cancel_reader);
    });

    let status = child
        .wait()
        .map_err(|e| format!("Process wait failed: {e}"))?;
    let _ = reader_thread.join();

    Ok((status.success(), format!("{status:?}")))
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
            for p in &opts.paths {
                cmd.arg(p);
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
                if let Some(stderr) = stderr {
                    thread::spawn(move || {
                        read_lines_cr_aware(stderr, tx2, parse_croc_output, c2);
                    });
                }

                let tx3 = tx.clone();
                let c3 = cancel2.clone();
                if let Some(stdout) = stdout {
                    thread::spawn(move || {
                        read_lines_cr_aware(stdout, tx3, parse_croc_output, c3);
                    });
                }

                let status = child.wait();
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
    binary_path: &str,
) -> (mpsc::Receiver<TransferMsg>, ProcessHandle) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel2 = cancel.clone();
    let child_pid = Arc::new(std::sync::Mutex::new(None));
    let pid_handle = child_pid.clone();
    let binary = binary_path.to_string();

    let _worker = thread::spawn(move || {
        // Determine what to send: single item directly, multiple items via staging dir
        let (send_path, _staging_dir) = if opts.paths.len() == 1 {
            (opts.paths[0].clone(), None)
        } else {
            // Create staging directory and copy/link all items into it
            match create_staging_dir(&opts.paths) {
                Ok((path, dir)) => (path, Some(dir)),
                Err(e) => {
                    let _ = tx.send(TransferMsg::Error(format!("Failed to stage files: {}", e)));
                    return;
                }
            }
        };

        let temp_dir = std::env::temp_dir().join(format!(
            "databeam_sendme_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        if let Err(e) = fs::create_dir_all(&temp_dir) {
            let _ = tx.send(TransferMsg::Error(format!(
                "Failed to prepare runtime directory: {}",
                e
            )));
            return;
        }
        let _ = tx.send(TransferMsg::Started);

        let send_args = vec![
            "send".to_string(),
            "-v".to_string(),
            send_path.to_string_lossy().to_string(),
        ];
        let pty_result = run_sendme_with_pty(
            &binary,
            &send_args,
            &temp_dir,
            Some(&temp_dir),
            &tx,
            cancel2.clone(),
            pid_handle.clone(),
            parse_sendme_send_output,
        );

        let status = match pty_result {
            Ok(status) => Some(status),
            Err(_) => {
                let mut cmd = new_hidden_command(&binary);
                cmd.env("RUST_LOG", "info");
                cmd.env("IROH_DATA_DIR", &temp_dir);
                cmd.arg("send");
                cmd.arg("-v");
                cmd.arg(&send_path);
                cmd.current_dir(&temp_dir);
                cmd.stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                match cmd.spawn() {
                    Ok(mut child) => {
                        if let Ok(mut guard) = pid_handle.lock() {
                            *guard = Some(child.id());
                        }

                        let stderr = child.stderr.take();
                        let stdout = child.stdout.take();

                        let tx2 = tx.clone();
                        let c2 = cancel2.clone();
                        if let Some(stderr) = stderr {
                            thread::spawn(move || {
                                read_lines_cr_aware(stderr, tx2, parse_sendme_send_output, c2);
                            });
                        }

                        let tx3 = tx.clone();
                        let c3 = cancel2.clone();
                        if let Some(stdout) = stdout {
                            thread::spawn(move || {
                                read_lines_cr_aware(stdout, tx3, parse_sendme_send_output, c3);
                            });
                        }

                        match child.wait() {
                            Ok(exit) => Some((exit.success(), format!("{exit}"))),
                            Err(e) => {
                                let _ =
                                    tx.send(TransferMsg::Error(format!("Process error: {}", e)));
                                None
                            }
                        }
                    }
                    Err(e) => {
                        let _ =
                            tx.send(TransferMsg::Error(format!("Failed to start sendme: {}", e)));
                        None
                    }
                }
            }
        };

        if cancel2.load(Ordering::Relaxed) {
            let _ = tx.send(TransferMsg::Error("Transfer cancelled".to_string()));
            cleanup_sendme_send_dirs(
                &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            );
            cleanup_runtime_dir(&temp_dir);
            return;
        }

        if let Some((success, status_text)) = status {
            if success {
                let _ = tx.send(TransferMsg::PeerDisconnected);
            } else {
                let _ = tx.send(TransferMsg::Error(format!(
                    "Send session ended with status: {}",
                    status_text
                )));
            }
        }

        cleanup_sendme_send_dirs(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        cleanup_runtime_dir(&temp_dir);
        // _staging_dir is dropped here, cleaning up the temp directory
    });

    (rx, ProcessHandle { cancel, child_pid })
}

/// Create a staging directory with all items copied/linked into it for sendme
fn create_staging_dir(paths: &[PathBuf]) -> Result<(PathBuf, tempfile::TempDir), String> {
    let staging = tempfile::Builder::new()
        .prefix("databeam_")
        .tempdir()
        .map_err(|e| format!("Could not create temp dir: {}", e))?;

    let staging_path = staging.path().to_path_buf();

    for src in paths {
        // Prevent recursion: if staging dir is inside source dir
        if staging_path.starts_with(src) {
            return Err(format!("Staging directory recursion detected. The temp dir {:?} is inside list of files to send. Please choose a different source or temp location.", staging_path));
        }

        let name = src
            .file_name()
            .ok_or_else(|| format!("Invalid path: {}", src.display()))?;
        let dest = staging_path.join(name);

        if src.is_dir() {
            copy_dir_recursive(src, &dest)
                .map_err(|e| format!("Failed to copy {}: {}", src.display(), e))?;
        } else if fs::hard_link(src, &dest).is_err() {
            fs::copy(src, &dest).map_err(|e| format!("Failed to copy {}: {}", src.display(), e))?;
        }
    }

    Ok((staging_path, staging))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if fs::hard_link(&src_path, &dst_path).is_err() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn cleanup_runtime_dir(path: &Path) {
    let _ = fs::remove_dir_all(path);
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

fn cleanup_sendme_receive_artifacts(base_dir: &Path) {
    let Ok(entries) = fs::read_dir(base_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(".sendme") {
            continue;
        }

        if path.is_dir() {
            let _ = fs::remove_dir_all(path);
        } else {
            let _ = fs::remove_file(path);
        }
    }
}

// ── Sendme Receive ─────────────────────────────────────────────────

pub fn sendme_receive(
    opts: SendmeReceiveOptions,
    binary_path: &str,
) -> (mpsc::Receiver<TransferMsg>, ProcessHandle) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel2 = cancel.clone();
    let child_pid = Arc::new(std::sync::Mutex::new(None));
    let pid_handle = child_pid.clone();
    let binary = binary_path.to_string();

    let _worker = thread::spawn(move || {
        let temp_dir = std::env::temp_dir().join(format!(
            "databeam_sendme_recv_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        if let Err(e) = fs::create_dir_all(&temp_dir) {
            let _ = tx.send(TransferMsg::Error(format!(
                "Failed to prepare runtime directory: {}",
                e
            )));
            return;
        }
        let _ = tx.send(TransferMsg::Started);

        let recv_args = vec!["receive".to_string(), "-v".to_string(), opts.ticket.clone()];
        let pty_result = run_sendme_with_pty(
            &binary,
            &recv_args,
            &temp_dir,
            opts.output_dir.as_deref(),
            &tx,
            cancel2.clone(),
            pid_handle.clone(),
            parse_sendme_receive_output,
        );

        let status = match pty_result {
            Ok(status) => Some(status),
            Err(_) => {
                let mut cmd = new_hidden_command(&binary);
                cmd.env("RUST_LOG", "info");
                cmd.env("IROH_DATA_DIR", &temp_dir);
                cmd.arg("receive");
                cmd.arg("-v");
                cmd.arg(&opts.ticket);
                if let Some(dir) = &opts.output_dir {
                    cmd.current_dir(dir);
                }
                cmd.stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                match cmd.spawn() {
                    Ok(mut child) => {
                        if let Ok(mut guard) = pid_handle.lock() {
                            *guard = Some(child.id());
                        }

                        let stderr = child.stderr.take();
                        let stdout = child.stdout.take();

                        let tx2 = tx.clone();
                        let c2 = cancel2.clone();
                        if let Some(stderr) = stderr {
                            thread::spawn(move || {
                                read_lines_cr_aware(stderr, tx2, parse_sendme_receive_output, c2);
                            });
                        }

                        let tx3 = tx.clone();
                        let c3 = cancel2.clone();
                        if let Some(stdout) = stdout {
                            thread::spawn(move || {
                                read_lines_cr_aware(stdout, tx3, parse_sendme_receive_output, c3);
                            });
                        }

                        match child.wait() {
                            Ok(exit) => Some((exit.success(), format!("{exit}"))),
                            Err(e) => {
                                let _ =
                                    tx.send(TransferMsg::Error(format!("Process error: {}", e)));
                                None
                            }
                        }
                    }
                    Err(e) => {
                        let _ =
                            tx.send(TransferMsg::Error(format!("Failed to start sendme: {}", e)));
                        None
                    }
                }
            }
        };

        if cancel2.load(Ordering::Relaxed) {
            let _ = tx.send(TransferMsg::Error("Transfer cancelled".to_string()));
            cleanup_runtime_dir(&temp_dir);
            return;
        }

        if let Some((success, status_text)) = status {
            if success {
                let _ = tx.send(TransferMsg::Progress(1.0));
                let _ = tx.send(TransferMsg::Completed);
            } else {
                let _ = tx.send(TransferMsg::Error(format!(
                    "Receive session ended with status: {}",
                    status_text
                )));
            }
        }

        if let Some(out) = &opts.output_dir {
            cleanup_sendme_receive_artifacts(&PathBuf::from(out));
        } else {
            cleanup_sendme_receive_artifacts(
                &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            );
        }
        cleanup_runtime_dir(&temp_dir);
    });

    (rx, ProcessHandle { cancel, child_pid })
}

// ── Output Parsing ─────────────────────────────────────────────────

fn parse_croc_output(line: &str, tx: &mpsc::Sender<TransferMsg>) {
    let cleaned = strip_ansi(line);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return;
    }

    // Detect known errors
    let lower = trimmed.to_lowercase();
    if lower.contains("error") || lower.contains("failed") {
        if lower.contains("could not connect") || lower.contains("connection refused") {
            let _ = tx.send(TransferMsg::Error(
                "Connection failed — check your network and ensure the relay is reachable"
                    .to_string(),
            ));
        } else if lower.contains("bad code") || lower.contains("invalid code") {
            let _ = tx.send(TransferMsg::Error(
                "Invalid code — double-check the transfer code and try again".to_string(),
            ));
        }
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

fn parse_sendme_common(line: &str, tx: &mpsc::Sender<TransferMsg>) -> Option<(String, String)> {
    // Strip ANSI escape sequences (from pty/script wrapper)
    let cleaned = strip_ansi(line);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Ignore crossterm panic messages and internal noise
    let lower = trimmed.to_lowercase();
    if lower.contains("reader source not set")
        || lower.contains("crossterm")
        || lower.contains("rust_backtrace")
        || lower.contains("panicked at")
        || lower.contains("press c to copy")
        || lower.contains("abort trap")
    {
        return None; // Don't show these as output or errors
    }

    // Detect ticket/code — sendme prints "sendme receive blob<ticket>"
    if trimmed.contains("sendme receive ") {
        if let Some(ticket_part) = trimmed.split("sendme receive ").last() {
            let ticket = ticket_part.trim().to_string();
            if !ticket.is_empty() {
                let _ = tx.send(TransferMsg::Code(ticket));
            }
        }
    } else if (trimmed.starts_with("blob") || trimmed.starts_with("Blob")) && trimmed.len() > 40 {
        // Raw blob ticket on its own line
        let _ = tx.send(TransferMsg::Code(trimmed.to_string()));
    }

    let _ = tx.send(TransferMsg::Output(trimmed.to_string()));
    Some((trimmed.to_string(), lower))
}

fn parse_sendme_send_output(line: &str, tx: &mpsc::Sender<TransferMsg>) {
    let Some((trimmed, lower)) = parse_sendme_common(line, tx) else {
        return;
    };

    // If it's stderr (heuristic: if line doesn't match standard patterns or is explicitly passed as stderr in future),
    // actually we are mixing stdout/stderr in the same pipe reader in the caller?
    // No, we have separate threads for stdout and stderr, but they both call this parser.
    // For now, let's just log everything.

    if (lower.contains("error") || lower.contains("failed") || lower.contains("aborted"))
        && (lower.contains("handshake")
            || lower.contains("incompatible")
            || lower.contains("certificatetype"))
    {
        let _ = tx.send(TransferMsg::Error(
            "Sendme version mismatch — both peers must run the same sendme version (bundled: v0.31.0)".to_string(),
        ));
    }

    if sendme_waiting_banner(&lower) {
        let _ = tx.send(TransferMsg::WaitingForReceiver);
    }

    if sendme_sender_activity_line(&trimmed, &lower) {
        let _ = tx.send(TransferMsg::SenderTransferActivity);
    }

    // Sender one-shot completion should be signaled by explicit send-finished lines.
    if lower.contains("finished sending")
        || lower.contains("transfer complete")
        || lower.contains("peer disconnected")
        || lower.contains("client disconnected")
    {
        let _ = tx.send(TransferMsg::Progress(1.0));
        let _ = tx.send(TransferMsg::PeerDisconnected);
    }
}

fn parse_sendme_receive_output(line: &str, tx: &mpsc::Sender<TransferMsg>) {
    let cleaned = strip_ansi(line);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return;
    }

    let Some((_trimmed, lower)) = parse_sendme_common(trimmed, tx) else {
        return;
    };

    if lower.starts_with("error:") {
        let _ = tx.send(TransferMsg::Error(trimmed.to_string()));
        return;
    }

    if lower.contains("error") || lower.contains("failed") || lower.contains("aborted") {
        if lower.contains("handshake")
            || lower.contains("incompatible")
            || lower.contains("certificatetype")
        {
            let _ = tx.send(TransferMsg::Error(
                "Sendme version mismatch — both peers must run the same sendme version (bundled: v0.31.0)".to_string(),
            ));
        } else if lower.contains("connection") || lower.contains("timeout") {
            let _ = tx.send(TransferMsg::Error(
                "Connection failed — check network and firewall settings".to_string(),
            ));
        }
    }

    // Some sendme builds keep the process alive briefly after this final summary line.
    // Emit completion immediately so UI can transition out of downloading state.
    if lower.contains("downloaded") && lower.contains("files") {
        let _ = tx.send(TransferMsg::Progress(1.0));
        let _ = tx.send(TransferMsg::Completed);
    }
}

fn sendme_waiting_banner(lower: &str) -> bool {
    lower.contains("waiting for incoming transfer")
        || lower.contains("waiting for incoming")
        || lower.contains("waiting for receiver")
}

fn sendme_sender_activity_line(trimmed: &str, lower: &str) -> bool {
    if (lower.contains("[3/4]") || lower.contains("[4/4]"))
        && (lower.contains("uploading") || lower.contains("downloading"))
    {
        return true;
    }
    if lower.contains("uploading ...") || lower.contains("downloading ...") {
        return true;
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    for pair in tokens.windows(2) {
        if pair[0] == "r" && pair[1].contains('/') {
            return true;
        }
    }
    false
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
    use super::{
        cleanup_sendme_send_dirs, init_bundled_binaries, parse_sendme_receive_output,
        parse_sendme_send_output, sendme_send, SendmeSendOptions, TransferMsg,
    };
    use std::fs;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

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

    #[test]
    #[ignore = "manual smoke test for local sendme behavior"]
    fn sendme_send_does_not_complete_immediately() {
        let (_, sendme_path) = init_bundled_binaries();
        let Some(binary) = sendme_path else {
            eprintln!("sendme binary unavailable on this platform; skipping");
            return;
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let sample = tmp.path().join("sample.txt");
        fs::write(&sample, b"hello").expect("write sample");

        let (rx, handle) = sendme_send(
            SendmeSendOptions {
                paths: vec![sample],
            },
            &binary.to_string_lossy(),
        );

        let start = Instant::now();
        let mut saw_early_completion = false;
        while start.elapsed() < Duration::from_secs(3) {
            match rx.try_recv() {
                Ok(TransferMsg::Completed | TransferMsg::PeerDisconnected) => {
                    saw_early_completion = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => std::thread::sleep(Duration::from_millis(30)),
            }
        }

        handle.request_cancel();
        assert!(
            !saw_early_completion,
            "sendme_send emitted completion/disconnect too early"
        );
    }

    #[test]
    fn sendme_send_parser_marks_finished_sending_as_disconnected() {
        let (tx, rx) = mpsc::channel();
        parse_sendme_send_output("finished sending to client", &tx);

        let mut saw_disconnect = false;
        for _ in 0..4 {
            match rx.try_recv() {
                Ok(TransferMsg::PeerDisconnected) => {
                    saw_disconnect = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        assert!(
            saw_disconnect,
            "expected parser to emit PeerDisconnected for finished sending"
        );
    }

    #[test]
    fn sendme_send_parser_marks_client_disconnected_as_disconnected() {
        let (tx, rx) = mpsc::channel();
        parse_sendme_send_output("client disconnected", &tx);

        let mut saw_disconnect = false;
        for _ in 0..4 {
            match rx.try_recv() {
                Ok(TransferMsg::PeerDisconnected) => {
                    saw_disconnect = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        assert!(
            saw_disconnect,
            "expected PeerDisconnected for client disconnected line"
        );
    }

    #[test]
    fn sendme_send_parser_emits_waiting_for_receiver_signal() {
        let (tx, rx) = mpsc::channel();
        parse_sendme_send_output("waiting for incoming transfer", &tx);

        let mut saw_waiting = false;
        for _ in 0..6 {
            match rx.try_recv() {
                Ok(TransferMsg::WaitingForReceiver) => {
                    saw_waiting = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        assert!(
            saw_waiting,
            "expected parser to emit WaitingForReceiver for waiting banner"
        );
    }

    #[test]
    fn sendme_send_parser_emits_sender_transfer_activity() {
        let (tx, rx) = mpsc::channel();
        parse_sendme_send_output(
            "[3/4] Uploading ... [00:03] [###>---] 300.00 MiB/600.00 MiB 10.00 MiB/s",
            &tx,
        );

        let mut saw_activity = false;
        for _ in 0..6 {
            match rx.try_recv() {
                Ok(TransferMsg::SenderTransferActivity) => {
                    saw_activity = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        assert!(
            saw_activity,
            "expected parser to emit SenderTransferActivity for upload progress line"
        );
    }

    #[test]
    fn sendme_receive_parser_emits_high_frequency_noise_lines() {
        let (tx, rx) = mpsc::channel();
        parse_sendme_receive_output(
            "n 99549f9de3 r 31724666896/0 i 474 # f9ffee9093 [] 8 B/8 B",
            &tx,
        );

        match rx.try_recv() {
            Ok(TransferMsg::Output(_)) => {}
            other => panic!("expected output message, got {other:?}"),
        }
    }

    #[test]
    fn sendme_receive_parser_marks_downloaded_files_as_completed() {
        let (tx, rx) = mpsc::channel();
        parse_sendme_receive_output("downloaded 7 files, 595.45 MiB. took 5 seconds", &tx);

        let mut saw_completed = false;
        for _ in 0..6 {
            match rx.try_recv() {
                Ok(TransferMsg::Completed) => {
                    saw_completed = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        assert!(
            saw_completed,
            "expected receive parser to emit Completed for downloaded files line"
        );
    }

    #[test]
    fn sendme_receive_parser_emits_error_message_for_error_prefix_line() {
        let (tx, rx) = mpsc::channel();
        parse_sendme_receive_output("error: ticket not found", &tx);

        let mut saw_error = false;
        for _ in 0..6 {
            match rx.try_recv() {
                Ok(TransferMsg::Error(msg)) => {
                    saw_error = msg.to_lowercase().starts_with("error:");
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        assert!(
            saw_error,
            "expected receive parser to emit Error for error: line"
        );
    }
}
