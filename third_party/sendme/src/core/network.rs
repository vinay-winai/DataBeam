//! Platform adapters can notify active endpoints without restarting a transfer.
use std::sync::OnceLock;
use tokio::sync::watch;

fn changes() -> &'static watch::Sender<u64> {
    static CHANGES: OnceLock<watch::Sender<u64>> = OnceLock::new();
    CHANGES.get_or_init(|| watch::channel(0).0)
}

pub fn notify_network_change() {
    changes().send_modify(|revision| *revision = revision.wrapping_add(1));
}

pub(super) fn watch_endpoint(endpoint: iroh::Endpoint) -> n0_future::task::AbortOnDropHandle<()> {
    let mut changes = changes().subscribe();
    n0_future::task::AbortOnDropHandle::new(tokio::spawn(async move {
        while changes.changed().await.is_ok() {
            endpoint.network_change().await;
        }
    }))
}

pub(super) fn describe_paths(paths: &iroh::endpoint::PathList<'_>) -> serde_json::Value {
    serde_json::Value::Array(paths.iter().map(|path| serde_json::json!({
        "remote": path.remote_addr().to_string(),
        "selected": path.is_selected(),
        "route": if path.is_relay() { "relay" } else { "direct" },
        "rtt_ms": path.rtt().as_millis(),
    })).collect())
}
