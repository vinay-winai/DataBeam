use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{BufReader, Cursor, Read, Write};
use std::net::ToSocketAddrs;
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
use rand::seq::IndexedRandom;
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
use sha2::{Digest, Sha256};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

// ── Embedded Binaries ──────────────────────────────────────────────
// Release builds embed the reproducibly built, patched engine. build.rs
// generates its identity and hashes from patches/croc-engine.json.
include!(concat!(env!("OUT_DIR"), "/croc_payload.rs"));
pub const CROC_COMPATIBILITY_PINNED: bool = true;

/// Get the cache directory for extracted binaries
fn bundled_bin_dir() -> PathBuf {
    let base = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("databeam").join("bin")
}

pub(crate) fn new_hidden_command(program: impl AsRef<OsStr>) -> Command {
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

fn restore_bundled_croc_to(output: &Path, gzip: &[u8], expected_sha256: &str) -> Result<PathBuf, String> {
    if gzip.is_empty() {
        return Err("This development build has no patched Croc payload. Run scripts/prepare_croc.py and rebuild DataBeam.".to_string());
    }
    let hash = |bytes: &[u8]| -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    };
    if fs::read(output).ok().is_some_and(|bytes| hash(&bytes) == expected_sha256) {
        set_executable_permissions(output);
        return Ok(output.to_path_buf());
    }
    let parent = output.parent().ok_or("Croc cache path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| format!("Create Croc cache: {e}"))?;
    let mut staged = tempfile::Builder::new().prefix("croc-bundle-")
        .suffix(if cfg!(windows) { ".exe" } else { "" }).tempfile_in(parent)
        .map_err(|e| format!("Stage bundled Croc: {e}"))?;
    let mut bytes = Vec::new();
    GzDecoder::new(gzip).read_to_end(&mut bytes).map_err(|e| format!("Decode bundled Croc: {e}"))?;
    if hash(&bytes) != expected_sha256 {
        return Err("Bundled Croc SHA-256 mismatch".to_string());
    }
    staged.write_all(&bytes).map_err(|e| format!("Write bundled Croc: {e}"))?;
    set_executable_permissions(staged.path());
    staged.persist(output).map_err(|e| format!("Restore bundled Croc: {}", e.error))?;
    Ok(output.to_path_buf())
}

fn restore_bundled_croc() -> Result<PathBuf, String> {
    restore_bundled_croc_to(&managed_binary_path(&Tool::Croc), CROC_GZ, CROC_BINARY_SHA256)
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
        .map(|version| format!("iroh 1.0.0 (sendme {version})"))
        .or_else(|| Some("iroh 1.0.0 (native)".to_string()));

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
    let base = if matches!(tool, Tool::Croc) {
        bundled_bin_dir().join(format!("croc-{SUPPORTED_CROC_VERSION}-{}", std::env::consts::ARCH))
    } else {
        bundled_bin_dir()
    };
    base.join(managed_binary_name(tool))
}

/// True when `path` is our own managed croc binary (not a system install).
/// Legacy migration must only ever replace the managed copy — re-downloading
/// every 6h because a *system* croc predates `update` would be pure waste.
pub fn is_managed_croc_binary(path: &str) -> bool {
    managed_binary_path(&Tool::Croc).to_string_lossy() == path
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

fn fetch_release(tool: &Tool) -> Result<GitHubRelease, String> {
    let release = if matches!(tool, Tool::Croc) && CROC_COMPATIBILITY_PINNED {
        format!("tags/{SUPPORTED_CROC_VERSION}")
    } else {
        "latest".to_string()
    };
    let url = format!(
        "https://api.github.com/repos/{}/releases/{release}", github_repo(tool)
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
        .args(["-fsSL", "--connect-timeout", "20", "--max-time", "180", "-A", "databeam", url])
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
        "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -UseBasicParsing -TimeoutSec 180 -Headers @{{'User-Agent'='Mozilla/5.0 (Windows NT 10.0; Win64; x64) DataBeam/{}'; 'Accept'='application/vnd.github+json, application/octet-stream'}} -Uri '{}' -OutFile '{}'",
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

/// Non-empty cached managed binary, if present. Never touches the network.
/// This path never launches subprocesses; version/quarantine work runs later.
fn cached_managed_binary(tool: &Tool) -> Option<PathBuf> {
    let path = managed_binary_path(tool);
    if path.exists() && fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
        Some(path)
    } else {
        None
    }
}

/// macOS tags files downloaded by an app with com.apple.quarantine; the
/// first exec then stalls in Gatekeeper verification (on the calling
/// thread). Managed binaries are built from verified upstream source, so
/// clearing the flag best-effort is safe.
#[cfg(target_os = "macos")]
fn strip_quarantine(path: &Path) {
    let _ = std::process::Command::new("xattr")
        .args(["-d", "com.apple.quarantine"])
        .arg(path)
        .output();
}

pub fn install_managed_binary(tool: &Tool) -> Option<PathBuf> {
    if matches!(tool, Tool::Croc) && CROC_COMPATIBILITY_PINNED {
        return restore_bundled_croc().map_err(|e| eprintln!("Croc install failed: {e}")).ok();
    }
    if let Some(cached) = cached_managed_binary(tool) {
        return Some(cached);
    }
    download_managed_binary(tool).map_err(|e| {
        eprintln!("{} install failed: {e}", tool.name());
    }).ok()
}

fn download_managed_binary(tool: &Tool) -> Result<PathBuf, String> {
    if matches!(tool, Tool::Croc) && CROC_COMPATIBILITY_PINNED {
        return restore_bundled_croc();
    }
    let output_path = managed_binary_path(tool);
    fs::create_dir_all(output_path.parent().ok_or("Binary path has no parent")?)
        .map_err(|e| format!("Create binary cache: {e}"))?;
    let release = fetch_release(tool)?;
    let asset = find_release_asset(tool, &release).ok_or_else(|| format!(
        "{} has no release asset for {}-{} in {}",
        tool.name(), std::env::consts::OS, std::env::consts::ARCH, release.tag_name
    ))?;
    let bytes = download_bytes(&asset.browser_download_url)?;
    stage_managed_binary(tool, &asset.name, &bytes, &release.tag_name, &output_path)?;
    Ok(output_path)
}

/// Stage beside the destination so replacement is atomic and a failed
/// download, extraction or version probe leaves the working copy intact.
fn stage_managed_binary(
    tool: &Tool, asset_name: &str, bytes: &[u8], release_tag: &str, output_path: &Path,
) -> Result<(), String> {
    let parent = output_path.parent().ok_or("Binary path has no parent")?;
    let staged = tempfile::Builder::new().prefix("croc-stage-")
        .suffix(if cfg!(windows) { ".exe" } else { "" }).tempfile_in(parent)
        .map_err(|e| format!("Stage binary: {e}"))?.into_temp_path();
    if asset_name.ends_with(".tar.gz") {
        extract_binary_from_tar_gz(bytes, &managed_binary_name(tool), &staged)?;
    } else if asset_name.ends_with(".zip") {
        extract_binary_from_zip(bytes, &managed_binary_name(tool), &staged)?;
    } else {
        return Err(format!("Unsupported archive format: {asset_name}"));
    }
    if fs::metadata(&staged).map_err(|e| e.to_string())?.len() == 0 {
        return Err("Downloaded binary is empty".to_string());
    }
    set_executable_permissions(&staged);
    #[cfg(target_os = "macos")]
    strip_quarantine(&staged);
    if matches!(tool, Tool::Croc) {
        let reported = croc_version_string(&staged.to_string_lossy())
            .ok_or("Downloaded croc could not report its version")?;
        verify_croc_version(&reported, release_tag)?;
    }
    staged.persist(output_path).map_err(|e| format!("Replace cached binary: {}", e.error))?;
    Ok(())
}

// ── Croc Self-Update (`croc update`) ─────────────────────────────────
// Managed croc is compatibility-pinned. The self-update machinery remains
// available for a future verified release, but cannot replace the pin:
//
//   1. `croc update --check`  → detect whether an update is available.
//   2. `croc update --yes`    → apply it non-interactively.
//
// Restoring a managed copy extracts the bundled patch while pinned.

/// Result of `croc update --check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrocUpdateCheck {
    Pinned { version: String },
    UpToDate {
        version: String,
        raw: String,
    },
    Available {
        current: String,
        latest: String,
        raw: String,
    },
    /// Binary is package-managed / non-writable: croc prints upgrade guidance
    /// instead of self-updating. Never attempt to overwrite it.
    NotWritable {
        raw: String,
    },
    /// Binary predates the `update` subcommand (or otherwise doesn't support
    /// it). Caller should migrate via managed re-download instead.
    Unsupported {
        raw: String,
    },
    Unknown {
        raw: String,
    },
}

/// Outcome of `croc update --yes` (or interactive `croc update` + `y`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrocUpdateApplyOutcome {
    pub updated: bool,
    pub already_up_to_date: bool,
    pub not_writable: bool,
    /// croc refused to self-update for environment reasons (verified live:
    /// Windows always refuses with "cannot safely replace the running
    /// executable in place"; Unix refuses unless the binary carries the
    /// official-installer registration). Caller should fall back to a
    /// managed re-download (managed copy only) instead of retrying.
    pub needs_redownload: bool,
    pub new_version: Option<String>,
    pub raw: String,
}

/// Extract `(current, latest)` from an availability line.
/// Expected: "croc v11.5.0 is available (current: v11.2.5)."
/// Operates on the lowercased line throughout so non-ASCII input can never
/// misalign byte indices (version tokens are ASCII by construction).
fn extract_croc_versions_from_available_line(lower: &str) -> Option<(String, String)> {
    let avail_idx = lower.find("is available")?;
    let current_idx = lower.find("(current:")?;
    let before = lower[..avail_idx].trim();
    let latest = before
        .split_whitespace()
        .last()?
        .trim()
        .trim_matches(['.', ',', ';', ')', '('])
        .to_string();
    let after = &lower[current_idx + "(current:".len()..];
    let current = after
        .split_whitespace()
        .next()?
        .trim()
        .trim_matches(['.', ',', ';', ')', '(', ':'])
        .to_string();
    if latest.is_empty() || current.is_empty() {
        return None;
    }
    Some((current, latest))
}

fn contains_package_manager_guidance(lower: &str) -> bool {
    lower.contains("upgrade with")
        || lower.contains("package-manager")
        || lower.contains("package manager")
        || lower.contains("will not be overwritten")
        || lower.contains("not be overwritten")
        || lower.contains("use your package manager")
        || lower.contains("install a stable croc release")
        || lower.contains("cannot update development build")
}

fn looks_like_legacy_receive_prompt(output: &str) -> bool {
    let lower = output.to_lowercase();
    lower.contains("enter receive code") || lower.contains("could not read receive code")
}

/// True when croc declined to self-update for environment reasons rather
/// than failing. Verified against 11.5.x source (`registeredWritableTarget`)
/// and live on Windows: unconditional refusal there, registration/writability
/// probes elsewhere. None of these are fixed by retrying or piping `y`.
fn is_self_update_refusal(lower: &str) -> bool {
    lower.contains("cannot safely replace the running executable")
        || lower.contains("not registered as an official standalone install")
        || lower.contains("registration does not match the running executable")
        || lower.contains("not a replaceable regular file")
        || lower.contains("not writable without privilege escalation")
        || lower.contains("directory is not writable without privilege escalation")
}

/// Pure parser for `croc update --check` output. Unit-tested below.
pub fn parse_croc_update_check_output(output: &str) -> CrocUpdateCheck {
    let raw = output.to_string();
    let lower = output.to_lowercase();
    if looks_like_legacy_receive_prompt(output)
        || lower.contains("unknown command")
        || (lower.contains("incorrect usage") && lower.contains("update"))
    {
        return CrocUpdateCheck::Unsupported { raw };
    }
    if lower.contains("is up to date") {
        let version = lower
            .split_whitespace()
            .find(|t| {
                let c = t.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != 'v');
                (c.starts_with('v') || c.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
                    && c.contains('.')
            })
            .unwrap_or("")
            .trim()
            .trim_matches(['.', ',', ';', ')', '('])
            .to_string();
        return CrocUpdateCheck::UpToDate { version, raw };
    }
    if lower.contains("is available") {
        if contains_package_manager_guidance(&lower) {
            return CrocUpdateCheck::NotWritable { raw };
        }
        if let Some((current, latest)) = extract_croc_versions_from_available_line(&lower) {
            return CrocUpdateCheck::Available {
                current,
                latest,
                raw,
            };
        }
        return CrocUpdateCheck::Unknown { raw };
    }
    if contains_package_manager_guidance(&lower) {
        return CrocUpdateCheck::NotWritable { raw };
    }
    CrocUpdateCheck::Unknown { raw }
}

/// Pure parser for `croc update` apply output. Unit-tested below.
pub fn parse_croc_update_apply_output(output: &str) -> CrocUpdateApplyOutcome {
    let lower = output.to_lowercase();
    if looks_like_legacy_receive_prompt(output) {
        return CrocUpdateApplyOutcome {
            updated: false,
            already_up_to_date: false,
            not_writable: false,
            needs_redownload: false,
            new_version: None,
            raw: output.to_string(),
        };
    }
    let already_up_to_date = lower.contains("is up to date");
    let needs_redownload = !already_up_to_date && is_self_update_refusal(&lower);
    // A refused standalone update also prints "Upgrade with ..." guidance.
    // Keep the refusal distinct so our managed copy reaches the fallback.
    let not_writable = !needs_redownload && contains_package_manager_guidance(&lower);
    let mut updated = lower.contains("updated croc from");
    let mut new_version: Option<String> = None;
    // "Updated croc from v11.2.5 to v11.5.0."
    if let Some(idx) = lower.find("updated croc from") {
        let tail = &lower[idx + "updated croc from".len()..];
        let parts: Vec<&str> = tail.split_whitespace().collect();
        if parts.len() >= 3 {
            let to = parts[2].trim().trim_matches(['.', ',', ';', ')', '(']).to_string();
            if !to.is_empty() {
                new_version = Some(to);
            }
        }
    }
    if already_up_to_date || not_writable || needs_redownload {
        updated = false;
    }
    CrocUpdateApplyOutcome {
        updated,
        already_up_to_date,
        not_writable,
        needs_redownload,
        new_version,
        raw: output.to_string(),
    }
}

fn run_croc_command(binary: &str, args: &[&str], stdin_input: Option<&[u8]>) -> Result<String, String> {
    use std::io::{Read, Write};
    let mut cmd = new_hidden_command(binary);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if stdin_input.is_some() { Stdio::piped() } else { Stdio::null() });
    cmd.current_dir(std::env::temp_dir());
    let mut child = cmd.spawn().map_err(|e| format!("Failed to start croc: {e}"))?;
    if let Some(input) = stdin_input {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input);
        }
    }
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let err_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        bytes
    });
    let limit = if args == ["--version"] { 10 } else { 210 };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(limit);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("croc {} timed out after {limit}s", args.join(" ")));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("Failed to wait for croc: {e}"));
            }
        }
    };
    let out = out_reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    let mut combined = String::from_utf8_lossy(&out).to_string();
    if !err.is_empty() {
        if !combined.is_empty() { combined.push('\n'); }
        combined.push_str(&String::from_utf8_lossy(&err));
    }
    let status = status?;
    // Old croc versions reject `update` with a nonzero status; callers need
    // that output for migration. Actual network/process errors must surface.
    if !status.success()
        && !matches!(parse_croc_update_check_output(&combined), CrocUpdateCheck::Unsupported { .. } | CrocUpdateCheck::NotWritable { .. })
        && !is_self_update_refusal(&combined.to_lowercase())
    {
        return Err(format!("croc exited with {status}: {}", combined.trim()));
    }
    Ok(combined)
}

