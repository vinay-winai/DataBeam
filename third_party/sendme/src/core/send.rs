use crate::core::types::{
    apply_options, get_or_create_secret, AddrInfoOptions, AppHandle, SendOptions, SendResult,
};
use super::sender_protocol::AcknowledgedBlobs;
use anyhow::Context;
use data_encoding::HEXLOWER;
use iroh::{address_lookup::pkarr::PkarrPublisher, endpoint::presets, Endpoint, RelayMode};
use iroh_blobs::{
    api::{
        blobs::{AddPathOptions, ImportMode},
        Store, TempTag,
    },
    format::collection::Collection,
    provider::events::{ConnectMode, EventMask, EventSender, RequestMode},
    store::fs::FsStore,
    ticket::BlobTicket,
    BlobFormat,
};
use n0_future::StreamExt;
use n0_future::{task::AbortOnDropHandle, BufferedStreamExt};
use rand::Rng;
use serde::Serialize;
use std::{
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
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
    let cancel = options.cancel.clone();
    check_cancelled(cancel.as_deref())?;
    let secret_key = get_or_create_secret()?;

    let relay_mode: RelayMode = options.relay_mode.clone().into();

    let mut builder = Endpoint::builder(presets::N0)
        .alpns(vec![iroh_blobs::protocol::ALPN.to_vec()])
        .secret_key(secret_key)
        .relay_mode(relay_mode.clone());

    if options.ticket_type == AddrInfoOptions::Id {
        builder = builder.address_lookup(PkarrPublisher::n0_dns());
    }
    if let Some(addr) = options.magic_ipv4_addr {
        builder = builder.bind_addr(addr)?;
    }
    if let Some(addr) = options.magic_ipv6_addr {
        builder = builder.bind_addr(addr)?;
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
    let paths2 = paths.clone();
    let payload_root2 = payload_root.clone();

    // Retain cleanup handles outside the setup future. Cancelling drops its
    // import streams first, then closes their workers before removing files.
    let mut setup_endpoint = None;
    let mut setup_store = None;
    let mut setup_router = None;
    let setup_result = async {
        let t0 = Instant::now();
        // Finish initialization before observing cancellation so an abandoned
        // filesystem task cannot create a store after its directory was removed.
        tokio::fs::create_dir_all(&blobs_data_dir2).await?;
        check_cancelled(cancel.as_deref())?;

        let endpoint = builder.bind().await?;
        setup_endpoint = Some(endpoint.clone());
        check_cancelled(cancel.as_deref())?;

        let store = FsStore::load(&blobs_data_dir2).await?;
        setup_store = Some(store.clone());
        check_cancelled(cancel.as_deref())?;

        let setup = async {
            let blobs = AcknowledgedBlobs::new(
                store.as_ref().clone(),
                EventSender::new(
                    progress_tx,
                    EventMask {
                        connected: ConnectMode::Notify,
                        get: RequestMode::NotifyLog,
                        ..EventMask::DEFAULT
                    },
                ),
            );

            let import_result = import(
                paths2,
                payload_root2,
                blobs.store(),
                options.jobs,
                cancel.as_deref(),
                &app_handle,
            )
            .await?;
            let dt = t0.elapsed();

            let (ref temp_tag, size, ref _collection, ref sizes) = import_result;
            let progress_handle =
                AbortOnDropHandle::new(n0_future::task::spawn(show_provide_progress_with_logging(
                    progress_rx,
                    app_handle_clone,
                    size,
                    temp_tag.hash(),
                    sizes.clone(),
                )));

            let router = iroh::protocol::Router::builder(endpoint)
                .accept(iroh_blobs::ALPN, blobs.clone())
                .spawn();
            setup_router = Some(router.clone());

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

        select! {
            biased;
            _ = wait_for_cancel(cancel.as_deref()) => Err(anyhow::anyhow!("Operation cancelled")),
            _ = tokio::signal::ctrl_c() => Err(anyhow::anyhow!("Operation cancelled")),
            x = setup => x,
        }
    }
    .await;
    let (
        router,
        (temp_tag, size, _collection, _sizes),
        _dt,
        _blobs_data_dir,
        store,
        progress_handle,
    ) = match setup_result {
        Ok(v) => v,
        Err(e) => {
            if let Some(router) = setup_router {
                let _ = router.shutdown().await;
                // AcknowledgedBlobs::shutdown closes the store with the router.
            } else {
                if let Some(endpoint) = setup_endpoint {
                    endpoint.close().await;
                }
                if let Some(store) = setup_store {
                    if let Err(error) = store.shutdown().await {
                        tracing::warn!("Failed to close cancelled Sendme store: {error:#}");
                    }
                }
            }
            if let Err(error) = tokio::fs::remove_dir_all(&blobs_data_dir).await {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(e.context(format!("remove Sendme preparation cache: {error}")));
                }
            }
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
        _progress_handle: progress_handle,
        _store: store,
    })
}

fn check_cancelled(cancel: Option<&AtomicBool>) -> anyhow::Result<()> {
    anyhow::ensure!(
        !cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)),
        "Operation cancelled"
    );
    Ok(())
}

