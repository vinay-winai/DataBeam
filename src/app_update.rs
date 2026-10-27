mod install;

use eframe::egui;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const API_URL: &str = "https://api.github.com/repos/vinay-winai/DataBeam/releases/latest";
const DOWNLOAD_ROOT: &str = "https://github.com/vinay-winai/DataBeam/releases/download/";
const CHECK_INTERVAL: Duration = Duration::from_secs(3 * 60 * 60);
const MAX_PACKAGE_SIZE: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
enum PackageKind {
    WindowsExe,
    MacDmg,
    LinuxAppImage,
    LinuxDeb,
}

#[derive(Clone, Debug)]
pub struct AvailableUpdate {
    pub version: String,
    package: Asset,
    checksums: Asset,
    kind: PackageKind,
}

fn select_update(
    release: Release,
    current: &str,
    os: &str,
    arch: &str,
    appimage: bool,
) -> Result<Option<AvailableUpdate>, String> {
    let version = Version::parse(release.tag_name.trim_start_matches('v'))
        .map_err(|error| format!("Invalid release version: {error}"))?;
    let current = Version::parse(current).map_err(|error| error.to_string())?;
    if release.draft || release.prerelease || !version.pre.is_empty() || version <= current {
        return Ok(None);
    }
    let (suffix, kind) = match (os, arch, appimage) {
        ("windows", "x86_64" | "aarch64", _) => ("windows-x64.exe", PackageKind::WindowsExe),
        ("macos", "aarch64", _) => ("macos-aarch64.dmg", PackageKind::MacDmg),
        ("macos", "x86_64", _) => ("macos-universal.dmg", PackageKind::MacDmg),
        ("linux", "x86_64", true) => ("linux-x64.AppImage", PackageKind::LinuxAppImage),
        ("linux", "x86_64", false) => ("linux-x64.deb", PackageKind::LinuxDeb),
        _ => return Err("Automatic updates are unavailable for this platform.".into()),
    };
    let name = format!("databeam_{}_{suffix}", release.tag_name);
    let find = |name: &str| -> Result<Asset, String> {
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or_else(|| format!("Release is missing {name}"))?
            .clone();
        let expected_url = format!("{DOWNLOAD_ROOT}{}/{name}", release.tag_name);
        if asset.browser_download_url != expected_url
            || asset.size == 0
            || asset.size > MAX_PACKAGE_SIZE
        {
            return Err(format!("Invalid release asset: {name}"));
        }
        Ok(asset)
    };
    Ok(Some(AvailableUpdate {
        version: version.to_string(),
        package: find(&name)?,
        checksums: find("SHA256SUMS.txt")?,
        kind,
    }))
}

fn curl(url: &str, max_bytes: u64, timeout_secs: u64) -> std::process::Command {
    let mut command = crate::backend::new_hidden_command("curl");
    command.args([
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--connect-timeout",
        "10",
        "--user-agent",
        "DataBeam-updater",
        "--header",
        "Accept: application/vnd.github+json",
        "--max-filesize",
        &max_bytes.to_string(),
        "--max-time",
        &timeout_secs.to_string(),
        url,
    ]);
    command
}

fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let output = curl(url, 1024 * 1024, 20)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Update check failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

fn fetch_update() -> Result<Option<AvailableUpdate>, String> {
    let release =
        serde_json::from_slice(&fetch_bytes(API_URL)?).map_err(|error| error.to_string())?;
    select_update(
        release,
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::var_os("APPIMAGE").is_some(),
    )
}

fn file_digest(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn expected_digest(manifest: &[u8], asset: &Asset) -> Result<String, String> {
    let manifest = std::str::from_utf8(manifest).map_err(|error| error.to_string())?;
    let mut matches = manifest.lines().filter_map(|line| {
        let (digest, name) = line.split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == asset.name).then_some(digest)
    });
    let digest = matches.next().ok_or("Package checksum is missing")?;
    if matches.next().is_some()
        || digest.len() != 64
        || !digest.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid package checksum manifest".into());
    }
    if let Some(api_digest) = &asset.digest {
        if !api_digest.eq_ignore_ascii_case(&format!("sha256:{digest}")) {
            return Err("Release checksum disagrees with GitHub's asset digest".into());
        }
    }
    Ok(digest.to_ascii_lowercase())
}