/// Run `croc update --check` and parse the result.
pub fn croc_update_check(binary: &str) -> Result<CrocUpdateCheck, String> {
    if CROC_COMPATIBILITY_PINNED {
        return Ok(CrocUpdateCheck::Pinned { version: SUPPORTED_CROC_VERSION.to_string() });
    }
    let output = run_croc_command(binary, &["update", "--check"], None)?;
    Ok(parse_croc_update_check_output(&output))
}

/// Run the update itself. Prefers non-interactive `--yes`; falls back to
/// piping `y` into `croc update` exactly as a user answering the
/// `Update croc from vX to vY? (y/N)` prompt would. Runs at most two croc
/// invocations: a decline/cancel surfaces as `updated == false`, which the
/// caller treats as a no-op.
pub fn croc_update_apply(binary: &str) -> Result<CrocUpdateApplyOutcome, String> {
    if CROC_COMPATIBILITY_PINNED {
        return Err(format!("croc {SUPPORTED_CROC_VERSION} is pinned for relay compatibility"));
    }
    // Fast path: `--yes` skips the prompt when self-update is safe.
    let output = run_croc_command(binary, &["update", "--yes"], None)?;
    let mut outcome = parse_croc_update_apply_output(&output);
    if outcome.updated || outcome.already_up_to_date || outcome.not_writable || outcome.needs_redownload {
        return verify_croc_update_result(binary, outcome);
    }
    // If --yes was rejected (very old/new flag variants) or the prompt is
    // still pending, answer `y` explicitly.
    if output.to_lowercase().contains("update croc from")
        || output.to_lowercase().contains("(y/n)")
        || output.to_lowercase().contains("requires an interactive terminal")
        || (!outcome.updated && output.trim().is_empty())
    {
        let fallback = run_croc_command(binary, &["update"], Some(b"y\n"))?;
        outcome = parse_croc_update_apply_output(&fallback);
    }
    verify_croc_update_result(binary, outcome)
}

fn verify_croc_update_result(binary: &str, outcome: CrocUpdateApplyOutcome) -> Result<CrocUpdateApplyOutcome, String> {
    if outcome.updated {
        let expected = outcome.new_version.as_deref().ok_or("croc reported success without a version")?;
        let reported = croc_version_string(binary).ok_or("Updated croc could not report its installed version")?;
        verify_croc_version(&reported, expected)?;
    }
    Ok(outcome)
}

fn verify_croc_version(reported: &str, expected: &str) -> Result<(), String> {
    let expected = format!("croc version {}", expected.trim_start_matches('v'));
    if reported.trim() != expected {
        return Err(format!("croc reports {reported:?}; expected {expected:?}"));
    }
    Ok(())
}

/// Best-effort `croc --version` string (without managed/system suffix).
pub fn croc_version_string(binary: &str) -> Option<String> {
    let output = run_croc_command(binary, &["--version"], None).ok()?;
    let version = output.trim();
    (!version.is_empty()).then(|| version.to_string())
}

