use eframe::egui;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const GUIDE_URL: &str = "https://github.com/vinay-winai/DataBeam/blob/main/troubleshoot.md";
const RAW_GUIDE_URL: &str =
    "https://raw.githubusercontent.com/vinay-winai/DataBeam/main/troubleshoot.md";
const CHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(5 * 60);

fn revision(contents: &str) -> String {
    // A Windows checkout may embed CRLF while GitHub serves LF.
    Sha256::digest(contents.replace("\r\n", "\n").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GuideNews {
    seen_revision: String,
    latest_revision: Option<String>,
}

impl Default for GuideNews {
    fn default() -> Self {
        Self {
            seen_revision: revision(include_str!("../troubleshoot.md")),
            latest_revision: None,
        }
    }
}

impl GuideNews {
    pub fn has_unread(&self) -> bool {
        self.latest_revision
            .as_ref()
            .is_some_and(|latest| latest != &self.seen_revision)
    }

    fn observe(&mut self, latest: String) -> bool {
        if self.latest_revision.as_ref() == Some(&latest) {
            return false;
        }
        self.latest_revision = Some(latest);
        true
    }

    pub fn mark_read(&mut self) -> bool {
        if !self.has_unread() {
            return false;
        }
        self.seen_revision = self.latest_revision.clone().unwrap();
        true
    }
}

#[derive(Default)]
pub struct GuideMonitor {
    pending: Option<mpsc::Receiver<Result<String, String>>>,
    next_check: Option<Instant>,
}

impl GuideMonitor {
    /// Runs on the UI thread; all network work happens on one bounded worker.
    /// Returns true only when the persisted news state changed.
    pub fn poll(&mut self, ctx: &egui::Context, news: &mut GuideNews) -> bool {
        let now = Instant::now();
        let mut changed = false;
        if let Some(rx) = self.pending.take() {
            match rx.try_recv() {
                Ok(Ok(latest)) => {
                    changed = news.observe(latest);
                    self.next_check = Some(now + CHECK_INTERVAL);
                }
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                    // Keep any unread badge when offline or when GitHub fails.
                    self.next_check = Some(now + RETRY_INTERVAL);
                }
                Err(mpsc::TryRecvError::Empty) => self.pending = Some(rx),
            }
        }
        if self.pending.is_none() {
            let delay = self
                .next_check
                .map(|next| next.saturating_duration_since(now))
                .unwrap_or_default();
            if delay.is_zero() {
                let (tx, rx) = mpsc::channel();
                self.pending = Some(rx);
                let wake = ctx.clone();
                std::thread::spawn(move || {
                    let result = fetch_revision();
                    let _ = tx.send(result);
                    wake.request_repaint();
                });
            } else {
                ctx.request_repaint_after(delay);
            }
        }
        changed
    }
}

fn fetch_revision() -> Result<String, String> {
    // curl is already used by DataBeam on macOS/Linux and is included in
    // supported Windows versions. Keep TLS verification enabled on every OS.
    let output = crate::backend::new_hidden_command("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "5",
            "--max-time",
            "15",
            "--max-filesize",
            "1048576",
            "--user-agent",
            "DataBeam",
            RAW_GUIDE_URL,
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let contents = std::str::from_utf8(&output.stdout).map_err(|error| error.to_string())?;
    if contents.trim().is_empty() {
        return Err("Empty troubleshooting guide".into());
    }
    Ok(revision(contents))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_guide_is_read_but_content_edits_are_unread() {
        let mut news = GuideNews::default();
        assert!(!news.has_unread());
        news.observe(revision(include_str!("../troubleshoot.md")));
        assert!(!news.has_unread());
        news.observe(revision("# News\nA new fix is available.\n"));
        assert!(news.has_unread());
        assert!(news.mark_read());
        assert!(!news.has_unread());
        assert!(!news.mark_read());
    }

    #[test]
    fn unread_and_read_revisions_survive_restart() {
        let mut news = GuideNews::default();
        let update = revision("updated guide");
        news.observe(update.clone());
        let mut restored: GuideNews =
            serde_json::from_str(&serde_json::to_string(&news).unwrap()).unwrap();
        assert!(restored.has_unread());
        restored.mark_read();
        let mut restored: GuideNews =
            serde_json::from_str(&serde_json::to_string(&restored).unwrap()).unwrap();
        restored.observe(update);
        assert!(!restored.has_unread());
        restored.observe(revision("another edit"));
        assert!(restored.has_unread());
    }

    #[test]
    fn repeated_check_does_not_restore_a_dismissed_badge() {
        let mut news = GuideNews::default();
        let update = revision("updated guide");
        news.observe(update.clone());
        news.mark_read();
        assert!(!news.observe(update));
        assert!(!news.has_unread());
    }

    #[test]
    fn line_endings_do_not_create_false_notifications() {
        assert_eq!(revision("# News\r\nhello\r\n"), revision("# News\nhello\n"));
        assert_ne!(revision("hello\n"), revision("hello!\n"));
    }

    #[test]
    fn failed_or_disconnected_checks_preserve_unread_state() {
        let ctx = egui::Context::default();
        for disconnected in [false, true] {
            let mut news = GuideNews::default();
            news.observe(revision("updated guide"));
            let before = serde_json::to_string(&news).unwrap();
            let (tx, rx) = mpsc::channel();
            if !disconnected {
                tx.send(Err("offline".into())).unwrap();
            }
            drop(tx);
            let mut monitor = GuideMonitor {
                pending: Some(rx),
                next_check: None,
            };
            assert!(!monitor.poll(&ctx, &mut news));
            assert!(news.has_unread());
            assert_eq!(serde_json::to_string(&news).unwrap(), before);
            assert!(monitor.pending.is_none());
            assert!(monitor.next_check.unwrap() > Instant::now());
        }
    }

    #[test]
    fn pending_check_is_not_duplicated_and_success_schedules_refresh() {
        let ctx = egui::Context::default();
        let mut news = GuideNews::default();
        let (tx, rx) = mpsc::channel();
        let mut monitor = GuideMonitor {
            pending: Some(rx),
            next_check: None,
        };
        assert!(!monitor.poll(&ctx, &mut news));
        assert!(monitor.pending.is_some());
        tx.send(Ok(revision("updated guide"))).unwrap();
        assert!(monitor.poll(&ctx, &mut news));
        assert!(news.has_unread());
        assert!(monitor.pending.is_none());
        assert!(monitor.next_check.unwrap() > Instant::now());
    }
}
