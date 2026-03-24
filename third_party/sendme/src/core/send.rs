use crate::core::types::{
    apply_options, get_or_create_secret, AddrInfoOptions, AppHandle, SendOptions, SendResult,
};
use anyhow::Context;
use data_encoding::HEXLOWER;
use iroh::{discovery::pkarr::PkarrPublisher, Endpoint, RelayMode};
use iroh_blobs::{
    api::{
        blobs::{AddPathOptions, ImportMode},
        Store, TempTag,
    },
    format::collection::Collection,
    provider::events::{ConnectMode, EventMask, EventSender, RequestMode},
    store::fs::FsStore,
    ticket::BlobTicket,
    BlobFormat, BlobsProtocol,
};
use n0_future::StreamExt;
use n0_future::{task::AbortOnDropHandle, BufferedStreamExt};
use rand::Rng;
use serde::Serialize;
use std::{
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::{select, sync::mpsc};
use tracing::trace;
use walkdir::WalkDir;

fn emit_event(app_handle: &AppHandle, event_name: &str) {
    if let Some(handle) = app_handle {
        if let Err(e) = handle.emit_event(event_name) {
            tracing::warn!("Failed to emit event {}: {}", event_name, e);
        }
    }
}

fn emit_progress_event(
    app_handle: &AppHandle,
    bytes_transferred: u64,
    total_bytes: u64,
    speed_bps: f64,
) {
    if let Some(handle) = app_handle {
        let event_name = "transfer-progress";

        let speed_int = (speed_bps * 1000.0) as i64;

        let payload = format!("{}:{}:{}", bytes_transferred, total_bytes, speed_int);

        if let Err(e) = handle.emit_event_with_payload(event_name, &payload) {
            tracing::warn!("Failed to emit progress event: {}", e);
        }
    }
}

fn emit_active_connection_count(app_handle: &AppHandle, count: usize) {
    if let Some(handle) = app_handle {
        let event_name = "active-connection-count";
        let payload = count.to_string();

        if let Err(e) = handle.emit_event_with_payload(event_name, &payload) {
            tracing::warn!("Failed to emit active connection count event: {}", e);
        }
    }
}

#[derive(Serialize)]
struct SenderRequestStartedPayload {
    endpoint_id: String,
    connection_id: u64,
    request_id: u64,
    item_index: u64,
    hash_short: String,
    total_bytes: u64,
}

#[derive(Serialize)]
struct SenderRequestProgressPayload {
    connection_id: u64,
    request_id: u64,
    done_bytes: u64,
}

#[derive(Serialize)]
struct SenderRequestCompletedPayload {
    connection_id: u64,
    request_id: u64,
}

fn emit_event_with_payload<T: Serialize>(app_handle: &AppHandle, event_name: &str, payload: &T) {
    if let Some(handle) = app_handle {
        match serde_json::to_string(payload) {
            Ok(json) => {
                if let Err(e) = handle.emit_event_with_payload(event_name, &json) {
                    tracing::warn!("Failed to emit event {}: {}", event_name, e);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to serialize event {}: {}", event_name, e);
            }
        }
    }
}
pub async fn start_share(
    paths: Vec<PathBuf>,
    payload_root: String,
    options: SendOptions,
    app_handle: AppHandle,
) -> anyhow::Result<SendResult> {
    let secret_key = get_or_create_secret()?;

    let relay_mode: RelayMode = options.relay_mode.clone().into();

    let mut builder = Endpoint::builder()
        .alpns(vec![iroh_blobs::protocol::ALPN.to_vec()])
        .secret_key(secret_key)
        .relay_mode(relay_mode.clone());

    if options.ticket_type == AddrInfoOptions::Id {
        builder = builder.discovery(PkarrPublisher::n0_dns());
    }
    if let Some(addr) = options.magic_ipv4_addr {
        builder = builder.bind_addr_v4(addr);
    }
    if let Some(addr) = options.magic_ipv6_addr {
        builder = builder.bind_addr_v6(addr);
    }

    let suffix = rand::rng().random::<[u8; 16]>();
    let temp_base = {
        let base = options.blob_dir.clone().unwrap_or_else(std::env::temp_dir);
        let _ = std::fs::create_dir_all(&base);
        base
    };
    let blobs_data_dir = temp_base.join(format!(".sendme-send-{}", HEXLOWER.encode(&suffix)));
    if blobs_data_dir.exists() {
        anyhow::bail!(
            "can not share twice from the same directory: {}",
            temp_base.display(),
        );
    }
    let blobs_data_dir2 = blobs_data_dir.clone();
    let (progress_tx, progress_rx) = mpsc::channel(32);
    let app_handle_clone = app_handle.clone();
    anyhow::ensure!(!paths.is_empty(), "no valid paths to share");
    let entry_type = if paths.len() == 1 && paths[0].is_file() {
        "file"
    } else {
        "directory"
    };
    let entry_type_for_progress = entry_type.to_string();
    let paths2 = paths.clone();
    let payload_root2 = payload_root.clone();

    let setup = async move {
        let t0 = Instant::now();
        tokio::fs::create_dir_all(&blobs_data_dir2).await?;

        let endpoint = builder.bind().await?;

        let store = FsStore::load(&blobs_data_dir2).await?;

        let blobs = BlobsProtocol::new(
            &store,
            Some(EventSender::new(
                progress_tx,
                EventMask {
                    connected: ConnectMode::Notify,
                    get: RequestMode::NotifyLog,
                    ..EventMask::DEFAULT
                },
            )),
        );

        let import_result = import(paths2, payload_root2, blobs.store()).await?;
        let dt = t0.elapsed();

        let (ref _temp_tag, size, ref _collection) = import_result;
        let progress_handle = n0_future::task::spawn(show_provide_progress_with_logging(
            progress_rx,
            app_handle_clone,
            size,
            entry_type_for_progress,
        ));

        let router = iroh::protocol::Router::builder(endpoint)
            .accept(iroh_blobs::ALPN, blobs.clone())
            .spawn();

        let ep = router.endpoint();
        tokio::time::timeout(Duration::from_secs(30), async move {
            if !matches!(relay_mode, RelayMode::Disabled) {
                let _ = ep.online().await;
            }
        })
        .await?;

        anyhow::Ok((
            router,
            import_result,
            dt,
            blobs_data_dir2,
            store,
            progress_handle,
        ))
    };

    let setup_result = select! {
        x = setup => x,
        _ = tokio::signal::ctrl_c() => {
            anyhow::bail!("Operation cancelled");
        }
    };
    let (router, (temp_tag, size, _collection), _dt, _blobs_data_dir, store, progress_handle) =
        match setup_result {
            Ok(v) => v,
            Err(e) => {
                let _ = tokio::fs::remove_dir_all(&blobs_data_dir).await;
                return Err(e);
            }
        };
    let hash = temp_tag.hash();

    let mut addr = router.endpoint().addr();

    apply_options(&mut addr, options.ticket_type);

    let ticket = BlobTicket::new(addr, hash, BlobFormat::HashSeq);

    Ok(SendResult {
        ticket: ticket.to_string(),
        hash: hash.to_hex().to_string(),
        size,
        entry_type: entry_type.to_string(),
        router,
        temp_tag,
        blobs_data_dir,
        _progress_handle: AbortOnDropHandle::new(progress_handle),
        _store: store,
    })
}

async fn import(
    paths: Vec<PathBuf>,
    payload_root: String,
    db: &Store,
) -> anyhow::Result<(TempTag, u64, Collection)> {
    let data_sources = build_virtual_data_sources(paths, &payload_root)?;
    anyhow::ensure!(!data_sources.is_empty(), "no valid files to share");

    let parallelism = num_cpus::get();

    let mut names_and_tags = n0_future::stream::iter(data_sources)
        .map(|(name, path)| {
            let db = db.clone();
            async move {
                let import = db.add_path_with_opts(AddPathOptions {
                    path,
                    mode: ImportMode::TryReference,
                    format: iroh_blobs::BlobFormat::Raw,
                });
                let mut stream = import.stream().await;
                let mut item_size = 0;
                let temp_tag = loop {
                    let item = stream
                        .next()
                        .await
                        .context("import stream ended without a tag")?;
                    trace!("importing {name} {item:?}");
                    match item {
                        iroh_blobs::api::blobs::AddProgressItem::Size(size) => {
                            item_size = size;
                        }
                        iroh_blobs::api::blobs::AddProgressItem::CopyProgress(_) => {}
                        iroh_blobs::api::blobs::AddProgressItem::CopyDone => {}
                        iroh_blobs::api::blobs::AddProgressItem::OutboardProgress(_) => {}
                        iroh_blobs::api::blobs::AddProgressItem::Error(cause) => {
                            anyhow::bail!("error importing {}: {}", name, cause);
                        }
                        iroh_blobs::api::blobs::AddProgressItem::Done(tt) => {
                            break tt;
                        }
                    }
                };
                anyhow::Ok((name, temp_tag, item_size))
            }
        })
        .buffered_unordered(parallelism)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()?;

    names_and_tags.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
    let size = names_and_tags.iter().map(|(_, _, size)| *size).sum::<u64>();
    let (collection, tags) = names_and_tags
        .into_iter()
        .map(|(name, tag, _)| ((name, tag.hash()), tag))
        .unzip::<_, _, Collection, Vec<_>>();
    let temp_tag = collection.clone().store(db).await?;
    drop(tags);
    Ok((temp_tag, size, collection))
}

fn build_virtual_data_sources(
    paths: Vec<PathBuf>,
    payload_root: &str,
) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut data_sources = Vec::new();

    for source in paths {
        let canonical = source.canonicalize()?;
        anyhow::ensure!(
            canonical.exists(),
            "path {} does not exist",
            canonical.display()
        );

        let source_name = canonical
            .file_name()
            .context("shared path has no file name")?;
        let source_name = canonicalized_path_to_string(Path::new(source_name), true)?;

        if canonical.is_file() {
            data_sources.push((format!("{payload_root}/{source_name}"), canonical));
            continue;
        }

        for entry in WalkDir::new(&canonical).into_iter() {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("skipping inaccessible entry: {}", e);
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.into_path();
            let relative = match path.strip_prefix(&canonical) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("skipping {}: {}", path.display(), e);
                    continue;
                }
            };
            let relative = match canonicalized_path_to_string(relative, true) {
                Ok(name) => name,
                Err(e) => {
                    tracing::warn!("skipping {}: {}", path.display(), e);
                    continue;
                }
            };
            data_sources.push((format!("{payload_root}/{source_name}/{relative}"), path));
        }
    }

    Ok(data_sources)
}