/// Restore the bundled patched engine (download latest only when unpinned).
pub fn refresh_managed_croc_to_latest() -> Result<PathBuf, String> {
    download_managed_binary(&Tool::Croc)
}

/// Re-run version detection for croc after an update so the home page
/// `tool_card` shows the new version immediately.
pub fn redetect_croc_status(bundled_croc: Option<&PathBuf>) -> ToolStatus {
    detect_tool_with_bundled(&Tool::Croc, bundled_croc)
}

/// Startup extraction and cached reuse; never touches the network or executes
/// Croc. Version probes and quarantine handling run on a worker.
pub fn init_bundled_binaries_fast() -> (Option<PathBuf>, Option<PathBuf>) {
    let mut croc_path = if CROC_GZ.is_empty() { None } else { restore_bundled_croc().ok() };

    // Prefer managed Croc binaries on every platform for consistent behavior.
    // Only a version-verified system copy may stand in during background install.
    if croc_path.is_none() && CROC_GZ.is_empty() {
        croc_path = cached_managed_binary(&Tool::Croc);
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
            #[cfg(target_os = "macos")]
            strip_quarantine(bp);
            let version = croc_version_string(&bp.to_string_lossy())
                .map(|version| format!("{version} (managed)"));

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
    let reported = if let Some(path) = sys_path {
        croc_version_string(&path.to_string_lossy())
    } else {
        None
    };
    let available = path_str.is_some() && (!matches!(tool, Tool::Croc)
        || reported.as_deref().is_some_and(|version| verify_croc_version(version, SUPPORTED_CROC_VERSION).is_ok()));
    let version = reported.map(|version| format!("{version} (system)"));

    ToolStatus {
        tool: tool.clone(),
        available,
        path: path_str,
        version,
    }
}

/// Initial status without executing croc: safe before first paint.
pub fn detect_tool_paths(bundled_croc: Option<&PathBuf>) -> Vec<ToolStatus> {
    // A system copy is only eligible after background version verification.
    let path = bundled_croc.filter(|path| path.exists()).cloned();
    vec![ToolStatus {
        tool: Tool::Croc,
        available: path.is_some(),
        path: path.map(|path| path.to_string_lossy().to_string()),
        version: None,
    }, detect_native_sendme()]
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
    ReceivedText(String),
    Progress(f32),
    Code(String),
    Completed,
    Error(String),
    Started,
    PeerDisconnected,
    WaitingForReceiver,
    SenderTransferActivity,
    ActiveTransfers(usize),
    SendmeRoute(Option<SendmeRoute>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendmeRoute {
    Direct,
    Relay,
}

impl SendmeRoute {
    pub fn label(self) -> &'static str {
        match self {
            Self::Direct => "Direct",
            Self::Relay => "Relay",
        }
    }
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
    ReceiveRoute(Option<SendmeRoute>),
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
            "receive-route" => NativeSendmeEvent::ReceiveRoute(match payload {
                "direct" => Some(SendmeRoute::Direct),
                "relay" => Some(SendmeRoute::Relay),
                "unknown" => None,
                _ => return Ok(()),
            }),
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
    parser: impl Fn(&str, &mpsc::Sender<TransferMsg>, &Arc<AtomicBool>),
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

// ── Croc transfers ─────────────────────────────────────────────────

#[derive(Default)]
struct CrocAttemptOutput {
    code: Option<String>,
    error: Option<String>,
    error_from_stdout: bool,
    payload_started: bool,
    receiving_text: bool,
    stdout_read_error: Option<String>,
}

impl CrocAttemptOutput {
    fn observe(&mut self, line: &str, is_stdout: bool) {
        let cleaned = strip_ansi(line);
        let line = cleaned.trim();
        if let Some(code) = extract_croc_code(line) {
            self.code = Some(code);
        }
        self.payload_started |= parse_croc_direction(line).is_some();
        if !is_stdout && line.starts_with("Receiving text message (") {
            self.receiving_text = true;
        }
        let lower = line.to_lowercase();
        if lower.contains("could not connect")
            || lower.starts_with("could not ")
            || lower.contains("relay connection failed")
            || lower.contains("decompress")
            || lower.contains("corrupt input")
            || lower.contains("error:")
            || lower.contains("failed:")
            || lower.contains("room not found")
            || lower.contains("room is full")
            || lower.contains("connection reset")
            || lower.contains("connection refused")
            || lower.contains("no public relay")
            || lower.contains("relay closed")
            || lower == "eof"
        {
            // croc prints its returned error on stdout at exit. A stderr
            // reader draining older warnings later must not replace it.
            if lower.starts_with("close decompressor:") && self.error_from_stdout {
                return; // Preserve the primary decoding error, not its duplicate close error.
            }
            if is_stdout || !self.error_from_stdout {
                self.error = Some(line.to_owned());
                self.error_from_stdout = is_stdout;
            }
        }
    }

    fn dns_relay(&self) -> Option<String> {
        if self.payload_started {
            return None;
        }
        relay_dns_endpoint(self.error.as_deref()?)
    }
}

/// Only retry the exact public relay named in a terminal DNS error. Croc's
/// code-to-relay routing must not be changed, or the peers join different rooms.
fn relay_dns_endpoint(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let (_, lookup) = lower.split_once("lookup ")?;
    let (host, cause) = lookup.split_once(':')?;
    let host = host.trim();
    if ![
        "i/o timeout",
        "no such host",
        "server misbehaving",
        "temporary failure",
    ]
    .iter()
    .any(|cause_text| cause.contains(cause_text))
    {
        return None;
    }
    if !matches!(
        host,
        "1.getcroc.com"
            | "2.getcroc.com"
            | "3.getcroc.com"
            | "4.getcroc.com"
            | "croc.schollz.com"
            | "croc6.schollz.com"
            | "ipv4.getcroc.com"
            | "ipv6.getcroc.com"
    ) {
        return None;
    }
    // Keep the original port, including non-default deployments. Do not infer
    // an endpoint from just an unrelated mention of a DNS failure in a log.
    let (_, destination) = lower.split_once("could not connect to ")?;
    let endpoint = destination.split(": ").next()?;
    let (endpoint_host, port) = endpoint.rsplit_once(':')?;
    if endpoint_host != host || port.parse::<u16>().ok()? == 0 {
        return None;
    }
    Some(endpoint.to_owned())
}

fn resolve_croc_relay(endpoint: &str) -> Option<String> {
    // Rust uses the host resolver. Go's netgo releases can time out even while
    // the host can resolve the same name. Resolve anew, never hardcode relay IPs.
    let addresses: Vec<_> = endpoint.to_socket_addrs().ok()?.collect();
    addresses
        .iter()
        .find(|address| address.is_ipv4())
        .or_else(|| addresses.first())
        .map(ToString::to_string)
}

static CROC_CODE_WORDS: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    include_str!("../assets/wordlists/eff-short-wordlist-1.txt")
        .lines()
        .collect()
});

fn random_croc_code() -> String {
    // Match croc's default: three independent, uniformly selected EFF words.
    // Generate locally so relay selection can use system DNS without croc's
    // one-second probe race. rand::rng() is seeded from operating-system entropy.
    let mut rng = rand::rng();
    let words: [&str; 3] = std::array::from_fn(|_| {
        CROC_CODE_WORDS
            .choose(&mut rng)
            .copied()
            .expect("croc word list is non-empty")
    });
    words.join("-")
}

/// Current croc web/CLI routing: SHA-256 of the exact UTF-8 code, interpreted
/// big-endian, modulo the ordered four-relay pool. Pick the same relay as web
/// peers without depending on croc's short probe race or its Go DNS resolver.
fn croc_relay_for_code(code: &str) -> &'static str {
    const RELAYS: [&str; 4] = [
        "1.getcroc.com:9009",
        "2.getcroc.com:9009",
        "3.getcroc.com:9009",
        "4.getcroc.com:9009",
    ];
    let digest = Sha256::digest(code.as_bytes());
    RELAYS[(digest[digest.len() - 1] % 4) as usize]
}