async fn wait_for_cancel(cancel: Option<&AtomicBool>) {
    let Some(cancel) = cancel else {
        return std::future::pending().await;
    };
    while !cancel.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Hashing progress across parallel imports, separate from network progress.
struct PreparationProgress {
    done: u64,
    total: u64,
    rate: super::live_rate::LiveRate,
    last_emit: Option<Instant>,
}

impl PreparationProgress {
    fn emit(&mut self, app_handle: &AppHandle, force: bool) {
        let now = Instant::now();
        if !force
            && self
                .last_emit
                .is_some_and(|last| now.duration_since(last) < Duration::from_millis(100))
        {
            return;
        }
        self.last_emit = Some(now);
        if let Some(handle) = app_handle {
            let speed = self.rate.observe(self.done, now);
            let payload = format!("{}:{}:{}", self.done, self.total, (speed * 1000.0) as i64);
            let _ = handle.emit_event_with_payload("preparation-progress", &payload);
        }
    }

    fn advance(&mut self, offset: u64, previous: &mut u64, item_size: u64, app_handle: &AppHandle) {
        let offset = offset.min(item_size).max(*previous);
        self.done = self.done.saturating_add(offset - *previous).min(self.total);
        *previous = offset;
        self.emit(app_handle, false);
    }
}

async fn import(
    paths: Vec<PathBuf>,
    payload_root: String,
    db: &Store,
    jobs: Option<usize>,
    cancel: Option<&AtomicBool>,
    app_handle: &AppHandle,
) -> anyhow::Result<(TempTag, u64, Collection, Vec<u64>)> {
    let data_sources = build_virtual_data_sources(paths, &payload_root, cancel)?;
    anyhow::ensure!(!data_sources.is_empty(), "no valid files to share");
    let mut sized_sources = Vec::with_capacity(data_sources.len());
    let mut total = 0_u64;
    for (name, path) in data_sources {
        check_cancelled(cancel)?;
        let size = std::fs::metadata(&path)?.len();
        total = total.saturating_add(size);
        sized_sources.push((name, path, size));
    }
    let progress = std::sync::Arc::new(std::sync::Mutex::new(PreparationProgress {
        done: 0,
        total,
        rate: super::live_rate::LiveRate::new(0, Instant::now()),
        last_emit: None,
    }));
    progress.lock().unwrap().emit(app_handle, true);

    let parallelism = jobs.unwrap_or_else(num_cpus::get).max(1);

    let mut names_and_tags = n0_future::stream::iter(sized_sources)
        .map(|(name, path, expected_size)| {
            let db = db.clone();
            let progress = progress.clone();
            async move {
                check_cancelled(cancel)?;
                let import = db.add_path_with_opts(AddPathOptions {
                    path,
                    mode: ImportMode::TryReference,
                    format: iroh_blobs::BlobFormat::Raw,
                });
                let mut stream = import.stream().await;
                let mut item_size = 0;
                let mut item_done = 0;
                let mut reported_import = false;
                let temp_tag = loop {
                    let item = stream
                        .next()
                        .await
                        .context("import stream ended without a tag")?;
                    trace!("importing {name} {item:?}");
                    match item {
                        iroh_blobs::api::blobs::AddProgressItem::Size(size) => {
                            item_size = size;
                            let mut progress = progress.lock().unwrap();
                            progress.total = progress
                                .total
                                .saturating_sub(expected_size)
                                .saturating_add(size);
                        }
                        iroh_blobs::api::blobs::AddProgressItem::CopyProgress(_) => {}
                        iroh_blobs::api::blobs::AddProgressItem::CopyDone => {}
                        iroh_blobs::api::blobs::AddProgressItem::OutboardProgress(offset) => {
                            progress.lock().unwrap().advance(
                                offset,
                                &mut item_done,
                                item_size,
                                app_handle,
                            );
                            // One event per file, after hashing has actually begun.
                            if offset > 0 && !reported_import {
                                emit_event_with_payload(
                                    app_handle,
                                    "sendme-import-progress",
                                    &offset,
                                );
                                reported_import = true;
                            }
                        }
                        iroh_blobs::api::blobs::AddProgressItem::Error(cause) => {
                            anyhow::bail!("error importing {}: {}", name, cause);
                        }
                        iroh_blobs::api::blobs::AddProgressItem::Done(tt) => {
                            progress.lock().unwrap().advance(
                                item_size,
                                &mut item_done,
                                item_size,
                                app_handle,
                            );
                            break tt;
                        }
                    }
                    check_cancelled(cancel)?;
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
    let sizes = names_and_tags.iter().map(|(_, _, size)| *size).collect();
    check_cancelled(cancel)?;
    let (collection, tags) = names_and_tags
        .into_iter()
        .map(|(name, tag, _)| ((name, tag.hash()), tag))
        .unzip::<_, _, Collection, Vec<_>>();
    let temp_tag = collection.clone().store(db).await?;
    drop(tags);
    progress.lock().unwrap().emit(app_handle, true);
    Ok((temp_tag, size, collection, sizes))
}

fn build_virtual_data_sources(
    paths: Vec<PathBuf>,
    payload_root: &str,
    cancel: Option<&AtomicBool>,
) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut data_sources = Vec::new();

    for source in paths {
        check_cancelled(cancel)?;
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
            check_cancelled(cancel)?;
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
    use std::sync::{atomic::AtomicU64, Arc};

    #[tokio::test]
    async fn aborted_size_probe_is_not_a_payload_failure() -> anyhow::Result<()> {
        use iroh_blobs::{protocol::{GetRequest, ChunkRangesSeq}, provider::events::{Notify, ProviderProto, RequestReceived, RequestUpdate, TransferStarted, TransferAborted}};
        #[derive(Default)]
        struct Events(std::sync::Mutex<Vec<String>>);
        impl crate::core::types::EventEmitter for Events {
            fn emit_event(&self, name: &str) -> Result<(), String> {
                self.0.lock().unwrap().push(name.into()); Ok(())
            }
            fn emit_event_with_payload(&self, _: &str, _: &str) -> Result<(), String> { Ok(()) }
        }
        for probe in [true, false] {
            let hash = iroh_blobs::Hash::new(b"payload");
            let events = Arc::new(Events::default());
            let (tx, rx) = tokio::sync::mpsc::channel(16);
            let task = tokio::spawn(show_provide_progress_with_logging(rx, Some(events.clone()), 4096, hash, vec![4096]));
            // Notify's constructor is private; its public serde representation
            // lets the test exercise the real provider-event stream.
            let request: Notify<RequestReceived<GetRequest>> = serde_json::from_value(serde_json::to_value(RequestReceived {
                connection_id: 1, request_id: 1,
                request: GetRequest { hash, ranges: if probe { ChunkRangesSeq::verified_child_sizes() } else { ChunkRangesSeq::all() } },
            })?)?;
            let (updates, requests) = irpc::channel::mpsc::channel(16);
            let request: irpc::WithChannels<_, ProviderProto> = (request, irpc::channel::none::NoSender, requests).into();
            tx.send(request.into()).await?;
            // Size probes include a child index, but that does not make them
            // payload requests. A deliberate early reset must leave sharing alive.
            updates.send(RequestUpdate::Started(TransferStarted { index: 2, hash, size: 4096 })).await?;
            updates.send(RequestUpdate::Aborted(TransferAborted { stats: Box::new(iroh_blobs::provider::TransferStats {
                payload_bytes_sent: 1024, other_bytes_sent: 8, other_bytes_read: 0, duration: Duration::ZERO,
            }) })).await?;
            drop(updates);
            drop(tx);
            tokio::time::timeout(Duration::from_secs(5), task).await???;
            let events = events.0.lock().unwrap();
            assert_eq!(events.iter().filter(|name| *name == "transfer-failed").count(), usize::from(!probe));
            assert!(!events.iter().any(|name| name == "transfer-completed"));
        }
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn preparation_progress_counts_parallel_files_once() -> anyhow::Result<()> {
        #[derive(Default)]
        struct Events(std::sync::Mutex<Vec<String>>);
        impl crate::core::types::EventEmitter for Events {
            fn emit_event(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn emit_event_with_payload(&self, event: &str, payload: &str) -> Result<(), String> {
                if event == "preparation-progress" {
                    self.0.lock().unwrap().push(payload.into());
                }
                Ok(())
            }
        }
        let directory = tempfile::tempdir()?;
        let mut paths = Vec::new();
        let mut total = 0;
        for (name, size) in [
            ("a.bin", 8 * 1024 * 1024),
            ("b.bin", 4 * 1024 * 1024),
            ("empty", 0),
        ] {
            let path = directory.path().join(name);
            std::fs::File::create(&path)?.set_len(size)?;
            paths.push(path);
            total += size;
        }
        let events = Arc::new(Events::default());
        let share = start_share(
            paths,
            "payload".into(),
            SendOptions {
                relay_mode: crate::core::types::RelayModeOption::Disabled,
                blob_dir: Some(directory.path().join("cache")),
                jobs: Some(2),
                ..Default::default()
            },
            Some(events.clone()),
        )
        .await?;
        let events = events.0.lock().unwrap().clone();
        let mut previous = 0;
        for (index, event) in events.iter().enumerate() {
            let values: Vec<u64> = event.split(':').map(|n| n.parse().unwrap()).collect();
            assert_eq!(values[1], total);
            assert!(values[0] >= previous && values[0] <= total);
            if index == 0 {
                assert_eq!(values[0], 0);
            }
            previous = values[0];
        }
        assert!(events.len() >= 2);
        assert_eq!(previous, total);
        assert_eq!(share.size, total);
        share.router.shutdown().await?;
        let cache = share.blobs_data_dir.clone();
        drop(share);
        std::fs::remove_dir_all(cache)?;
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_preparation_does_not_create_cache() {
        let directory = tempfile::tempdir().unwrap();
        let cache = directory.path().join("cache");
        let result = start_share(
            vec![directory.path().join("does-not-exist")],
            "payload".into(),
            SendOptions {
                cancel: Some(Arc::new(AtomicBool::new(true))),
                blob_dir: Some(cache.clone()),
                ..Default::default()
            },
            None,
        )
        .await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("cancelled share succeeded"),
        };
        assert!(error.to_string().contains("Operation cancelled"));
        assert!(!cache.exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_preparation_stops_import_and_releases_resources() -> anyhow::Result<()> {
        struct CancelAfterRead {
            cancel: Arc<AtomicBool>,
            offset: AtomicU64,
        }
        impl crate::core::types::EventEmitter for CancelAfterRead {
            fn emit_event(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn emit_event_with_payload(&self, event: &str, payload: &str) -> Result<(), String> {
                if event == "sendme-import-progress" {
                    self.offset
                        .store(payload.parse().unwrap(), Ordering::SeqCst);
                    self.cancel.store(true, Ordering::Relaxed);
                }
                Ok(())
            }
        }

        let directory = tempfile::tempdir()?;
        let source = directory.path().join("large.bin");
        const SIZE: u64 = 64 * 1024 * 1024;
        std::fs::File::create(&source)?.set_len(SIZE)?;
        let cache = directory.path().join("cache");
        let other_share = cache.join(".sendme-send-other");
        std::fs::create_dir_all(&other_share)?;
        std::fs::write(other_share.join("keep.txt"), b"other active share")?;
        let socket = std::net::UdpSocket::bind("127.0.0.1:0")?;
        let bind_addr = socket.local_addr()?;
        drop(socket);
        let cancel = Arc::new(AtomicBool::new(false));
        let events = Arc::new(CancelAfterRead {
            cancel: cancel.clone(),
            offset: AtomicU64::new(0),
        });
        let options = || SendOptions {
            relay_mode: crate::core::types::RelayModeOption::Disabled,
            magic_ipv4_addr: Some(match bind_addr {
                std::net::SocketAddr::V4(a) => a,
                _ => unreachable!(),
            }),
            blob_dir: Some(cache.clone()),
            jobs: Some(1),
            ..Default::default()
        };
        let mut first_options = options();
        first_options.cancel = Some(cancel);
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            start_share(
                vec![source.clone()],
                "payload".into(),
                first_options,
                Some(events.clone()),
            ),
        )
        .await?;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("cancelled import succeeded"),
        };
        assert!(error.to_string().contains("Operation cancelled"));
        let offset = events.offset.load(Ordering::SeqCst);
        assert!(
            offset > 0 && offset < SIZE,
            "must cancel after reading starts, before EOF"
        );
        let remaining: Vec<_> = std::fs::read_dir(&cache)?
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(
            remaining,
            vec![other_share.clone()],
            "only this share's cache should be removed"
        );
        assert_eq!(
            std::fs::read(other_share.join("keep.txt"))?,
            b"other active share"
        );

        // Windows refuses this open if the cancelled reader still owns the file.
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            drop(
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .share_mode(0)
                    .open(&source)?,
            );
        }
        std::fs::write(&source, b"a fresh share after cancellation")?;
        let share = tokio::time::timeout(
            Duration::from_secs(5),
            start_share(vec![source], "payload".into(), options(), None),
        )
        .await??;
        assert!(
            !share.ticket.is_empty(),
            "fresh share should reuse the released UDP port"
        );
        share.router.shutdown().await?;
        let share_dir = share.blobs_data_dir.clone();
        drop(share);
        std::fs::remove_dir_all(share_dir)?;
        Ok(())
    }

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
    collection_hash: iroh_blobs::Hash,
    file_sizes: Vec<u64>,
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
        item_index: u64,
    }

    struct ProgressTracker {
        transfers: std::collections::HashMap<(u64, u64), TransferState>,
        completed_bytes: u64,
        started_at: Option<Instant>,
        resume: super::sender_progress::SenderProgress,
    }

    fn snapshot_progress(tracker: &ProgressTracker, total_bytes: u64) -> (u64, f64) {
        if let Some((done, speed)) = tracker.resume.snapshot() {
            return (done.min(total_bytes), speed);
        }
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

    let progress_tracker = Arc::new(Mutex::new(ProgressTracker {
        transfers: Default::default(),
        completed_bytes: 0,
        started_at: None,
        resume: super::sender_progress::SenderProgress::new(file_sizes),
    }));
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
                        progress_tracker.lock().await.resume.close(msg.connection_id);
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
                        // Size discovery reads the root and only the last chunk
                        // of each child. Request IDs do not identify payloads:
                        // retries and other peers can issue more size probes.
                        let is_size_probe = msg.request.ranges
                            == iroh_blobs::protocol::ChunkRangesSeq::verified_child_sizes();
                        if !is_size_probe && msg.request.hash == collection_hash {
                            progress_tracker.lock().await.resume.begin(connection_id, request_id, &msg.request.ranges);
                        }

                        let mut rx = msg.rx;
                        tasks.push(async move {
                            let mut transfer_started = false;
                            let mut request_completed = false;
                            let mut payload_started = false;

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
                                        // Index 0 is the hash sequence and index 1
                                        // is collection metadata, not file data.
                                        let count_toward_payload = !is_size_probe && msg.index > 1;
                                        payload_started |= count_toward_payload;

                                        let (active_count, done_bytes, speed_bps) = {
                                            let mut tracker = progress_tracker_task.lock().await;
                                            if tracker.started_at.is_none() {
                                                tracker.started_at = Some(Instant::now());
                                            }
                                            let key = (connection_id, request_id);
                                            match tracker.transfers.remove(&key) {
                                                Some(state) if state.started => {
                                                    if state.count_toward_payload {
                                                        tracker.resume.complete_blob(connection_id, request_id, state.item_index);
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
                                                            item_index: msg.index,
                                                        },
                                                    );
                                                }
                                                Some(mut state) => {
                                                    state.started = true;
                                                    state.count_toward_payload = count_toward_payload;
                                                    state.item_index = msg.index;
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
                                                            item_index: msg.index,
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
                                                    item_index: 0,
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
                                            let index = state.item_index;
                                            let offset = state.last_offset;
                                            tracker.resume.advance(connection_id, request_id, index, offset);
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
                                                        tracker.resume.complete_blob(connection_id, request_id, state.item_index);
                                                        tracker.completed_bytes = tracker
                                                            .completed_bytes
                                                            .saturating_add(state.total_size);
                                                    }
                                                }
                                                tracker.resume.finish_request(connection_id, request_id);
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

                                            if payload_started
                                                && completed >= active
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
                                                tracker.resume.finish_request(connection_id, request_id);
                                                let (done_bytes, speed_bps) = snapshot_progress(&tracker, total_bytes);
                                                (tracker.transfers.len(), done_bytes, speed_bps)
                                            };
                                            emit_progress_event(&app_handle_task, done_bytes, total_bytes, speed_bps);
                                            emit_active_connection_count(&app_handle_task, active_count);

                                            request_completed = true;

                                            let completed = completed_requests_task.fetch_add(1, Ordering::SeqCst) + 1;
                                            let active = active_requests_task.load(Ordering::SeqCst);

                                            // Size discovery deliberately stops reading once
                                            // it has the sizes. Its reset is not payload loss.
                                            if payload_started && completed >= active {
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
                                    tracker.transfers.remove(&(connection_id, request_id));
                                    tracker.resume.finish_request(connection_id, request_id);
                                    snapshot_progress(&tracker, total_bytes)
                                };

                                emit_progress_event(&app_handle_task, done_bytes, total_bytes, speed_bps);

                                completed_requests_task.fetch_add(1, Ordering::SeqCst);
                                // Losing the update stream is not a successful
                                // payload completion, regardless of byte counts.
                                if payload_started {
                                    emit_event(&app_handle_task, "transfer-failed");
                                }
                            }
                            // Also release display state when a request failed
                            // before its first Started update.
                            progress_tracker_task.lock().await.resume.finish_request(connection_id, request_id);
                        });
                    }
                    _ => {}
                }
            }
            Some(_) = tasks.next(), if !tasks.is_empty() => {}
        }
    }

    while tasks.next().await.is_some() {}

    Ok(())
}
