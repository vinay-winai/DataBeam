//! Restore the existing Windows release instance when its EXE is launched again.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::{self, JoinHandle};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND, WAIT_OBJECT_0},
    System::Threading::{
        CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
    },
    UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, EnumWindows, GetPropW, GetWindowThreadProcessId, RemovePropW,
        SetPropW, ShowWindowAsync, SW_RESTORE,
    },
};

const NAME: &str = "Local\\com.vinaywinai.databeam.release.activate";

fn wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(Some(0)).collect()
}

struct Event(HANDLE);
// Kernel event handles can be waited on and signalled across threads.
unsafe impl Send for Event {}
unsafe impl Sync for Event {}
impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

unsafe extern "system" fn grant_foreground(window: HWND, key: isize) -> i32 {
    if unsafe { !GetPropW(window, key as *const u16).is_null() } {
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut pid);
            AllowSetForegroundWindow(pid);
        }
        return 0;
    }
    1
}

fn notify_named(name: &str) -> bool {
    let name = wide(name);
    for _ in 0..10 {
        let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if !handle.is_null() {
            let event = Event(handle);
            // Transfer foreground permission only to the window registered by
            // the primary instance, independent of its title or EXE filename.
            unsafe {
                EnumWindows(Some(grant_foreground), name.as_ptr() as isize);
            }
            return unsafe { SetEvent(event.0) != 0 };
        }
        thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

pub fn notify_existing() -> bool {
    notify_named(NAME)
}

pub struct Listener {
    event: Arc<Event>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    requests: mpsc::Receiver<()>,
    hwnd: Option<isize>,
    name: Vec<u16>,
}

impl Listener {
    pub fn start(ctx: eframe::egui::Context, hwnd: Option<isize>) -> std::io::Result<Self> {
        Self::start_named(ctx, hwnd, NAME)
    }

    fn start_named(
        ctx: eframe::egui::Context,
        hwnd: Option<isize>,
        name: &str,
    ) -> std::io::Result<Self> {
        let name = wide(name);
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let event = Arc::new(Event(handle));
        if let Some(hwnd) = hwnd {
            unsafe {
                SetPropW(hwnd as HWND, name.as_ptr(), 1usize as HANDLE);
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, requests) = mpsc::channel();
        let worker_event = event.clone();
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            while unsafe { WaitForSingleObject(worker_event.0, INFINITE) } == WAIT_OBJECT_0 {
                if worker_stop.load(Ordering::Acquire) {
                    break;
                }
                if tx.send(()).is_err() {
                    break;
                }
                if let Some(hwnd) = hwnd {
                    // Do not block the worker on the UI thread: it may be
                    // shutting down. The app restores tray styles itself.
                    unsafe {
                        ShowWindowAsync(hwnd as HWND, SW_RESTORE);
                    }
                }
                ctx.request_repaint();
            }
        });
        Ok(Self {
            event,
            stop,
            worker: Some(worker),
            requests,
            hwnd,
            name,
        })
    }

    pub fn take_request(&self) -> bool {
        self.requests.try_iter().count() > 0
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe {
            SetEvent(self.event.0);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(hwnd) = self.hwnd {
            unsafe {
                RemovePropW(hwnd as HWND, self.name.as_ptr());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_signal_child() {
        if let Ok(name) = std::env::var("DATABEAM_ACTIVATION_TEST_EVENT") {
            assert!(notify_named(&name));
        }
    }

    #[test]
    fn second_process_notifies_existing_instance_and_listener_stops() {
        let name = format!("Local\\DataBeam-activation-test-{}", std::process::id());
        let listener =
            Listener::start_named(eframe::egui::Context::default(), None, &name).unwrap();
        for _ in 0..2 {
            let result = crate::backend::new_hidden_command(std::env::current_exe().unwrap())
                .args(["--exact", "activation::tests::activation_signal_child"])
                .env("DATABEAM_ACTIVATION_TEST_EVENT", &name)
                .output()
                .unwrap();
            assert!(result.status.success());
            listener
                .requests
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap();
        }
        assert!(!listener.take_request());
        drop(listener);
        let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, wide(&name).as_ptr()) };
        assert!(handle.is_null(), "listener leaked its named event");
    }

    #[test]
    fn activation_restores_tray_state_without_resetting_a_transfer() {
        let _guard = crate::tray::TRAY_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let ctx = eframe::egui::Context::default();
        let name = format!("Local\\DataBeam-activation-state-{}", std::process::id());
        let mut app = crate::DataBeamApp::default();
        app.activation_listener = Some(Listener::start_named(ctx.clone(), None, &name).unwrap());
        app.window_in_tray = true;
        app.transfer_state = crate::TransferState::Running;
        app.transfer_progress = 0.5;
        assert!(notify_named(&name));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while app.window_in_tray && std::time::Instant::now() < deadline {
            app.poll_activation(&ctx);
            thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!app.window_in_tray);
        assert_eq!(app.transfer_state, crate::TransferState::Running);
        assert_eq!(app.transfer_progress, 0.5);
    }
}