fn croc_transfer_command(binary: &str, code: Option<&str>, relay: Option<&str>) -> Command {
    let mut cmd = new_hidden_command(binary);
    // A GUI must not let croc treat an inherited pipe as the payload instead
    // of the selected files or --text. All these flags are global.
    cmd.args(["--yes", "--ignore-stdin"]);
    let custom_relay = ["CROC_RELAY", "CROC_RELAY6"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
    let default_relay = if relay.is_none() && !custom_relay {
        code.map(|code| {
            let endpoint = croc_relay_for_code(code);
            resolve_croc_relay(endpoint).unwrap_or_else(|| endpoint.to_string())
        })
    } else {
        None
    };
    if let Some(relay) = relay.or(default_relay.as_deref()) {
        cmd.arg("--relay").arg(relay);
    }
    if let Some(code) = code {
        cmd.env("CROC_SECRET", code);
    } else {
        cmd.env_remove("CROC_SECRET");
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.current_dir(std::env::temp_dir());
    cmd
}

fn spawn_croc_transfer(
    initial_code: Option<String>,
    announce_code: bool,
    make_command: impl Fn(Option<&str>, Option<&str>) -> Command + Send + 'static,
    resolve_relay: impl Fn(&str) -> Option<String> + Send + 'static,
) -> (mpsc::Receiver<TransferMsg>, ProcessHandle) {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel2 = cancel.clone();
    let child_pid = Arc::new(std::sync::Mutex::new(None));
    let pid_handle = child_pid.clone();

    thread::spawn(move || {
        let mut code = initial_code;
        let mut relay_override: Option<String> = None;
        let mut dns_retried = false;
        let _ = tx.send(TransferMsg::Started);
        // At most one probe fallback and one DNS fallback, before payload
        // transfer only. Never replay a partially completed transfer.
        for _ in 0..3 {
            if cancel2.load(Ordering::Relaxed) {
                break;
            }
            let mut cmd = make_command(code.as_deref(), relay_override.as_deref());
            let mut child = match cmd.spawn() {
                Ok(child) => child,
                Err(error) => {
                    let _ = tx.send(TransferMsg::Error(format!("Failed to start croc: {error}")));
                    return;
                }
            };
            if let Ok(mut pid) = pid_handle.lock() {
                *pid = Some(child.id());
                // Cover cancellation between the pre-spawn check and publishing
                // the PID so a cancelled recovery cannot leave a child behind.
                if cancel2.load(Ordering::Relaxed) {
                    let _ = child.kill();
                }
            }
            track_child_pid(child.id());
            // The sender already knows its secret. Publish it independently
            // of croc's human-readable command/clipboard output.
            if announce_code {
                if let Some(code) = &code {
                    let _ = tx.send(TransferMsg::Code(code.clone()));
                }
            }
            let output = Arc::new(std::sync::Mutex::new(CrocAttemptOutput::default()));
            let received_stdout = Arc::new(std::sync::Mutex::new(Vec::new()));
            let readers: Vec<_> = [
                child
                    .stdout
                    .take()
                    .map(|r| (true, Box::new(r) as Box<dyn Read + Send>)),
                child
                    .stderr
                    .take()
                    .map(|r| (false, Box::new(r) as Box<dyn Read + Send>)),
            ]
            .into_iter()
            .flatten()
            .map(|(is_stdout, reader)| {
                let tx = tx.clone();
                let cancel = cancel2.clone();
                let output = output.clone();
                let received_stdout = received_stdout.clone();
                thread::spawn(move || {
                    if is_stdout && !announce_code {
                        // Croc writes the exact text payload to stdout, while
                        // transfer status/prompts use stderr. Do not split,
                        // trim, strip ANSI, or interpret message text as logs.
                        let mut reader = reader;
                        let mut bytes = Vec::new();
                        if let Err(error) = reader.read_to_end(&mut bytes) {
                            output.lock().unwrap_or_else(|e| e.into_inner()).stdout_read_error =
                                Some(format!("Read Croc text: {error}"));
                        }
                        *received_stdout.lock().unwrap_or_else(|e| e.into_inner()) = bytes;
                        return;
                    }
                    read_lines_cr_aware(
                        reader,
                        tx,
                        move |line, tx, cancel| {
                            if let Ok(mut output) = output.lock() {
                                output.observe(line, is_stdout);
                            }
                            parse_croc_output(line, tx, cancel);
                        },
                        cancel,
                    )
                })
            })
            .collect();
            let status = child.wait();
            untrack_child_pid(child.id());
            if let Ok(mut pid) = pid_handle.lock() {
                *pid = None;
            }
            for reader in readers {
                let _ = reader.join();
            }
            if cancel2.load(Ordering::Relaxed) {
                break;
            }
            let status = match status {
                Ok(status) => status,
                Err(error) => {
                    let _ = tx.send(TransferMsg::Error(format!("Process error: {error}")));
                    return;
                }
            };
            if !announce_code {
                if let Some(error) = output.lock().unwrap_or_else(|e| e.into_inner()).stdout_read_error.clone() {
                    let _ = tx.send(TransferMsg::Error(error));
                    return;
                }
                let bytes = std::mem::take(&mut *received_stdout.lock().unwrap_or_else(|e| e.into_inner()));
                let is_text = output.lock().unwrap_or_else(|e| e.into_inner()).receiving_text;
                if status.success() && is_text {
                    match String::from_utf8(bytes) {
                        Ok(text) => { let _ = tx.send(TransferMsg::ReceivedText(text)); }
                        Err(_) => {
                            let _ = tx.send(TransferMsg::Error("Received text is not valid UTF-8".into()));
                            return;
                        }
                    }
                } else {
                    // Returned CLI errors still need classification for the
                    // existing DNS recovery path, after the process finishes.
                    for line in String::from_utf8_lossy(&bytes).lines() {
                        output.lock().unwrap_or_else(|e| e.into_inner()).observe(line, true);
                        parse_croc_output(line, &tx, &cancel2);
                    }
                }
            }
            if status.success() {
                let _ = tx.send(TransferMsg::Completed);
                return;
            }
            let output = output.lock().unwrap_or_else(|error| error.into_inner());
            if code.is_none() {
                code = output.code.clone();
            }
            if !output.payload_started
                && code.is_none()
                && output
                    .error
                    .as_deref()
                    .is_some_and(|error| error.to_lowercase().contains("no public relay available"))
            {
                // Newer croc releases give their public-relay probe race only
                // one second. A custom secret bypasses that race and lets croc
                // use its normal connection timeout and code-to-relay routing.
                code = Some(random_croc_code());
                let _ = tx.send(TransferMsg::Output(
                    "Public relay probes failed; retrying with a fresh random code and croc's normal connection timeout.".to_string()
                ));
                continue;
            }
            if !dns_retried {
                if let Some(endpoint) = output.dns_relay() {
                    dns_retried = true;
                    if code.is_some() {
                        relay_override = resolve_relay(&endpoint);
                    }
                    let detail = if relay_override.is_some() {
                        "retrying the same relay with the system-resolved address"
                    } else {
                        "retrying croc once"
                    };
                    let _ = tx.send(TransferMsg::Output(format!(
                        "Relay DNS lookup failed; {detail}."
                    )));
                    continue;
                }
            }
            let error = output
                .error
                .clone()
                .unwrap_or_else(|| format!("Process exited with code: {status}"));
            let _ = tx.send(TransferMsg::Error(error));
            return;
        }
        let _ = tx.send(TransferMsg::Error("Transfer cancelled".to_string()));
    });
    (rx, ProcessHandle { cancel, child_pid })
}

pub fn croc_send(
    opts: CrocSendOptions,
    binary_path: &str,
) -> (mpsc::Receiver<TransferMsg>, ProcessHandle) {
    let binary = binary_path.to_owned();
    let code = opts.custom_code.clone().unwrap_or_else(random_croc_code);
    spawn_croc_transfer(
        Some(code),
        true,
        move |code, relay| {
            let mut cmd = croc_transfer_command(&binary, code, relay);
            cmd.arg("send");
            if opts.text_mode {
                if let Some(text) = &opts.text_value {
                    cmd.arg("--text").arg(text);
                }
            } else {
                // Stop option parsing so a filename beginning with '-' is a path.
                cmd.arg("--").args(&opts.paths);
            }
            cmd
        },
        resolve_croc_relay,
    )
}

pub fn croc_receive(
    opts: CrocReceiveOptions,
    binary_path: &str,
) -> (mpsc::Receiver<TransferMsg>, ProcessHandle) {
    let binary = binary_path.to_owned();
    spawn_croc_transfer(
        Some(opts.code),
        false,
        move |code, relay| {
            let mut cmd = croc_transfer_command(&binary, code, relay);
            if let Some(dir) = &opts.output_dir {
                cmd.arg("--out").arg(dir);
            }
            cmd
        },
        resolve_croc_relay,
    )
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
                jobs: None,
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
                            if !sender_activity_emitted {
                                let _ = tx.send(TransferMsg::SenderTransferActivity);
                                sender_activity_emitted = true;
                            }
                        }
                        continue;
                    }
                    if count > 0 {
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
                        // Written/queued byte counts are not proof of delivery.
                        // Never turn an explicit provider failure into a peer
                        // disconnect that the UI may interpret as success.
                        let _ = tx.send(TransferMsg::Error(
                            "Sendme transfer failed on sender side".to_string(),
                        ));
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
                | Ok(NativeSendmeEvent::ReceiveRoute(_))
                | Ok(NativeSendmeEvent::ReceiveProgress { .. })
                | Ok(NativeSendmeEvent::ReceiveCompleted)
                | Ok(NativeSendmeEvent::ReceiveExportStarted)
                | Ok(NativeSendmeEvent::ReceiveExportProgress { .. }) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if opts.one_shot {
                        let _ = tx.send(TransferMsg::Error(
                            "Sender event stream ended before delivery was confirmed".to_string(),
                        ));
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

        let mut last_progress_log = Instant::now();
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
                Ok(NativeSendmeEvent::ReceiveRoute(route)) => {
                    let _ = tx.send(TransferMsg::SendmeRoute(route));
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
                    // Progress lines are UI metrics; throttle them so the log
                    // and channel are not flooded by per-chunk events.
                    if last_progress_log.elapsed() >= Duration::from_millis(200) {
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
                        last_progress_log = Instant::now();
                    }
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
            match evt {
                NativeSendmeEvent::ReceiveProgress { done, total, .. } if total > 0 => {
                    let progress = (done as f32 / total as f32).clamp(0.0, 1.0);
                    let _ = tx.send(TransferMsg::Progress(progress));
                }
                NativeSendmeEvent::ReceiveRoute(route) => {
                    let _ = tx.send(TransferMsg::SendmeRoute(route));
                }
                _ => {}
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
                let _ = tx.send(TransferMsg::Error(format!("{e:#}")));
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

    // Sender shapes include `croc <code> (code copied to clipboard)` and
    // `croc --relay <address> <code> (code copied to clipboard)`. The paren
    // requirement distinguishes the code line from subcommand echoes
    // (`croc send …`, `croc relay`, …) which never carry `(…)` directly.
    {
        let mut text = trimmed;
        // Tolerate a log-level tag such as `[Croc] ` if a caller passes one.
        if let Some(tag) = text.get(..6) {
            if tag.eq_ignore_ascii_case("[croc]") {
                text = text[6..].trim_start();
            }
        }
        let is_croc_prefix = text
            .get(..5)
            .is_some_and(|head| head.eq_ignore_ascii_case("croc "));
        if is_croc_prefix {
            let mut tokens = text[5..].split_whitespace();
            let mut token = tokens.next()?;
            while matches!(token, "--relay" | "--relay6") {
                tokens.next()?; // relay address, not the secret
                token = tokens.next()?;
            }
            let token = token.trim_matches('"');
            if let Some(tail) = tokens.next() {
                if tail.starts_with('(')
                    && !token.is_empty()
                    && !token.starts_with('-')
                    && !matches!(token, "send" | "relay" | "update" | "upgrade" | "store" | "ssh" | "help")
                    && !token.contains(['/', '\\', ':', '?', '#'])
                {
                    return Some(token.to_string());
                }
            }
        }
    }

    None
}

/// Which data path a croc transfer uses, read off croc 11.5.0's
/// `Sending (...)` / `Receiving (...)` direction line
/// (`transferDirection()` in upstream `terminal_display.go`):
/// - `Sending (->peer)` / `Receiving (<-peer)` (arrow first): relayed path.
/// - `Sending (local->peer)` / `Receiving (local<-peer)` (two addresses):
///   direct peer link (LAN or Tailcat-direct — the format deliberately does
///   not split those, so neither do we).
/// Anything else (file announcements like `Sending 'f' (1 MB)`, progress,
/// errors) yields `None`: unknown shapes stay silent, never guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrocRoute {
    Relay,
    Direct,
}

impl CrocRoute {
    pub fn label(self) -> &'static str {
        match self {
            CrocRoute::Relay => "via public relay",
            CrocRoute::Direct => "direct peer link",
        }
    }
}

pub fn parse_croc_direction(line: &str) -> Option<CrocRoute> {
    let lower = line.to_lowercase();
    let rest = lower
        .strip_prefix("sending (")
        .or_else(|| lower.strip_prefix("receiving ("))?;
    let rest = rest.trim_start();
    // Arrow first means the relay shape (`->peer` / `<-peer`).
    if rest.starts_with("->") || rest.starts_with("<-") {
        return Some(CrocRoute::Relay);
    }
    // File announcements (`Sending 'payload.bin' (1.0 MB)`) share the
    // prefix but always quote the name; direction lines never quote.
    if rest.contains('\'') {
        return None;
    }
    // Otherwise a `local->peer` / `local<-peer` pair means a direct link,
    // with a non-empty local endpoint before the arrow.
    let arrow = rest.find("->").or_else(|| rest.find("<-"))?;
    if rest[..arrow].trim().is_empty() {
        return None;
    }
    Some(CrocRoute::Direct)
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
                kill_pid(pid);
            }
        }
    }
}

