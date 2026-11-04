//! Linux instance lock with close-on-exec ownership.
//!
//! The previous single-instance crate creates an inheritable socket. An
//! updater or transfer child then keeps the lock alive after the UI exits,
//! preventing the updated app from starting and causing update rollback.
use std::io;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener};

pub struct SingleInstance {
    listener: Option<UnixListener>,
}

impl SingleInstance {
    pub fn new(name: &str) -> io::Result<Self> {
        let address = SocketAddr::from_abstract_name(name.as_bytes())?;
        // std creates this socket with SOCK_CLOEXEC, atomically, so concurrent
        // child launches cannot inherit it between socket creation and fcntl.
        match UnixListener::bind_addr(&address) {
            Ok(listener) => Ok(Self { listener: Some(listener) }),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse =>
                Ok(Self { listener: None }),
            Err(error) => Err(error),
        }
    }

    pub fn is_single(&self) -> bool {
        self.listener.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn non_windows_linux_instance_blocks_duplicates_and_releases_on_drop() {
        let name = format!("databeam-instance-test-{}", std::process::id());
        let first = SingleInstance::new(&name).unwrap();
        assert!(first.is_single());
        assert!(!SingleInstance::new(&name).unwrap().is_single());
        drop(first);
        assert!(SingleInstance::new(&name).unwrap().is_single());
    }

    // Runs in a separate executable, standing in for the updater helper.
    #[test]
    fn non_windows_linux_instance_exec_child() {
        let Some(directory) = std::env::var_os("DATABEAM_INSTANCE_TEST_CHILD") else { return; };
        let directory = std::path::Path::new(&directory);
        fs::write(directory.join("ready"), b"ready").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !directory.join("exit").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn non_windows_linux_updater_child_does_not_keep_instance_lock_alive() {
        let directory = tempfile::tempdir().unwrap();
        let name = format!("databeam-update-lock-test-{}", std::process::id());
        let original = SingleInstance::new(&name).unwrap();
        assert!(original.is_single());
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "single_instance::tests::non_windows_linux_instance_exec_child"])
            .env("DATABEAM_INSTANCE_TEST_CHILD", directory.path())
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !directory.path().join("ready").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let ready = directory.path().join("ready").exists();
        drop(original);
        let restarted = SingleInstance::new(&name).unwrap();
        let restart_allowed = restarted.is_single();
        fs::write(directory.path().join("exit"), b"exit").unwrap();
        let status = child.wait().unwrap();
        assert!(ready && status.success(), "updater fixture did not start");
        assert!(restart_allowed, "updater inherited the original app's instance lock");
    }
}