pub fn canonicalized_path_to_string(
    path: impl AsRef<Path>,
    must_be_relative: bool,
) -> anyhow::Result<String> {
    let mut path_str = String::new();
    let parts = path
        .as_ref()
        .components()
        .filter_map(|c| match c {
            Component::Normal(x) => {
                let c = match x.to_str() {
                    Some(c) => c,
                    None => return Some(Err(anyhow::anyhow!("invalid character in path"))),
                };

                if !c.contains('/') && !c.contains('\\') {
                    Some(Ok(c))
                } else {
                    Some(Err(anyhow::anyhow!("invalid path component {:?}", c)))
                }
            }
            Component::RootDir => {
                if must_be_relative {
                    Some(Err(anyhow::anyhow!("invalid path component {:?}", c)))
                } else {
                    path_str.push('/');
                    None
                }
            }
            _ => Some(Err(anyhow::anyhow!("invalid path component {:?}", c))),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let parts = parts.join("/");
    path_str.push_str(&parts);
    Ok(path_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[cfg(unix)]
    #[test]
    fn canonicalized_path_rejects_backslash() {
        let path = Path::new("system-systemd\\x2dcryptsetup.slice");
        assert!(canonicalized_path_to_string(path, true).is_err());
    }

    #[test]
    fn canonicalized_path_accepts_normal() {
        let result = canonicalized_path_to_string(Path::new("subdir/file.txt"), true);
        assert_eq!(result.unwrap(), "subdir/file.txt");
    }

    #[test]
    fn canonicalized_path_rejects_parent_traversal() {
        assert!(canonicalized_path_to_string(Path::new("../etc/passwd"), true).is_err());
    }

    #[test]
    fn canonicalized_path_rejects_absolute_when_relative() {
        assert!(canonicalized_path_to_string(Path::new("/etc/passwd"), true).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn import_skips_invalid_files() {
        use tempfile::TempDir;

        let td = TempDir::new().unwrap();
        let dir = td.path().join("testdir");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("good.txt"), "hello").unwrap();
        std::fs::write(dir.join(format!("bad{}file.txt", '\\')), "bad").unwrap();

        let path = dir.canonicalize().unwrap();
        let root = path.parent().unwrap();
        let data_sources: Vec<(String, PathBuf)> = WalkDir::new(path.clone())
            .into_iter()
            .filter_map(|entry| {
                let entry = entry.ok()?;
                if !entry.file_type().is_file() {
                    return None;
                }
                let path = entry.into_path();
                let relative = path.strip_prefix(root).ok()?;
                canonicalized_path_to_string(relative, true)
                    .ok()
                    .map(|name| (name, path))
            })
            .collect();

        assert_eq!(data_sources.len(), 1, "should skip file with backslash");
        assert!(data_sources[0].0.contains("good.txt"));
    }
}

async fn show_provide_progress_with_logging(
    mut recv: mpsc::Receiver<iroh_blobs::provider::events::ProviderMessage>,
    app_handle: AppHandle,
    total_file_size: u64,
    entry_type: String,
) -> anyhow::Result<()> {
    use n0_future::FuturesUnordered;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let mut tasks = FuturesUnordered::new();
    let connections: Arc<Mutex<std::collections::HashMap<u64, String>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let total_bytes = total_file_size.max(1);

    #[derive(Clone)]
    struct TransferState {
        started: bool,
        count_toward_payload: bool,
        total_size: u64,
        last_offset: u64,
    }

    #[derive(Default)]
    struct ProgressTracker {
        transfers: std::collections::HashMap<(u64, u64), TransferState>,
        completed_bytes: u64,
        started_at: Option<Instant>,
    }

    fn snapshot_progress(tracker: &ProgressTracker, total_bytes: u64) -> (u64, f64) {
        let active_done = tracker
            .transfers
            .values()
            .filter(|state| state.count_toward_payload)
            .map(|state| state.last_offset.min(state.total_size))
            .sum::<u64>();
        let done_bytes = tracker
            .completed_bytes
            .saturating_add(active_done)
            .min(total_bytes);
        let speed_bps = tracker
            .started_at
            .and_then(|started_at| {
                let elapsed = started_at.elapsed().as_secs_f64();
                (elapsed > 0.0).then_some(done_bytes as f64 / elapsed)
            })
            .unwrap_or(0.0);
        (done_bytes, speed_bps)
    }

    let progress_tracker = Arc::new(Mutex::new(ProgressTracker::default()));
    let active_requests = Arc::new(AtomicUsize::new(0));
    let completed_requests = Arc::new(AtomicUsize::new(0));
    let has_emitted_started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let has_any_transfer = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let last_request_time: Arc<tokio::sync::Mutex<Option<Instant>>> =
        Arc::new(tokio::sync::Mutex::new(None));

    loop {
        tokio::select! {
            biased;
            item = recv.recv() => {
                let Some(item) = item else {
                    break;
                };

                match item {
                    iroh_blobs::provider::events::ProviderMessage::ClientConnectedNotify(msg) => {
                        let endpoint_id = msg
                            .endpoint_id
                            .map(|id| id.fmt_short().to_string())
                            .unwrap_or_else(|| "?".to_string());
                        connections.lock().await.insert(msg.connection_id, endpoint_id);
                    }
                    iroh_blobs::provider::events::ProviderMessage::ConnectionClosed(msg) => {
                        connections.lock().await.remove(&msg.connection_id);
                    }
                    iroh_blobs::provider::events::ProviderMessage::GetRequestReceivedNotify(msg) => {
                        let connection_id = msg.connection_id;
                        let request_id = msg.request_id;

                        active_requests.fetch_add(1, Ordering::SeqCst);

                        let mut last_time = last_request_time.lock().await;
                        *last_time = Some(Instant::now());
                        drop(last_time);

                        let app_handle_task = app_handle.clone();
                        let connections_task = connections.clone();
                        let progress_tracker_task = progress_tracker.clone();
                        let active_requests_task = active_requests.clone();
                        let completed_requests_task = completed_requests.clone();
                        let has_emitted_started_task = has_emitted_started.clone();
                        let has_any_transfer_task = has_any_transfer.clone();
                        let last_request_time_task = last_request_time.clone();
                        let entry_type_task = entry_type.clone();

                        let mut rx = msg.rx;
                        tasks.push(async move {
                            let mut transfer_started = false;
                            let mut request_completed = false;

                            while let Ok(Some(update)) = rx.recv().await {
                                match update {
                                    iroh_blobs::provider::events::RequestUpdate::Started(msg) => {
                                        let endpoint_id = connections_task
                                            .lock()
                                            .await
                                            .get(&connection_id)
                                            .cloned()
                                            .unwrap_or_else(|| "?".to_string());
                                        let hash_short = msg.hash.fmt_short().to_string();
                                        let total_size = msg.size.max(1);
                                        let count_toward_payload = request_id > 0 && msg.index > 0;

                                        let (active_count, done_bytes, speed_bps) = {
                                            let mut tracker = progress_tracker_task.lock().await;
                                            if tracker.started_at.is_none() {
                                                tracker.started_at = Some(Instant::now());
                                            }
                                            let key = (connection_id, request_id);
                                            match tracker.transfers.remove(&key) {
                                                Some(state) if state.started => {
                                                    if state.count_toward_payload {
                                                        tracker.completed_bytes = tracker
                                                            .completed_bytes
                                                            .saturating_add(state.total_size);
                                                    }
                                                    tracker.transfers.insert(
                                                        key,
                                                        TransferState {
                                                            started: true,
                                                            count_toward_payload,
                                                            total_size,
                                                            last_offset: 0,
                                                        },
                                                    );
                                                }
                                                Some(mut state) => {
                                                    state.started = true;
                                                    state.count_toward_payload = count_toward_payload;
                                                    state.total_size = total_size.max(state.last_offset).max(1);
                                                    tracker.transfers.insert(key, state);
                                                }
                                                None => {
                                                    tracker.transfers.insert(
                                                        key,
                                                        TransferState {
                                                            started: true,
                                                            count_toward_payload,
                                                            total_size,
                                                            last_offset: 0,
                                                        },
                                                    );
                                                }
                                            }
                                            let (done_bytes, speed_bps) =
                                                snapshot_progress(&tracker, total_bytes);
                                            (tracker.transfers.len(), done_bytes, speed_bps)
                                        };

                                        emit_event_with_payload(
                                            &app_handle_task,
                                            "sender-request-started",
                                            &SenderRequestStartedPayload {
                                                endpoint_id,
                                                connection_id,
                                                request_id,
                                                item_index: msg.index,
                                                hash_short,
                                                total_bytes: total_size,
                                            },
                                        );
                                        emit_progress_event(&app_handle_task, done_bytes, total_bytes, speed_bps);

                                        if !transfer_started {
                                            emit_active_connection_count(&app_handle_task, active_count);

                                            if !has_emitted_started_task.swap(true, Ordering::SeqCst) {
                                                emit_event(&app_handle_task, "transfer-started");
                                            }

                                            transfer_started = true;
                                            has_any_transfer_task.store(true, Ordering::SeqCst);
                                        }
                                    }
                                    iroh_blobs::provider::events::RequestUpdate::Progress(msg) => {
                                        if !transfer_started {
                                            let active_count = {
                                                let tracker = progress_tracker_task.lock().await;
                                                tracker.transfers.len()
                                            };

                                            emit_active_connection_count(&app_handle_task, active_count);

                                            if !has_emitted_started_task.swap(true, Ordering::SeqCst) {
                                                emit_event(&app_handle_task, "transfer-started");
                                            }
                                            transfer_started = true;
                                            has_any_transfer_task.store(true, Ordering::SeqCst);
                                        }

                                        let (payload, done_bytes, speed_bps) = {
                                            let mut tracker = progress_tracker_task.lock().await;
                                            if tracker.started_at.is_none() {
                                                tracker.started_at = Some(Instant::now());
                                            }
                                            let state = tracker
                                                .transfers
                                                .entry((connection_id, request_id))
                                                .or_insert(TransferState {
                                                    started: false,
                                                    count_toward_payload: false,
                                                    total_size: msg.end_offset.max(1),
                                                    last_offset: 0,
                                                });
                                            state.total_size = state.total_size.max(msg.end_offset).max(1);
                                            state.last_offset = state
                                                .last_offset
                                                .max(msg.end_offset.min(state.total_size));
                                            let payload = SenderRequestProgressPayload {
                                                connection_id,
                                                request_id,
                                                done_bytes: state.last_offset,
                                            };
                                            let (done_bytes, speed_bps) =
                                                snapshot_progress(&tracker, total_bytes);
                                            (payload, done_bytes, speed_bps)
                                        };

                                        emit_event_with_payload(
                                            &app_handle_task,
                                            "sender-request-progress",
                                            &payload,
                                        );
                                        emit_progress_event(&app_handle_task, done_bytes, total_bytes, speed_bps);
                                    }
                                    iroh_blobs::provider::events::RequestUpdate::Completed(_msg) => {
                                        if transfer_started && !request_completed {
                                            let (active_count, done_bytes, speed_bps) = {
                                                let mut tracker = progress_tracker_task.lock().await;
                                                if let Some(state) = tracker.transfers.remove(&(connection_id, request_id)) {
                                                    if state.count_toward_payload {
                                                        tracker.completed_bytes = tracker
                                                            .completed_bytes
                                                            .saturating_add(state.total_size);
                                                    }
                                                }
                                                let (done_bytes, speed_bps) =
                                                    snapshot_progress(&tracker, total_bytes);
                                                (tracker.transfers.len(), done_bytes, speed_bps)
                                            };

                                            emit_event_with_payload(
                                                &app_handle_task,
                                                "sender-request-completed",
                                                &SenderRequestCompletedPayload {
                                                    connection_id,
                                                    request_id,
                                                },
                                            );
                                            emit_progress_event(&app_handle_task, done_bytes, total_bytes, speed_bps);
                                            emit_active_connection_count(&app_handle_task, active_count);

                                            request_completed = true;

                                            let completed = completed_requests_task.fetch_add(1, Ordering::SeqCst) + 1;
                                            let active = active_requests_task.load(Ordering::SeqCst);

                                            let min_required = if entry_type_task == "directory" { 2 } else { 1 };

                                            if completed >= active
                                                && completed >= min_required
                                                && has_any_transfer_task.load(Ordering::SeqCst) {
                                                let active_before_wait = active;

                                                tokio::time::sleep(Duration::from_millis(500)).await;

                                                let completed_after = completed_requests_task.load(Ordering::SeqCst);
                                                let active_after = active_requests_task.load(Ordering::SeqCst);

                                                let new_requests_arrived = active_after > active_before_wait;

                                                let has_active_transfers = {
                                                    let tracker = progress_tracker_task.lock().await;
                                                    !tracker.transfers.is_empty()
                                                };

                                                let last_request_recent = {
                                                    let last_time = last_request_time_task.lock().await;
                                                    if let Some(time) = *last_time {
                                                        time.elapsed() < Duration::from_millis(500)
                                                    } else {
                                                        false
                                                    }
                                                };

                                                if completed_after >= active_after
                                                    && completed_after >= min_required
                                                    && !new_requests_arrived
                                                    && !has_active_transfers
                                                    && !last_request_recent
                                                {
                                                    emit_event(&app_handle_task, "transfer-completed");
                                                    has_emitted_started_task.store(false, Ordering::SeqCst);

                                                    active_requests_task.store(0, Ordering::SeqCst);
                                                    completed_requests_task.store(0, Ordering::SeqCst);
                                                    has_any_transfer_task.store(false, Ordering::SeqCst);
                                                }
                                            }
                                        }
                                    }
                                    iroh_blobs::provider::events::RequestUpdate::Aborted(_msg) => {
                                        tracing::warn!("Request aborted: conn {} req {}", connection_id, request_id);
                                        if transfer_started && !request_completed {
                                            let (active_count, done_bytes, speed_bps) = {
                                                let mut tracker = progress_tracker_task.lock().await;
                                                tracker.transfers.remove(&(connection_id, request_id));
                                                let (done_bytes, speed_bps) = snapshot_progress(&tracker, total_bytes);
                                                (tracker.transfers.len(), done_bytes, speed_bps)
                                            };
                                            emit_progress_event(&app_handle_task, done_bytes, total_bytes, speed_bps);
                                            emit_active_connection_count(&app_handle_task, active_count);

                                            request_completed = true;

                                            let completed = completed_requests_task.fetch_add(1, Ordering::SeqCst) + 1;
                                            let active = active_requests_task.load(Ordering::SeqCst);

                                            if completed >= active {
                                                emit_event(&app_handle_task, "transfer-failed");
                                                has_emitted_started_task.store(false, Ordering::SeqCst);

                                                active_requests_task.store(0, Ordering::SeqCst);
                                                completed_requests_task.store(0, Ordering::SeqCst);
                                                has_any_transfer_task.store(false, Ordering::SeqCst);
                                            }
                                        }
                                    }
                                }
                            }

                            if transfer_started && !request_completed {
                                let (done_bytes, speed_bps) = {
                                    let mut tracker = progress_tracker_task.lock().await;
                                    if let Some(state) = tracker.transfers.remove(&(connection_id, request_id)) {
                                        if state.count_toward_payload {
                                            tracker.completed_bytes = tracker
                                                .completed_bytes
                                                .saturating_add(state.total_size);
                                        }
                                    }
                                    snapshot_progress(&tracker, total_bytes)
                                };

                                emit_event_with_payload(
                                    &app_handle_task,
                                    "sender-request-completed",
                                    &SenderRequestCompletedPayload {
                                        connection_id,
                                        request_id,
                                    },
                                );
                                emit_progress_event(&app_handle_task, done_bytes, total_bytes, speed_bps);

                                let completed = completed_requests_task.fetch_add(1, Ordering::SeqCst) + 1;
                                let active = active_requests_task.load(Ordering::SeqCst);

                                let min_required = if entry_type_task == "directory" { 2 } else { 1 };

                                if completed >= active
                                    && completed >= min_required
                                    && has_any_transfer_task.load(Ordering::SeqCst) {
                                    let active_before_wait = active;

                                    tokio::time::sleep(Duration::from_millis(500)).await;

                                    let completed_after = completed_requests_task.load(Ordering::SeqCst);
                                    let active_after = active_requests_task.load(Ordering::SeqCst);

                                    let new_requests_arrived = active_after > active_before_wait;

                                    let has_active_transfers = {
                                        let tracker = progress_tracker_task.lock().await;
                                        !tracker.transfers.is_empty()
                                    };

                                    let last_request_recent = {
                                        let last_time = last_request_time_task.lock().await;
                                        if let Some(time) = *last_time {
                                            time.elapsed() < Duration::from_millis(500)
                                        } else {
                                            false
                                        }
                                    };

                                    if completed_after >= active_after
                                        && completed_after >= min_required
                                        && !new_requests_arrived
                                        && !has_active_transfers
                                        && !last_request_recent
                                    {
                                        emit_event(&app_handle_task, "transfer-completed");
                                    }
                                }
                            }
                        });
                    }
                    _ => {}
                }
            }
            Some(_) = tasks.next(), if !tasks.is_empty() => {}
        }
    }

    while tasks.next().await.is_some() {}

    if has_any_transfer.load(Ordering::SeqCst) {
        let completed = completed_requests.load(Ordering::SeqCst);
        let active = active_requests.load(Ordering::SeqCst);

        let min_required = if entry_type == "directory" { 2 } else { 1 };

        if completed >= active && completed >= min_required && completed > 0 {
            emit_event(&app_handle, "transfer-completed");
        }
    }

    Ok(())
}