/// Kill one process id best-effort (shared by cancel and instant-quit).
fn kill_pid(pid: u32) {
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

// ── Managed child-process registry ───────────────────────────────
// Long-lived croc children tracked here so tray instant-quit can kill them
// without app state (`process::exit` skips `Drop`, which is what normally
// cancels them). PIDs are removed when the child is reaped.
static MANAGED_CHILD_PIDS: std::sync::LazyLock<std::sync::Mutex<HashSet<u32>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashSet::new()));

pub(crate) fn track_child_pid(pid: u32) {
    if let Ok(mut set) = MANAGED_CHILD_PIDS.lock() {
        set.insert(pid);
    }
}

pub(crate) fn untrack_child_pid(pid: u32) {
    if let Ok(mut set) = MANAGED_CHILD_PIDS.lock() {
        set.remove(&pid);
    }
}

/// Best-effort kill of every tracked child (tray instant-quit path).
pub(crate) fn kill_all_tracked_children() {
    let pids: Vec<u32> = MANAGED_CHILD_PIDS
        .lock()
        .map(|mut set| set.drain().collect())
        .unwrap_or_default();
    for pid in pids {
        kill_pid(pid);
    }
}

#[cfg(test)]
mod tests {
    use super::cleanup_sendme_send_dirs;

    const CROC_DNS_ERROR: &str = "could not connect to 1.getcroc.com:9009: 1.getcroc.com:9009: comm.NewConnection failed: dial tcp: lookup 1.getcroc.com: i/o timeout";

    fn assert_croc_word_code(code: &str) {
        let mut remaining = code;
        for index in 0..3 {
            // "yo-yo" is one upstream word, even though it contains a hyphen.
            let word = super::CROC_CODE_WORDS.iter().find(|word| {
                remaining == **word || remaining.strip_prefix(**word).is_some_and(|tail| tail.starts_with('-'))
            }).expect("code must use the upstream croc word list");
            remaining = remaining.strip_prefix(*word).unwrap();
            if index < 2 { remaining = remaining.strip_prefix('-').expect("three words are required"); }
        }
        assert!(remaining.is_empty(), "code must contain exactly three words");
    }

    #[test]
    fn croc_random_codes_match_the_cli_word_format() {
        use sha2::Digest;
        assert_eq!(super::CROC_CODE_WORDS.len(), 1296);
        let canonical = super::CROC_CODE_WORDS.join("\n") + "\n";
        let checksum: String = super::Sha256::digest(canonical.as_bytes()).iter()
            .map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(checksum,
            "36ecca49e4fa20ca84b176c32f2e9c82f98f446585190e75f9879a95c08247bf");
        assert_croc_word_code("lip-swab-jab");
        assert_croc_word_code("yo-yo-swab-yo-yo");
        for _ in 0..100 { assert_croc_word_code(&super::random_croc_code()); }
    }

    #[test]
    fn croc_codes_match_current_web_relay_routing() {
        for (code, endpoint) in [
            ("stew-skate-blog", "4.getcroc.com:9009"),
            ("maker-decal-from", "4.getcroc.com:9009"),
            ("databeam-test-60787ae3cac17f94", "1.getcroc.com:9009"),
        ] {
            assert_eq!(super::croc_relay_for_code(code), endpoint);
        }
    }

    #[test]
    fn managed_croc_pin_cannot_be_undone_by_self_update() {
        assert_eq!(
            super::croc_update_check("unused").unwrap(),
            super::CrocUpdateCheck::Pinned {
                version: super::SUPPORTED_CROC_VERSION.to_string()
            }
        );
        assert!(super::croc_update_apply("unused")
            .unwrap_err()
            .contains("pinned"));
        assert!(super::managed_binary_path(&super::Tool::Croc)
            .to_string_lossy()
            .contains(&format!("croc-{}", super::SUPPORTED_CROC_VERSION)));
    }

    #[test]
    fn croc_dns_recovery_requires_the_failed_relay_endpoint() {
        assert_eq!(
            super::relay_dns_endpoint(CROC_DNS_ERROR).as_deref(),
            Some("1.getcroc.com:9009")
        );
        assert_eq!(
            super::relay_dns_endpoint(&CROC_DNS_ERROR.replace(":9009", ":9011")).as_deref(),
            Some("1.getcroc.com:9011")
        );
        for line in [
            "dial tcp: lookup 1.getcroc.com: i/o timeout",
            "could not connect to 2.getcroc.com:9009: lookup 1.getcroc.com: i/o timeout",
            "could not connect to custom.example:9009: lookup custom.example: i/o timeout",
            "could not connect to 1.getcroc.com:9009: connection refused",
            "decompress data: flate: corrupt input before offset 6",
        ] {
            assert_eq!(super::relay_dns_endpoint(line), None, "{line}");
        }
    }

    #[test]
    fn croc_final_compression_error_wins_over_earlier_dns_warning() {
        let mut output = super::CrocAttemptOutput::default();
        output.observe(CROC_DNS_ERROR, false);
        assert!(output.dns_relay().is_some());
        let error = "decompress data: flate: corrupt input before offset 6";
        output.observe(error, true);
        // A stderr reader can drain an old warning after the final stdout error.
        output.observe(CROC_DNS_ERROR, false);
        assert_eq!(output.error.as_deref(), Some(error));
        assert_eq!(output.dns_relay(), None);
    }

    fn fake_croc(lines: &[&str], exit_code: i32) -> std::process::Command {
        #[cfg(windows)]
        {
            let script = lines
                .iter()
                .map(|line| format!("Write-Output '{}';", line.replace('\'', "''")))
                .collect::<String>()
                + &format!("exit {exit_code}");
            let mut cmd = super::new_hidden_command("powershell.exe");
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
            cmd.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            cmd
        }
        #[cfg(not(windows))]
        {
            let script = lines
                .iter()
                .map(|line| format!("printf '%s\\n' '{}';", line.replace('\'', "'\\''")))
                .collect::<String>()
                + &format!("exit {exit_code}");
            let mut cmd = super::new_hidden_command("sh");
            cmd.args(["-c", &script]);
            cmd.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            cmd
        }
    }

