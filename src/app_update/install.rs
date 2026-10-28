use super::{download_verified, file_digest, AvailableUpdate, PackageKind};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Deserialize)]
struct InstallPlan {
    parent_pid: u32,
    kind: PackageKind,
    target: PathBuf,
    payload: PathBuf,
    workspace: PathBuf,
    version: String,
    payload_digest: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct RestartResult {
    helper_pid: u32,
    message: String,
}

pub(super) struct PreparedUpdate {
    directory: tempfile::TempDir,
    helper: PathBuf,
    config: PathBuf,
}

impl PreparedUpdate {
    pub fn launch(self) -> Result<(), String> {
        let log = fs::File::create(self.directory.path().join("helper.log"))
            .map_err(|error| error.to_string())?;
        let mut child = crate::backend::new_hidden_command(&self.helper)
            .arg("--databeam-apply-update")
            .arg(&self.config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|error| error.to_string())?;
        // Wait on the worker thread, keeping the UI and the current app alive
        // until the helper has validated its plan and is waiting for us.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.directory.path().join("helper-ready").is_file() {
            if child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                let error = fs::read_to_string(self.directory.path().join("helper.log"))
                    .unwrap_or_default();
                return Err(format!("Updater did not start: {error}"));
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Timed out starting the updater; the app was not changed.".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // The helper owns this directory until the restarted app cleans it.
        let _ = self.directory.keep();
        Ok(())
    }
}

fn app_bundle(executable: &Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .map(Path::to_path_buf)
}

fn install_target(
    kind: PackageKind,
    executable: &Path,
    appimage: Option<&Path>,
) -> Result<PathBuf, String> {
    match kind {
        PackageKind::MacDmg => app_bundle(executable)
            .ok_or_else(|| "Run the installed DataBeam.app to update it.".into()),
        PackageKind::LinuxAppImage => appimage.map(Path::to_path_buf)
            .ok_or_else(|| "Could not locate the running AppImage.".into()),
        PackageKind::LinuxDeb if executable != Path::new("/usr/bin/databeam") =>
            Err("Automatic DEB updates require an installed /usr/bin/databeam. Use the AppImage for portable updates.".into()),
        _ => Ok(executable.to_path_buf()),
    }
}

pub(super) fn prepare(update: &AvailableUpdate) -> Result<PreparedUpdate, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    let target = install_target(update.kind, &executable, appimage.as_deref())?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".databeam-update-");
    let directory = if update.kind == PackageKind::LinuxDeb {
        builder.tempdir()
    } else {
        builder.tempdir_in(target.parent().ok_or("Missing application folder")?)
    }.map_err(|error| format!("Cannot stage update beside the app: {error}. Move DataBeam to a writable application folder and retry."))?;
    let workspace = directory
        .path()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let package = download_verified(update, &workspace)?;
    let payload = if update.kind == PackageKind::MacDmg {
        prepare_mac_bundle(&package, &workspace, &update.version)?
    } else {
        package
    };
    set_executable(&payload)?;
    let payload_digest = if payload.is_file() {
        Some(file_digest(&payload)?)
    } else {
        None
    };
    let helper = workspace.join(if cfg!(windows) {
        "updater.exe"
    } else {
        "updater"
    });
    fs::copy(&executable, &helper).map_err(|error| error.to_string())?;
    set_executable(&helper)?;
    let plan = InstallPlan {
        parent_pid: std::process::id(),
        kind: update.kind,
        target,
        payload,
        workspace,
        version: update.version.clone(),
        payload_digest,
    };
    validate_plan(&plan)?;
    let config = plan.workspace.join("install.json");
    fs::write(
        &config,
        serde_json::to_vec(&plan).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(PreparedUpdate {
        directory,
        helper,
        config,
    })
}

fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    if path.is_file() {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }
    let _ = path;
    Ok(())
}