fn download_verified(update: &AvailableUpdate, directory: &Path) -> Result<PathBuf, String> {
    let digest = expected_digest(
        &fetch_bytes(&update.checksums.browser_download_url)?,
        &update.package,
    )?;
    let path = directory.join(&update.package.name);
    let output = curl(
        &update.package.browser_download_url,
        update.package.size,
        600,
    )
    .arg("--output")
    .arg(&path)
    .output()
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Update download failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if fs::metadata(&path)
        .map_err(|error| error.to_string())?
        .len()
        != update.package.size
        || file_digest(&path)? != digest
    {
        return Err(
            "Downloaded update failed SHA-256 verification; the app was not changed.".into(),
        );
    }
    Ok(path)
}

enum Event {
    Checked(Result<Option<AvailableUpdate>, String>),
    Prepared(Result<(), String>),
}

#[derive(Default)]
pub struct AppUpdater {
    pub available: Option<AvailableUpdate>,
    pub status: Option<String>,
    pub installing: bool,
    pending: Option<mpsc::Receiver<Event>>,
    next_check: Option<Instant>,
}

impl AppUpdater {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn check(&mut self, ctx: &egui::Context) {
        if self.busy() || self.installing {
            return;
        }
        self.status = Some("Checking for updates…".into());
        self.next_check = Some(Instant::now() + CHECK_INTERVAL);
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Checked(fetch_update()));
            wake.request_repaint();
        });
    }

    pub fn install(&mut self, ctx: &egui::Context) {
        if self.busy() || self.installing {
            return;
        }
        let Some(update) = self.available.clone() else {
            self.check(ctx);
            return;
        };
        self.installing = true;
        self.status = Some(format!("Downloading and verifying v{}…", update.version));
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let result = install::prepare(&update).and_then(|prepared| prepared.launch());
            let _ = tx.send(Event::Prepared(result));
            wake.request_repaint();
        });
    }

    /// Returns true when a replacement helper is ready and the app should exit.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        if let Some(rx) = self.pending.take() {
            match rx.try_recv() {
                Ok(Event::Checked(Ok(update))) => {
                    self.status = Some(match &update {
                        Some(update) => format!(
                            "v{} is available. Click Update to install and restart.",
                            update.version
                        ),
                        None => "DataBeam is up to date.".into(),
                    });
                    self.available = update;
                }
                Ok(Event::Checked(Err(error))) => self.status = Some(error),
                Ok(Event::Prepared(Ok(()))) => return true,
                Ok(Event::Prepared(Err(error))) => {
                    self.status = Some(format!("Update failed: {error}"));
                    self.installing = false;
                }
                Err(mpsc::TryRecvError::Empty) => self.pending = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.status = Some("Update worker stopped; click Update to retry.".into());
                    self.installing = false;
                }
            }
        }
        if !self.busy() && !self.installing {
            let delay = self
                .next_check
                .map(|next| next.saturating_duration_since(Instant::now()))
                .unwrap_or_default();
            if delay.is_zero() {
                self.check(ctx);
            } else {
                ctx.request_repaint_after(delay);
            }
        }
        false
    }
}

/// The helper runs before UI and single-instance setup, from its own copy.
pub fn run_helper_if_requested() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    if args
        .get(1)
        .is_some_and(|arg| arg == "--databeam-apply-update")
    {
        if let Some(config) = args.get(2) {
            if let Err(error) = install::apply(Path::new(config)) {
                eprintln!("DataBeam update: {error}");
            }
        }
        return true;
    }
    false
}