    #[test]
    fn croc_received_text_preserves_large_multiline_stdout_verbatim() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("message.txt");
        let mut expected = "\n  \"Quoted text\"\r\n\nUnicode: café — नमस्ते 🙂\nerror: failed --overwrite file receiving\n".to_string();
        expected.push_str(&"long-unbroken-text".repeat(700));
        expected.push_str("\n\n");
        expected.push_str(&"Paragraph with indentation.\n  second line\n\n".repeat(1000));
        expected.push_str(&"x".repeat(1024 * 1024 - expected.len() - 8));
        expected.push_str("  END\n\n\n");
        std::fs::write(&path, &expected).unwrap();
        let (rx, handle) = super::spawn_croc_transfer(None, false, move |_, _| {
            #[cfg(windows)]
            let mut cmd = {
                let mut cmd = super::new_hidden_command("powershell.exe");
                let script = format!(
                    "[Console]::Error.WriteLine('Receiving text message (1.0 MiB)'); $payload=[IO.File]::ReadAllBytes('{}'); [Console]::OpenStandardOutput().Write($payload,0,$payload.Length); exit 0",
                    path.to_string_lossy().replace('\'', "''")
                );
                cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
                cmd
            };
            #[cfg(not(windows))]
            let mut cmd = {
                let mut cmd = super::new_hidden_command("sh");
                cmd.args(["-c", "printf 'Receiving text message (1.0 MiB)\\n' >&2; cat \"$1\"", "sh"])
                    .arg(&path);
                cmd
            };
            cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            cmd
        }, |_| None);
        let mut received = None;
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap() {
                super::TransferMsg::ReceivedText(text) => {
                    assert!(received.is_none(), "payload must arrive as one complete message");
                    received = Some(text);
                }
                super::TransferMsg::Completed => break,
                super::TransferMsg::Error(error) => panic!("payload was mistaken for a log: {error}"),
                _ => {}
            }
        }
        handle.request_cancel();
        let received = received.expect("complete text payload event");
        assert_eq!(received.len(), expected.len(), "payload byte count changed");
        assert!(received == expected, "payload bytes changed");
    }

    fn croc_terminal_message(
        rx: std::sync::mpsc::Receiver<super::TransferMsg>,
    ) -> super::TransferMsg {
        loop {
            let msg = rx
                .recv_timeout(std::time::Duration::from_secs(15))
                .expect("croc terminated");
            if matches!(
                msg,
                super::TransferMsg::Completed | super::TransferMsg::Error(_)
            ) {
                return msg;
            }
        }
    }

    #[test]
    fn croc_sender_announces_its_code_without_cli_code_output() {
        for sender in [true, false] {
            let code = "lip-swab-jab";
            let (rx, _) = super::spawn_croc_transfer(
                Some(code.to_string()),
                sender,
                |_, _| fake_croc(&[], 0),
                |_| panic!("successful child must not retry"),
            );
            let next = || rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap();
            assert!(matches!(next(), super::TransferMsg::Started));
            if sender {
                assert!(matches!(next(), super::TransferMsg::Code(actual) if actual == code));
            }
            assert!(matches!(next(), super::TransferMsg::Completed));
        }
    }

    #[test]
    fn croc_dns_recovery_preserves_generated_code_and_only_retries_once() {
        use std::sync::{Arc, Mutex};
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let (rx, handle) = super::spawn_croc_transfer(
            None,
            true,
            move |code, relay| {
                let mut calls = recorded.lock().unwrap();
                calls.push((code.map(str::to_owned), relay.map(str::to_owned)));
                if calls.len() == 1 {
                    fake_croc(
                        &[
                            "croc birch-coral-otter (code copied to clipboard)",
                            CROC_DNS_ERROR,
                        ],
                        1,
                    )
                } else {
                    fake_croc(&[CROC_DNS_ERROR], 1)
                }
            },
            |endpoint| {
                assert_eq!(endpoint, "1.getcroc.com:9009");
                Some("127.0.0.2:9009".to_string())
            },
        );
        assert!(
            matches!(croc_terminal_message(rx), super::TransferMsg::Error(error) if error == CROC_DNS_ERROR)
        );
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                (None, None),
                (
                    Some("birch-coral-otter".to_string()),
                    Some("127.0.0.2:9009".to_string())
                ),
            ]
        );
        assert!(handle.child_pid.lock().unwrap().is_none());
    }

    #[test]
    fn croc_probe_failure_can_recover_then_use_the_same_code_for_dns_recovery() {
        use std::sync::{Arc, Mutex};
        let codes = Arc::new(Mutex::new(Vec::new()));
        let recorded = codes.clone();
        let (rx, _) = super::spawn_croc_transfer(
            None,
            true,
            move |code, relay| {
                let mut codes = recorded.lock().unwrap();
                codes.push(code.map(str::to_owned));
                match codes.len() {
                    1 => fake_croc(&["no public relay available"], 1),
                    2 => fake_croc(&[CROC_DNS_ERROR], 1),
                    3 => {
                        assert_eq!(relay, Some("127.0.0.2:9009"));
                        fake_croc(&["sent"], 0)
                    }
                    _ => panic!("unbounded retry"),
                }
            },
            |_| Some("127.0.0.2:9009".to_string()),
        );
        assert!(matches!(
            croc_terminal_message(rx),
            super::TransferMsg::Completed
        ));
        let codes = codes.lock().unwrap();
        assert_eq!(codes[0], None);
        assert_croc_word_code(codes[1].as_deref().unwrap());
        assert_eq!(codes[1], codes[2]);
    }

    #[test]
    fn croc_never_replays_compression_errors_or_a_started_payload() {
        for lines in [
            vec!["decompress data: flate: corrupt input before offset 6"],
            vec!["Receiving (<-142.132.189.179)", CROC_DNS_ERROR],
        ] {
            let (rx, _) = super::spawn_croc_transfer(
                Some("test-secret".to_string()),
                false,
                move |_, relay| {
                    assert!(relay.is_none());
                    fake_croc(&lines, 1)
                },
                |_| panic!("must not retry after payload begins or for compression errors"),
            );
            assert!(matches!(
                croc_terminal_message(rx),
                super::TransferMsg::Error(_)
            ));
        }
    }

    #[test]
    #[ignore = "requires DATABEAM_CROC_TEST_BINARY; uses the configured croc relay"]
    fn croc_real_text_and_file_round_trip() {
        use super::{croc_receive, CrocReceiveOptions, CrocSendOptions, TransferMsg};
        use std::time::{Duration, Instant};
        let binary = std::env::var("DATABEAM_CROC_TEST_BINARY").expect("explicit test binary");
        let require_direct = std::env::var_os("DATABEAM_CROC_TEST_REQUIRE_DIRECT").is_some();
        let temp = tempfile::tempdir().unwrap();
        let text = "DataBeam live regression test — hello from croc";
        let random: Vec<u8> = (0..262144).map(|_| rand::random()).collect();
        let repeated = vec![b'x'; 262144];
        for (text_mode, receiver_first) in [(true, false), (true, true), (false, false), (false, true)] {
            let output = temp.path().join(format!("{}-{receiver_first}", if text_mode { "text" } else { "files" }));
            std::fs::create_dir(&output).unwrap();
            let source_a = temp.path().join("random.bin");
            let source_b = temp.path().join("compressed.txt");
            std::fs::write(&source_a, &random).unwrap();
            std::fs::write(&source_b, &repeated).unwrap();
            let code = super::random_croc_code();
            let opts = CrocSendOptions {
                paths: if text_mode {
                    vec![]
                } else {
                    vec![source_a, source_b]
                },
                custom_code: Some(code.clone()),
                text_mode,
                text_value: text_mode.then(|| text.to_string()),
            };
            let start_receiver = || croc_receive(
                CrocReceiveOptions { code: code.clone(), output_dir: Some(output.clone()) }, &binary,
            );
            let waiting_receiver = if receiver_first {
                let receiver = start_receiver();
                // Leave time for the relay to deliver keepalives before the
                // sender joins: the timing missed by sender-first smoke tests.
                std::thread::sleep(Duration::from_secs(3));
                Some(receiver)
            } else { None };
            // Normally force relay coverage. Direct mode uses the production
            // sender unchanged and must prove a direct route on both peers.
            let sender_binary = binary.clone();
            let (sender, send_handle) = if require_direct {
                super::croc_send(opts, &binary)
            } else {
                super::spawn_croc_transfer(
                    Some(code.clone()),
                    true,
                    move |code, relay| {
                        let mut cmd = super::croc_transfer_command(&sender_binary, code, relay);
                        // Croc 11.5.5 can also select Tailcat in auto mode;
                        // explicitly select relay for this relay-only trial.
                        cmd.args(["send", "--transport", "relay", "--no-local"]);
                        if let Some(text) = &opts.text_value {
                            cmd.arg("--text").arg(text);
                        } else {
                            cmd.arg("--").args(&opts.paths);
                        }
                        cmd
                    },
                    super::resolve_croc_relay,
                )
            };
            let (receiver, receive_handle) = waiting_receiver.unwrap_or_else(start_receiver);
            let deadline = Instant::now() + Duration::from_secs(90);
            let mut sent = false;
            let mut received = false;
            let mut received_text = false;
            let mut announced_code = false;
            let mut routes = [None; 2];
            let mut error = None;
            while Instant::now() < deadline && error.is_none() && !(sent && received) {
                for (is_receiver, rx) in [(false, &sender), (true, &receiver)] {
                    while let Ok(msg) = rx.try_recv() {
                        match msg {
                            TransferMsg::Code(value) if !is_receiver => {
                                assert_eq!(value, code);
                                assert!(!sent, "code must reach the UI before completion");
                                announced_code = true;
                            }
                            TransferMsg::Completed if is_receiver => received = true,
                            TransferMsg::Completed => sent = true,
                            TransferMsg::Error(message) => error = Some(message),
                            TransferMsg::ReceivedText(text_value) if is_receiver => {
                                received_text = text_value == text;
                            }
                            TransferMsg::Output(line) => {
                                if let Some(route) = super::parse_croc_direction(&line) {
                                    routes[is_receiver as usize] = Some(route);
                                }
                                eprintln!(
                                    "{}: {line}",
                                    if is_receiver { "receiver" } else { "sender" }
                                );
                                if is_receiver && line.contains(text) {
                                    received_text = true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            send_handle.request_cancel();
            receive_handle.request_cancel();
            assert!(error.is_none(), "{error:?}");
            assert!(sent && received, "transfer timed out");
            assert!(announced_code, "sender did not publish the share code");
            let expected_route = if require_direct {
                super::CrocRoute::Direct
            } else {
                super::CrocRoute::Relay
            };
            assert_eq!(routes, [Some(expected_route); 2], "both peers must report the actual route");
            if text_mode {
                assert!(received_text, "text bytes did not arrive");
            } else {
                assert_eq!(std::fs::read(output.join("random.bin")).unwrap(), random);
                assert_eq!(
                    std::fs::read(output.join("compressed.txt")).unwrap(),
                    repeated
                );
            }
        }
    }

    #[test]
    #[ignore = "requires DATABEAM_CROC_TEST_BINARY and network access for real EasySendMe handoff"]
    fn eazysendme_real_ticket_and_payload_round_trip() {
        use super::*;

        struct CancelOnDrop(ProcessHandle);
        impl Drop for CancelOnDrop {
            fn drop(&mut self) { self.0.request_cancel(); }
        }

        let binary = std::env::var("DATABEAM_CROC_TEST_BINARY").expect("explicit test binary");
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("hybrid.bin");
        let bytes: Vec<u8> = (0..262144).map(|_| rand::random()).collect();
        fs::write(&source, &bytes).unwrap();
        let (share_rx, share_handle) = sendme_send(SendmeSendOptions {
            paths: vec![source],
            one_shot: true,
            blob_dir: Some(temp.path().join("sender-cache")),
        }, "");
        let _share_guard = CancelOnDrop(share_handle);
        let ticket = loop {
            match share_rx.recv_timeout(Duration::from_secs(45)).expect("native share startup") {
                TransferMsg::Code(ticket) => break ticket,
                TransferMsg::Error(error) => panic!("native share failed: {error}"),
                _ => {}
            }
        };

        // Exercise the same receiver-first Croc text handoff used by the GUI.
        let code = random_croc_code();
        let handoff_dir = temp.path().join("handoff");
        fs::create_dir(&handoff_dir).unwrap();
        let (receive_rx, receive_handle) = croc_receive(CrocReceiveOptions {
            code: code.clone(), output_dir: Some(handoff_dir),
        }, &binary);
        let _receive_guard = CancelOnDrop(receive_handle);
        thread::sleep(Duration::from_secs(3));
        let (send_rx, send_handle) = croc_send(CrocSendOptions {
            paths: vec![], custom_code: Some(code), text_mode: true,
            text_value: Some(ticket.clone()),
        }, &binary);
        let _send_guard = CancelOnDrop(send_handle);
        let mut received_ticket = None;
        let (mut sent, mut received) = (false, false);
        let deadline = Instant::now() + Duration::from_secs(90);
        while Instant::now() < deadline && !(sent && received) {
            for (receiver, rx) in [(false, &send_rx), (true, &receive_rx)] {
                while let Ok(message) = rx.try_recv() {
                    match message {
                        TransferMsg::Completed if receiver => received = true,
                        TransferMsg::Completed => sent = true,
                        TransferMsg::Error(error) => panic!("Croc ticket handoff failed: {error}"),
                        TransferMsg::ReceivedText(value) if receiver => {
                            received_ticket = Some(value);
                        }
                        TransferMsg::Output(line) if receiver => {
                            if let Some(value) = crate::normalize_sendme_ticket(&line) {
                                received_ticket = Some(value);
                            }
                        }
                        _ => {}
                    }
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(sent && received, "Croc ticket handoff timed out");
        assert_eq!(received_ticket.as_deref(), Some(ticket.as_str()));

        let output = temp.path().join("received");
        let (payload_rx, payload_handle) = sendme_receive(SendmeReceiveOptions {
            ticket: received_ticket.unwrap(), output_dir: Some(output.clone()),
            blob_dir: Some(temp.path().join("receiver-cache")), overwrite: false,
        }, "");
        let _payload_guard = CancelOnDrop(payload_handle);
        let deadline = Instant::now() + Duration::from_secs(90);
        let mut completed = false;
        while Instant::now() < deadline && !completed {
            match payload_rx.recv_timeout(Duration::from_secs(1)) {
                Ok(TransferMsg::Completed) => completed = true,
                Ok(TransferMsg::Error(error)) => panic!("native receive failed: {error}"),
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        assert!(completed, "native payload timed out");
        assert_eq!(fs::read(output.join("hybrid.bin_databeam/hybrid.bin")).unwrap(), bytes);
    }

    #[test]
    #[ignore = "manual croc web interop: DATABEAM_CROC_TEST_BINARY, DATABEAM_CROC_WEB_CODE, DATABEAM_CROC_WEB_MODE"]
    fn croc_web_text_interop() {
        let binary = std::env::var("DATABEAM_CROC_TEST_BINARY").unwrap();
        let code = std::env::var("DATABEAM_CROC_WEB_CODE").unwrap();
        let mode = std::env::var("DATABEAM_CROC_WEB_MODE").unwrap();
        let file = std::env::var_os("DATABEAM_CROC_WEB_FILE").map(std::path::PathBuf::from);
        let text = "DataBeam croc regression test — text 2026-10-23";
        let output = tempfile::tempdir().unwrap();
        let (rx, handle) = if mode == "receive" {
            super::croc_receive(
                super::CrocReceiveOptions {
                    code,
                    output_dir: Some(output.path().to_owned()),
                },
                &binary,
            )
        } else {
            assert_eq!(mode, "send");
            super::croc_send(
                super::CrocSendOptions {
                    paths: file.clone().into_iter().collect(),
                    custom_code: Some(code),
                    text_mode: file.is_none(),
                    text_value: file.is_none().then(|| text.to_owned()),
                },
                &binary,
            )
        };
        let mut received = false;
        let result = loop {
            match rx.recv_timeout(std::time::Duration::from_secs(60)) {
                Ok(super::TransferMsg::Output(line)) => {
                    received |= line.contains(text);
                    eprintln!("{line}");
                }
                Ok(super::TransferMsg::ReceivedText(value)) => {
                    received = value == text;
                }
                Ok(super::TransferMsg::Completed) => break Ok(()),
                Ok(super::TransferMsg::Error(error)) => break Err(error),
                Ok(_) => {}
                Err(error) => break Err(error.to_string()),
            }
        };
        handle.request_cancel();
        result.unwrap();
        if mode == "receive" {
            if let Some(source) = file {
                assert_eq!(std::fs::read(output.path().join(source.file_name().unwrap())).unwrap(),
                    std::fs::read(source).unwrap(), "web file bytes did not match");
            } else {
                assert!(received, "text did not arrive intact");
            }
        }
    }

    #[test]
    fn croc_bundled_engine_extracts_and_reports_patched_version() {
        if super::CROC_GZ.is_empty() { return; } // unbundled development builds
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join(super::managed_binary_name(&super::Tool::Croc));
        super::restore_bundled_croc_to(&binary, super::CROC_GZ, super::CROC_BINARY_SHA256).unwrap();
        assert!(super::verify_croc_version(&super::croc_version_string(&binary.to_string_lossy()).unwrap(),
            super::SUPPORTED_CROC_VERSION).is_ok());
        assert!(super::verify_croc_version("croc version 11.2.5", super::SUPPORTED_CROC_VERSION).is_err());
        assert!(super::verify_croc_version("croc version 11.5.5", super::SUPPORTED_CROC_VERSION).is_err());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn croc_bundle_repairs_bad_cache_and_preserves_it_on_invalid_payload() {
        use std::io::Write;
        use sha2::{Digest, Sha256};
        let bytes = b"patched Croc fixture";
        let hash: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        let gzip = encoder.finish().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let binary = temp.path().join("croc");
        std::fs::write(&binary, b"old unpatched copy").unwrap();
        super::restore_bundled_croc_to(&binary, &gzip, &hash).unwrap();
        assert_eq!(std::fs::read(&binary).unwrap(), bytes);
        assert!(super::restore_bundled_croc_to(&binary, &gzip, "wrong hash").is_err());
        assert_eq!(std::fs::read(&binary).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
        assert!(super::restore_bundled_croc_to(&binary, &[], &hash).is_err());
    }
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

    #[test]
    fn managed_croc_path_roundtrip() {
        use super::{is_managed_croc_binary, managed_binary_path, Tool};

        // Whatever the platform cache dir is, the managed path must
        // recognize itself and reject foreign locations.
        let managed = managed_binary_path(&Tool::Croc)
            .to_string_lossy()
            .to_string();
        assert!(is_managed_croc_binary(&managed));
        assert!(!is_managed_croc_binary("/usr/local/bin/croc"));
        assert!(!is_managed_croc_binary(""));
    }

    #[test]
    fn croc_update_check_output_parses() {
        use super::{parse_croc_update_check_output, CrocUpdateCheck};
        assert!(matches!(
            parse_croc_update_check_output("croc v11.5.0 is up to date.\n"),
            CrocUpdateCheck::UpToDate { .. }
        ));
        match parse_croc_update_check_output("croc v11.5.0 is available (current: v11.2.5).\n") {
            CrocUpdateCheck::Available { current, latest, .. } => {
                assert_eq!(current, "v11.2.5");
                assert_eq!(latest, "v11.5.0");
            }
            other => panic!("expected Available, got {other:?}"),
        }
        // Legacy binary without the update subcommand surfaces a receive-code
        // prompt instead of update output.
        assert!(matches!(
            parse_croc_update_check_output("Enter receive code: could not read receive code: EOF"),
            CrocUpdateCheck::Unsupported { .. }
        ));
        // Package-managed installs print guidance instead of self-updating.
        assert!(matches!(
            parse_croc_update_check_output(
                "croc v11.5.0 is available (current: v11.2.5).\nUpgrade with Scoop: scoop update croc"
            ),
            CrocUpdateCheck::NotWritable { .. }
        ));
        // Unrecognized output stays Unknown (retried later, never acted on).
        assert!(matches!(
            parse_croc_update_check_output("something unexpected"),
            CrocUpdateCheck::Unknown { .. }
        ));
    }

    #[test]
    fn croc_update_apply_output_parses() {
        use super::parse_croc_update_apply_output;

        let ok = parse_croc_update_apply_output(
            "croc v11.5.0 is available (current: v11.2.5).\nUpdated croc from v11.2.5 to v11.5.0.\n",
        );
        assert!(ok.updated);
        assert_eq!(ok.new_version.as_deref(), Some("v11.5.0"));

        let cancelled =
            parse_croc_update_apply_output("croc v11.5.0 is available (current: v11.2.5).\nUpdate cancelled.\n");
        assert!(!cancelled.updated);

        let up_to_date = parse_croc_update_apply_output("croc v11.5.0 is up to date.\n");
        assert!(!up_to_date.updated);
        assert!(up_to_date.already_up_to_date);

        // Live Windows refusal: self-update can never apply in place.
        let refused = parse_croc_update_apply_output(
            "croc v11.5.1 is available (current: v11.5.0).\nThis platform cannot safely replace the running executable in place.\n",
        );
        assert!(!refused.updated);
        assert!(!refused.already_up_to_date);
        assert!(refused.needs_redownload);

        // Unix refusal without official-installer registration.
        let unregistered = parse_croc_update_apply_output(
            "This installation is not registered as an official standalone install.\n",
        );
        assert!(!unregistered.updated);
        assert!(unregistered.needs_redownload);
    }

    #[test]
    fn standalone_refusal_with_installer_guidance_reaches_download_fallback() {
        for reason in [
            "This installation is not registered as an official standalone install.",
            "The official installer registration does not match the running executable.",
            "This platform cannot safely replace the running executable in place.",
        ] {
            let output = format!("croc v11.5.1 is available (current: v11.5.0).\n{reason}\nUpgrade with the official installer: curl https://getcroc.com | bash\n");
            let outcome = super::parse_croc_update_apply_output(&output);
            assert!(outcome.needs_redownload, "{output}");
            assert!(!outcome.not_writable, "{output}");
        }
        let package = super::parse_croc_update_apply_output(
            "The running croc command resolves through a symlink and will not be overwritten.\nUpgrade with Homebrew: brew upgrade croc"
        );
        assert!(package.not_writable);
        assert!(!package.needs_redownload);
    }

    #[test]
    fn unchanged_installed_version_cannot_report_update_success() {
        assert!(super::verify_croc_version("croc version 11.5.0", "v11.5.1").is_err());
        assert!(super::verify_croc_version("croc version 11.5.1\n", "v11.5.1").is_ok());
        assert!(super::verify_croc_version("", "v11.5.1").is_err());
    }

    #[test]
    fn failed_managed_extraction_preserves_working_binary() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join(super::managed_binary_name(&super::Tool::Croc));
        std::fs::write(&binary, b"working binary").unwrap();
        assert!(super::stage_managed_binary(
            &super::Tool::Croc, "croc.zip", b"invalid archive", "v11.5.1", &binary
        ).is_err());
        assert_eq!(std::fs::read(&binary).unwrap(), b"working binary");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn staged_archive_replaces_binary_after_extraction() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let name = super::managed_binary_name(&super::Tool::Sendme);
        let binary = dir.path().join(&name);
        std::fs::write(&binary, b"previous binary").unwrap();
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive.start_file(&name, zip::write::FileOptions::default()).unwrap();
        archive.write_all(b"replacement binary").unwrap();
        let archive = archive.finish().unwrap().into_inner();
        super::stage_managed_binary(&super::Tool::Sendme, "sendme.zip", &archive, "v1.0.0", &binary).unwrap();
        assert_eq!(std::fs::read(&binary).unwrap(), b"replacement binary");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn croc_command_surfaces_nonzero_exit_status() {
        let error = super::run_croc_command("cmd.exe", &["/D", "/C", "echo network unavailable & exit /B 7"], None).unwrap_err();
        assert!(error.contains("network unavailable"), "{error}");
        assert!(error.contains("7"), "{error}");
    }

    #[test]
    fn test_extract_croc_code_sender_shapes() {
        use super::extract_croc_code;

        // croc 11.5.0 generated/custom sender line (Eazy + Croc modes).
        assert_eq!(
            extract_croc_code("croc asdfasdf (code copied to clipboard)"),
            Some("asdfasdf".to_string())
        );
        assert_eq!(
            extract_croc_code("croc coach-wasp-quill (code copied to clipboard)"),
            Some("coach-wasp-quill".to_string())
        );
        for line in [
            "croc --relay 142.132.189.179:9009 lip-swab-jab (code copied to clipboard)",
            "croc --relay relay.example:9009 lip-swab-jab (code copied to clipboard)",
            "[Croc] croc --relay6 [::1]:9009 lip-swab-jab (code copied to clipboard)",
            "croc --relay 127.0.0.1:9009 --relay6 [::1]:9009 lip-swab-jab (code copied to clipboard)",
        ] {
            assert_eq!(extract_croc_code(line).as_deref(), Some("lip-swab-jab"), "{line}");
        }
        // Tolerate a log-level tag prefix.
        assert_eq!(
            extract_croc_code("[Croc] croc asdfasdf (code copied to clipboard)"),
            Some("asdfasdf".to_string())
        );
        // Legacy shapes keep working.
        assert_eq!(
            extract_croc_code("Code is: ocean-monkey-42"),
            Some("ocean-monkey-42".to_string())
        );
        assert_eq!(
            extract_croc_code("CROC_SECRET=\"asdfasdf\" croc"),
            Some("asdfasdf".to_string())
        );
        // Must not fire on subcommand echoes, progress, or links.
        assert_eq!(extract_croc_code("croc send file.txt"), None);
        assert_eq!(extract_croc_code("croc relay"), None);
        assert_eq!(extract_croc_code("croc --relay"), None);
        assert_eq!(extract_croc_code("croc --relay relay.example:9009"), None);
        assert_eq!(extract_croc_code("croc --relay relay.example:9009 send file.txt (example)"), None);
        assert_eq!(extract_croc_code("Sending (->192.168.1.60:56052)"), None);
        assert_eq!(
            extract_croc_code("Or open: https://getcroc.com/?code=asdfasdf"),
            None
        );
        assert_eq!(extract_croc_code("Sending 'text' (222 B)"), None);
        assert_eq!(extract_croc_code(""), None);
    }

    #[test]
    fn test_parse_croc_direction_shapes() {
        use super::{parse_croc_direction, CrocRoute};

        // Relay shape: arrow first (upstream `!peerToPeer || local == ""`).
        assert_eq!(
            parse_croc_direction("Sending (->203.0.113.7)"),
            Some(CrocRoute::Relay)
        );
        assert_eq!(
            parse_croc_direction("Receiving (<-203.0.113.7)"),
            Some(CrocRoute::Relay)
        );
        // Direct shape: local->peer pair (LAN or Tailcat-direct — the
        // format does not split those, so neither do we).
        assert_eq!(
            parse_croc_direction("Sending (10.0.0.2->169.254.83.107%Tailscale)"),
            Some(CrocRoute::Direct)
        );
        assert_eq!(
            parse_croc_direction("Receiving (10.0.0.2<-169.254.83.107)"),
            Some(CrocRoute::Direct)
        );
        // Case-insensitive, like the rest of the log parsing.
        assert_eq!(
            parse_croc_direction("SENDING (10.0.0.2->169.254.83.107)"),
            Some(CrocRoute::Direct)
        );
        // File announcements share the prefix but quote the name.
        assert_eq!(parse_croc_direction("Sending 'payload.bin' (1.0 MB)"), None);
        assert_eq!(parse_croc_direction("Sending 'text' (222 B)"), None);
        // Inventory lines, progress, errors, and blanks stay silent.
        assert_eq!(parse_croc_direction("Sending 0 files (1.0 MB)"), None);
        assert_eq!(
            parse_croc_direction("Sending (->192.168.1.60:56052)"),
            Some(CrocRoute::Relay)
        );
        assert_eq!(parse_croc_direction("relay connection failed: nope"), None);
        assert_eq!(parse_croc_direction(""), None);
    }
}
