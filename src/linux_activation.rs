//! Restore the existing Linux instance when a desktop launcher runs it again.
use std::io;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

fn activation_name() -> &'static str {
    if crate::app_update::managed_by_flatpak() {
        "com.vinaywinai.databeam.flatpak.activate"
    } else {
        "com.vinaywinai.databeam.release.activate"
    }
}

fn notify_named(name: &str) -> bool {
    let Ok(address) = SocketAddr::from_abstract_name(name.as_bytes()) else { return false; };
    let Ok(socket) = UnixDatagram::unbound() else { return false; };
    if socket.set_write_timeout(Some(Duration::from_millis(100))).is_err() { return false; }
    // A pinned launcher can be clicked again while the first app is starting.
    for _ in 0..10 {
        if socket.send_to_addr(b"show", &address).is_ok() { return true; }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

pub fn notify_existing() -> bool {
    notify_named(activation_name())
}

pub struct Listener {
    requests: mpsc::Receiver<()>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Listener {
    pub fn start(ctx: eframe::egui::Context, _hwnd: Option<isize>) -> io::Result<Self> {
        Self::start_named(ctx, activation_name())
    }

    fn start_named(ctx: eframe::egui::Context, name: &str) -> io::Result<Self> {
        let address = SocketAddr::from_abstract_name(name.as_bytes())?;
        // std sockets are close-on-exec; neither transfer children nor the
        // updater can retain this endpoint after the primary app exits.
        let socket = UnixDatagram::bind_addr(&address)?;
        socket.set_read_timeout(Some(Duration::from_millis(100)))?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (tx, requests) = mpsc::channel();
        let worker = thread::Builder::new().name("databeam-activation".into()).spawn(move || {
            let mut buffer = [0; 32];
            while !worker_stop.load(Ordering::Acquire) {
                match socket.recv(&mut buffer) {
                    Ok(size) if &buffer[..size] == b"show" => {
                        if tx.send(()).is_err() { break; }
                        ctx.request_repaint();
                    }
                    Ok(_) => {}
                    Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => {}
                    Err(_) => break,
                }
            }
        })?;
        Ok(Self { requests, stop, worker: Some(worker) })
    }

    pub fn take_request(&self) -> bool {
        self.requests.try_iter().count() > 0
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_windows_linux_activation_signal_child() {
        if let Ok(name) = std::env::var("DATABEAM_LINUX_ACTIVATION_TEST") {
            assert!(notify_named(&name));
        }
    }

    #[test]
    fn non_windows_linux_relaunch_signals_existing_instance_and_listener_stops() {
        let name = format!("databeam-activation-test-{}", std::process::id());
        let listener = Listener::start_named(eframe::egui::Context::default(), &name).unwrap();
        for _ in 0..2 {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "activation::tests::non_windows_linux_activation_signal_child"])
                .env("DATABEAM_LINUX_ACTIVATION_TEST", &name).output().unwrap();
            assert!(output.status.success());
            listener.requests.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        assert!(!listener.take_request());
        drop(listener);
        let address = SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
        UnixDatagram::bind_addr(&address).expect("activation listener leaked its endpoint");
    }
}