#[cfg(target_os = "macos")]
fn prepare_mac_bundle(package: &Path, workspace: &Path, version: &str) -> Result<PathBuf, String> {
    let mount = workspace.join("mount");
    fs::create_dir(&mount).map_err(|error| error.to_string())?;
    run(
        "/usr/bin/hdiutil",
        &[
            Path::new("attach"),
            package,
            Path::new("-readonly"),
            Path::new("-nobrowse"),
            Path::new("-mountpoint"),
            &mount,
        ],
    )?;
    let bundle = workspace.join("DataBeam.app");
    let result = (|| {
        let source = mount.join("DataBeam.app");
        verify_mac_bundle(&source, version)?;
        run("/usr/bin/ditto", &[&source, &bundle])?;
        verify_mac_bundle(&bundle, version)?;
        Ok(bundle)
    })();
    let detach = run("/usr/bin/hdiutil", &[Path::new("detach"), &mount]);
    match (result, detach) {
        (Ok(bundle), Ok(())) => Ok(bundle),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}

#[cfg(not(target_os = "macos"))]
fn prepare_mac_bundle(_: &Path, _: &Path, _: &str) -> Result<PathBuf, String> {
    Err("macOS bundles can only be installed on macOS.".into())
}

#[cfg(target_os = "macos")]
fn verify_mac_bundle(bundle: &Path, version: &str) -> Result<(), String> {
    run(
        "/usr/bin/codesign",
        &[
            Path::new("--verify"),
            Path::new("--deep"),
            Path::new("--strict"),
            bundle,
        ],
    )?;
    let plist = bundle.join("Contents/Info.plist");
    for (key, expected) in [
        ("CFBundleIdentifier", "com.vinaywinai.databeam"),
        ("CFBundleShortVersionString", version),
    ] {
        let output = crate::backend::new_hidden_command("/usr/libexec/PlistBuddy")
            .args(["-c", &format!("Print :{key}")])
            .arg(&plist)
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != expected {
            return Err(format!("Downloaded app has an unexpected {key}"));
        }
    }
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    verify_mac_architecture(&bundle.join("Contents/MacOS/databeam"), arch)
}

#[cfg(target_os = "macos")]
fn verify_mac_architecture(executable: &Path, arch: &str) -> Result<(), String> {
    // Apple's lipo consumes everything after -verify_arch as architecture
    // names. Its input file must come before that option.
    run(
        "/usr/bin/lipo",
        &[executable, Path::new("-verify_arch"), Path::new(arch)],
    )
}

fn run(program: &str, args: &[&Path]) -> Result<(), String> {
    let output = crate::backend::new_hidden_command(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn validate_plan(plan: &InstallPlan) -> Result<(), String> {
    if !plan.workspace.is_absolute()
        || !plan
            .workspace
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".databeam-update-"))
        || !plan.target.is_absolute()
        || plan.target.file_name().is_none()
        || plan.payload.parent() != Some(plan.workspace.as_path())
        || plan.target.starts_with(&plan.workspace)
        || (plan.kind != PackageKind::LinuxDeb && plan.workspace.parent() != plan.target.parent())
    {
        return Err("Invalid update installation paths".into());
    }
    if plan.kind == PackageKind::LinuxDeb && plan.target != Path::new("/usr/bin/databeam") {
        return Err("Invalid DEB installation target".into());
    }
    if plan
        .payload
        .canonicalize()
        .map_err(|error| error.to_string())?
        != plan.payload
        || plan
            .workspace
            .canonicalize()
            .map_err(|error| error.to_string())?
            != plan.workspace
        || plan
            .target
            .canonicalize()
            .map_err(|error| error.to_string())?
            != plan.target
    {
        return Err("Update paths changed or contain symbolic links".into());
    }
    Ok(())
}

#[cfg(windows)]
fn wait_for_exit(pid: u32, timeout: Duration) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    };
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return if GetLastError() == ERROR_INVALID_PARAMETER {
                Ok(())
            } else {
                Err("Cannot wait for the running DataBeam process".into())
            };
        }
        let result = WaitForSingleObject(handle, timeout.as_millis().min(u32::MAX as u128) as u32);
        CloseHandle(handle);
        if result == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err("Timed out waiting for DataBeam to exit".into())
        }
    }
}