pub fn restart_result() -> Option<String> {
    let args: Vec<_> = std::env::args_os().collect();
    if args
        .get(1)
        .is_some_and(|arg| arg == "--databeam-update-result")
    {
        args.get(2)
            .and_then(|path| install::acknowledge_restart(Path::new(path)).ok())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
            draft: false,
            prerelease: false,
            assets: [
                "windows-x64.exe",
                "windows-x64.zip",
                "macos-aarch64.dmg",
                "macos-universal.dmg",
                "linux-x64.AppImage",
                "linux-x64.deb",
            ]
            .into_iter()
            .map(|suffix| format!("databeam_{tag}_{suffix}"))
            .chain(std::iter::once("SHA256SUMS.txt".into()))
            .map(|name| Asset {
                browser_download_url: format!("{DOWNLOAD_ROOT}{tag}/{name}"),
                name,
                size: 1024,
                digest: None,
            })
            .collect(),
        }
    }

    #[test]
    fn stable_versions_compare_numerically_and_never_downgrade() {
        assert!(
            select_update(release("v0.6.10"), "0.6.9", "windows", "x86_64", false)
                .unwrap()
                .is_some()
        );
        for tag in ["v0.6.6", "v0.6.5", "v0.7.0-rc.1"] {
            assert!(
                select_update(release(tag), "0.6.6", "windows", "x86_64", false)
                    .unwrap()
                    .is_none()
            );
        }
        let mut preview = release("v0.7.0");
        preview.prerelease = true;
        assert!(select_update(preview, "0.6.6", "windows", "x86_64", false)
            .unwrap()
            .is_none());
    }

    #[test]
    fn correct_assets_are_selected_on_all_shipping_platforms() {
        for (os, arch, appimage, suffix) in [
            ("windows", "x86_64", false, "windows-x64.exe"),
            ("windows", "aarch64", false, "windows-x64.exe"),
            ("macos", "aarch64", false, "macos-aarch64.dmg"),
            ("macos", "x86_64", false, "macos-universal.dmg"),
            ("linux", "x86_64", true, "linux-x64.AppImage"),
            ("linux", "x86_64", false, "linux-x64.deb"),
        ] {
            assert!(
                select_update(release("v0.7.0"), "0.6.6", os, arch, appimage)
                    .unwrap()
                    .unwrap()
                    .package
                    .name
                    .ends_with(suffix)
            );
        }
    }

    #[test]
    fn untrusted_asset_urls_and_incomplete_releases_are_rejected() {
        let mut bad = release("v0.7.0");
        bad.assets[0].browser_download_url = "https://example.com/payload.exe".into();
        assert!(select_update(bad, "0.6.6", "windows", "x86_64", false).is_err());
        let mut bad = release("v0.7.0");
        bad.assets.retain(|asset| asset.name != "SHA256SUMS.txt");
        assert!(select_update(bad, "0.6.6", "windows", "x86_64", false).is_err());
    }

    #[test]
    fn checksum_requires_exact_filename_and_matches_github_digest() {
        let mut asset = release("v0.7.0").assets.remove(0);
        let hash = "ab".repeat(32);
        let manifest = format!("{hash}  {}\n", asset.name);
        asset.digest = Some(format!("sha256:{hash}"));
        assert_eq!(expected_digest(manifest.as_bytes(), &asset).unwrap(), hash);
        assert!(expected_digest(format!("{manifest}{manifest}").as_bytes(), &asset).is_err());
        asset.digest = Some(format!("sha256:{}", "cd".repeat(32)));
        assert!(expected_digest(manifest.as_bytes(), &asset).is_err());
        assert!(expected_digest(format!("{hash}  wrong.exe\n").as_bytes(), &asset).is_err());
    }

    #[test]
    fn offline_checks_keep_an_available_update_and_do_not_spin() {
        let mut updater = AppUpdater::default();
        updater.available =
            select_update(release("v0.7.0"), "0.6.6", "windows", "x86_64", false).unwrap();
        let (tx, rx) = mpsc::channel();
        updater.pending = Some(rx);
        updater.next_check = Some(Instant::now() + CHECK_INTERVAL);
        tx.send(Event::Checked(Err("offline".into()))).unwrap();
        assert!(!updater.poll(&egui::Context::default()));
        assert!(updater.available.is_some());
        assert!(!updater.busy());
        assert_eq!(CHECK_INTERVAL, Duration::from_secs(10800));
    }

    #[test]
    #[ignore = "downloads a public release package; verifies without installing"]
    fn public_release_download_passes_checksum_verification() {
        let release: Release = serde_json::from_slice(&fetch_bytes(API_URL).unwrap()).unwrap();
        let update = select_update(release, "0.0.0", "windows", "x86_64", false)
            .unwrap()
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let payload = download_verified(&update, directory.path()).unwrap();
        assert_eq!(fs::metadata(payload).unwrap().len(), update.package.size);
    }
}