#[cfg(unix)]
fn wait_for_exit(pid: u32, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 {
            return if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                Ok(())
            } else {
                Err("Cannot wait for the running DataBeam process".into())
            };
        }
        if Instant::now() >= deadline {
            return Err("Timed out waiting for DataBeam to exit".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn rename_retry(source: &Path, destination: &Path) -> Result<(), String> {
    let mut last_error = None;
    for _ in 0..30 {
        match fs::rename(source, destination) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Err(last_error.unwrap().to_string())
}

/// Same-filesystem swap with restoration if staging cannot be installed.
fn replace(target: &Path, payload: &Path, backup: &Path) -> Result<(), String> {
    rename_retry(target, backup)?;
    if let Err(error) = rename_retry(payload, target) {
        rename_retry(backup, target)
            .map_err(|restore| format!("{error}; restoring previous app failed: {restore}"))?;
        return Err(error);
    }
    Ok(())
}

fn launch_app(plan: &InstallPlan, config: &Path) -> Result<Child, String> {
    let executable = if plan.kind == PackageKind::MacDmg {
        plan.target.join("Contents/MacOS/databeam")
    } else {
        plan.target.clone()
    };
    crate::backend::new_hidden_command(executable)
        .arg("--databeam-update-result")
        .arg(config)
        .current_dir(plan.target.parent().ok_or("Missing application folder")?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())
}

fn report_result(plan: &InstallPlan, message: String) -> Result<(), String> {
    fs::write(
        plan.workspace.join("result.json"),
        serde_json::to_vec(&RestartResult {
            helper_pid: std::process::id(),
            message,
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn apply(config: &Path) -> Result<(), String> {
    let plan: InstallPlan =
        serde_json::from_slice(&fs::read(config).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    validate_plan(&plan)?;
    fs::write(plan.workspace.join("helper-ready"), b"ready").map_err(|error| error.to_string())?;
    wait_for_exit(plan.parent_pid, Duration::from_secs(60))?;
    let backup = plan.workspace.join("previous");
    let installed = (|| {
        if let Some(expected) = &plan.payload_digest {
            if file_digest(&plan.payload)? != *expected {
                return Err(
                    "Staged package checksum changed; the existing app was preserved.".into(),
                );
            }
        }
        #[cfg(target_os = "macos")]
        if plan.kind == PackageKind::MacDmg {
            verify_mac_bundle(&plan.payload, &plan.version)?;
        }
        if plan.kind == PackageKind::LinuxDeb {
            run(
                "pkexec",
                &[
                    Path::new("/usr/bin/dpkg"),
                    Path::new("--install"),
                    &plan.payload,
                ],
            )
        } else {
            replace(&plan.target, &plan.payload, &backup)
        }
    })();
    match installed {
        Err(error) => {
            report_result(&plan, format!("Update failed: {error}"))?;
            launch_app(&plan, config)?;
        }
        Ok(()) => {
            report_result(&plan, format!("Updated to DataBeam v{}.", plan.version))?;
            let launched = launch_app(&plan, config);
            let result = launched.and_then(|mut child| {
                let deadline = Instant::now() + Duration::from_secs(45);
                loop {
                    if plan.workspace.join("restart-ready").is_file() {
                        return Ok(());
                    }
                    if child
                        .try_wait()
                        .map_err(|error| error.to_string())?
                        .is_some()
                    {
                        if plan.workspace.join("restart-ready").is_file() {
                            return Ok(());
                        }
                        break;
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err("The updated app did not finish starting".to_string())
            });
            if let Err(error) = result {
                if plan.kind != PackageKind::LinuxDeb {
                    rename_retry(&plan.target, &plan.workspace.join("failed"))?;
                    rename_retry(&backup, &plan.target)?;
                }
                report_result(&plan, format!("Update restart failed: {error}. The previous portable app was restored where possible."))?;
                launch_app(&plan, config)?;
            }
        }
    }
    Ok(())
}

pub(super) fn acknowledge_restart(config: &Path) -> Result<String, String> {
    let plan: InstallPlan =
        serde_json::from_slice(&fs::read(config).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    // The payload has moved into place; validate the cleanup root independently.
    if config != plan.workspace.join("install.json")
        || !plan.workspace.is_absolute()
        || !plan
            .workspace
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".databeam-update-"))
        || plan.target.starts_with(&plan.workspace)
        || (plan.kind != PackageKind::LinuxDeb && plan.workspace.parent() != plan.target.parent())
    {
        return Err("Invalid update cleanup directory".into());
    }
    let result: RestartResult = serde_json::from_slice(
        &fs::read(plan.workspace.join("result.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::write(plan.workspace.join("restart-ready"), b"ready").map_err(|error| error.to_string())?;
    std::thread::spawn(move || {
        if wait_for_exit(result.helper_pid, Duration::from_secs(60)).is_ok() {
            let _ = fs::remove_dir_all(&plan.workspace);
        }
    });
    Ok(result.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_lipo_accepts_native_binary_and_rejects_missing_architecture() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("Data Beam's test executable");
        fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let native = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        verify_mac_architecture(&executable, native)
            .expect("native Mach-O architecture must verify");
        assert!(verify_mac_architecture(&executable, "i386").is_err());
    }

    #[test]
    fn portable_replacement_preserves_backup_and_exact_new_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Data Beam's app.exe");
        let payload = dir.path().join("new");
        let backup = dir.path().join("old");
        fs::write(&target, b"old app").unwrap();
        fs::write(&payload, b"new app").unwrap();
        replace(&target, &payload, &backup).unwrap();
        assert_eq!(fs::read(target).unwrap(), b"new app");
        assert_eq!(fs::read(backup).unwrap(), b"old app");
    }

    #[test]
    fn failed_staging_restores_previous_app() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("app");
        let backup = dir.path().join("old");
        fs::write(&target, b"old app").unwrap();
        assert!(replace(&target, &dir.path().join("missing"), &backup).is_err());
        assert_eq!(fs::read(target).unwrap(), b"old app");
        assert!(!backup.exists());
    }

    #[test]
    fn targets_use_original_appimage_or_whole_mac_bundle() {
        assert_eq!(
            install_target(
                PackageKind::MacDmg,
                Path::new("/Applications/DataBeam.app/Contents/MacOS/databeam"),
                None
            )
            .unwrap(),
            Path::new("/Applications/DataBeam.app")
        );
        assert_eq!(
            install_target(
                PackageKind::LinuxAppImage,
                Path::new("/tmp/.mount/bin/databeam"),
                Some(Path::new("/home/user/DataBeam.AppImage"))
            )
            .unwrap(),
            Path::new("/home/user/DataBeam.AppImage")
        );
        assert!(install_target(
            PackageKind::LinuxDeb,
            Path::new("/home/user/databeam"),
            None
        )
        .is_err());
    }

    #[test]
    fn replacement_cannot_escape_workspace_or_overwrite_itself() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::Builder::new()
            .prefix(".databeam-update-")
            .tempdir_in(root.path())
            .unwrap();
        let target = root.path().join("app");
        fs::write(&target, b"old").unwrap();
        let payload = workspace.path().join("new");
        fs::write(&payload, b"new").unwrap();
        let mut plan = InstallPlan {
            parent_pid: 1,
            kind: PackageKind::WindowsExe,
            target: target.canonicalize().unwrap(),
            payload: payload.canonicalize().unwrap(),
            workspace: workspace.path().canonicalize().unwrap(),
            version: "0.7.0".into(),
            payload_digest: None,
        };
        validate_plan(&plan).unwrap();
        plan.payload = plan.target.clone();
        assert!(validate_plan(&plan).is_err());
        plan.payload = payload.canonicalize().unwrap();
        plan.target = plan.workspace.clone();
        assert!(validate_plan(&plan).is_err());
    }

    #[test]
    fn running_executable_is_replaced_and_bad_restart_rolls_back() {
        let root = tempfile::tempdir().unwrap();
        let fixture_source = root.path().join("fixture.rs");
        fs::write(&fixture_source, r#"
            fn main() {
                let args: Vec<_> = std::env::args_os().collect();
                if args.get(1).is_some_and(|arg| arg == "--hold") {
                    std::thread::sleep(std::time::Duration::from_millis(700));
                    return;
                }
                if cfg!(broken_update) { std::process::exit(1); }
                let config = std::path::Path::new(&args[2]);
                let root = config.parent().unwrap();
                std::fs::write(root.join("launched-pid"), std::process::id().to_string()).unwrap();
                std::fs::write(root.join("launched"), if cfg!(new_update) { b"new" } else { b"old" }).unwrap();
                std::fs::write(root.join("restart-ready"), b"ready").unwrap();
            }
        "#).unwrap();
        let build_fixture = |name: &str, cfg: &str| {
            let binary = root
                .path()
                .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            let output = crate::backend::new_hidden_command("rustc")
                .arg("--edition=2021")
                .arg("--cfg")
                .arg(cfg)
                .arg(&fixture_source)
                .arg("-o")
                .arg(&binary)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            binary
        };
        let old = build_fixture("old", "old_update");
        let new = build_fixture("new", "new_update");
        let broken = build_fixture("broken", "broken_update");
        for (candidate, marker) in [(&new, b"new"), (&broken, b"old")] {
            let target = root
                .path()
                .join(format!("Data Beam's app{}", std::env::consts::EXE_SUFFIX));
            fs::copy(&old, &target).unwrap();
            let workspace = tempfile::Builder::new()
                .prefix(".databeam-update-")
                .tempdir_in(root.path())
                .unwrap();
            let workspace_path = workspace.path().canonicalize().unwrap();
            let payload = workspace_path.join(format!("payload{}", std::env::consts::EXE_SUFFIX));
            fs::copy(candidate, &payload).unwrap();
            let mut holder = crate::backend::new_hidden_command(&target)
                .arg("--hold")
                .spawn()
                .unwrap();
            let parent_pid = holder.id();
            let reaper = std::thread::spawn(move || holder.wait().unwrap());
            let plan = InstallPlan {
                parent_pid,
                kind: PackageKind::WindowsExe,
                target: target.canonicalize().unwrap(),
                payload: payload.canonicalize().unwrap(),
                workspace: workspace_path.clone(),
                version: "0.7.0".into(),
                payload_digest: Some(file_digest(&payload).unwrap()),
            };
            let config = workspace_path.join("install.json");
            fs::write(&config, serde_json::to_vec(&plan).unwrap()).unwrap();
            apply(&config).unwrap();
            reaper.join().unwrap();
            // Reap the relaunched fixture before Windows removes its directory.
            let deadline = Instant::now() + Duration::from_secs(5);
            while !workspace_path.join("restart-ready").exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(fs::read(workspace_path.join("launched")).unwrap(), marker);
            let launched_pid: u32 = fs::read_to_string(workspace_path.join("launched-pid"))
                .unwrap()
                .parse()
                .unwrap();
            #[cfg(windows)]
            wait_for_exit(launched_pid, Duration::from_secs(5)).unwrap();
            #[cfg(unix)]
            unsafe {
                libc::waitpid(launched_pid as libc::pid_t, std::ptr::null_mut(), 0);
            }
            let result: RestartResult =
                serde_json::from_slice(&fs::read(workspace_path.join("result.json")).unwrap())
                    .unwrap();
            if marker == b"new" {
                assert_eq!(file_digest(&target).unwrap(), file_digest(&new).unwrap());
                assert!(result.message.starts_with("Updated to"));
                assert_eq!(
                    file_digest(&workspace_path.join("previous")).unwrap(),
                    file_digest(&old).unwrap()
                );
            } else {
                assert_eq!(file_digest(&target).unwrap(), file_digest(&old).unwrap());
                assert!(result.message.starts_with("Update restart failed"));
            }
            // The restarted app only removes staging after its helper exits.
            let mut cleaner = crate::backend::new_hidden_command(&old)
                .arg("--hold")
                .spawn()
                .unwrap();
            let cleanup_result = RestartResult {
                helper_pid: cleaner.id(),
                message: result.message.clone(),
            };
            fs::write(
                workspace_path.join("result.json"),
                serde_json::to_vec(&cleanup_result).unwrap(),
            )
            .unwrap();
            let reaper = std::thread::spawn(move || cleaner.wait().unwrap());
            assert_eq!(acknowledge_restart(&config).unwrap(), result.message);
            let deadline = Instant::now() + Duration::from_secs(5);
            while workspace_path.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            reaper.join().unwrap();
            assert!(
                !workspace_path.exists(),
                "Update staging must be removed after helper exit"
            );
            assert!(target.exists(), "Cleanup must preserve the installed app");
        }
    }
}
