use crate::core::types::{get_or_create_secret, AppHandle, ReceiveOptions, ReceiveResult};
use iroh::{discovery::dns::DnsDiscovery, Endpoint};
use iroh_blobs::{
    api::{
        blobs::{ExportMode, ExportOptions, ExportProgressItem},
        remote::GetProgressItem,
        Store,
    },
    format::collection::Collection,
    get::{request::get_hash_seq_and_sizes, GetError, Stats},
    store::fs::FsStore,
    ticket::BlobTicket,
};
use n0_future::StreamExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::select;

// Helper function to emit events through the app handle
fn emit_event(app_handle: &AppHandle, event_name: &str) {
    if let Some(handle) = app_handle {
        if let Err(e) = handle.emit_event(event_name) {
            tracing::warn!("Failed to emit event {}: {}", event_name, e);
        }
    }
}

// Helper function to emit progress events with payload
fn emit_progress_event(
    app_handle: &AppHandle,
    bytes_transferred: u64,
    total_bytes: u64,
    speed_bps: f64,
) {
    if let Some(handle) = app_handle {
        let event_name = "receive-progress";

        // Convert speed to integer (multiply by 1000 to preserve 3 decimal places)
        let speed_int = (speed_bps * 1000.0) as i64;

        // Create payload data as colon-separated string
        let payload = format!("{}:{}:{}", bytes_transferred, total_bytes, speed_int);

        // Emit the event with appropriate payload
        if let Err(e) = handle.emit_event_with_payload(event_name, &payload) {
            tracing::warn!("Failed to emit progress event: {}", e);
        }
    }
}

// Helper function to emit events with payload
fn emit_event_with_payload(app_handle: &AppHandle, event_name: &str, payload: &str) {
    if let Some(handle) = app_handle {
        if let Err(e) = handle.emit_event_with_payload(event_name, payload) {
            tracing::warn!("Failed to emit event {} with payload: {}", event_name, e);
        }
    }
}

pub async fn download(
    ticket_str: String,
    options: ReceiveOptions,
    app_handle: AppHandle,
) -> anyhow::Result<ReceiveResult> {
    let ticket = BlobTicket::from_str(&ticket_str)?;

    let addr = ticket.addr().clone();

    let secret_key = get_or_create_secret()?;

    let mut builder = Endpoint::builder()
        .alpns(vec![])
        .secret_key(secret_key)
        .relay_mode(options.relay_mode.clone().into());

    if ticket.addr().relay_urls().count() == 0 && ticket.addr().ip_addrs().count() == 0 {
        builder = builder.discovery(DnsDiscovery::n0_dns());
    }
    if let Some(addr) = options.magic_ipv4_addr {
        builder = builder.bind_addr_v4(addr);
    }
    if let Some(addr) = options.magic_ipv6_addr {
        builder = builder.bind_addr_v6(addr);
    }

    let endpoint = builder.bind().await?;

    // Stable blob store directory name based on ticket hash.
    // Uses user-configured blob_dir if set, otherwise system temp dir.
    let dir_name = format!(".sendme-recv-{}", ticket.hash().to_hex());
    let blob_base = {
        let base = options.blob_dir.clone().unwrap_or_else(std::env::temp_dir);
        let _ = std::fs::create_dir_all(&base);
        base
    };

    // Clean up OTHER .sendme-recv-* dirs, preserving the current one.
    let dir_name_clone = dir_name.clone();
    let blob_base_clone = blob_base.clone();
    tokio::spawn(async move {
        if let Ok(mut entries) = tokio::fs::read_dir(&blob_base_clone).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(".sendme-recv-") && name != dir_name_clone.as_str() {
                        // Ignore errors; directories currently in use by parallel instances
                        // will gracefully fail to delete due to OS-level file locks.
                        let _ = tokio::fs::remove_dir_all(&path).await;
                    }
                }
            }
        }
    });
    let _iroh_data_dir = blob_base.join(&dir_name);
    let db = FsStore::load(&_iroh_data_dir).await?;
    let db2 = db.clone();

    let fut = async move {
        let hash_and_format = ticket.hash_and_format();
        let local = db.remote().local(hash_and_format).await?;

        let (stats, total_files, payload_size) = if !local.is_complete() {
            // Emit receive-started event
            emit_event(&app_handle, "receive-started");

            let connection = match endpoint
                .connect(addr.clone(), iroh_blobs::protocol::ALPN)
                .await
            {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::error!("Connection failed: {}", e);
                    tracing::error!("Error details: {:?}", e);
                    tracing::error!("Tried to connect to node: {}", addr.id);
                    tracing::error!("With relay: {:?}", addr.relay_urls().collect::<Vec<_>>());
                    tracing::error!(
                        "With direct addrs: {:?}",
                        addr.ip_addrs().collect::<Vec<_>>()
                    );
                    return Err(anyhow::anyhow!("Connection failed: {}", e));
                }
            };

            let sizes_result =
                get_hash_seq_and_sizes(&connection, &hash_and_format.hash, 1024 * 1024 * 32, None)
                    .await;

            let (_hash_seq, sizes) = match sizes_result {
                Ok((hash_seq, sizes)) => (hash_seq, sizes),
                Err(e) => {
                    tracing::error!("Failed to get sizes: {:?}", e);
                    tracing::error!("Error type: {}", std::any::type_name_of_val(&e));
                    return Err(show_get_error(e).into());
                }
            };
            let _total_size = sizes.iter().copied().sum::<u64>();
            // For payload size, we want the actual file data size
            // The sizes array contains: [collection_size, file1_size, file2_size, ...]
            // We skip the first element (collection metadata) but include all file sizes
            let payload_size = sizes.iter().skip(1).copied().sum::<u64>();
            let total_files = (sizes.len().saturating_sub(1)) as u64;

            // Calculate how much we already successfully downloaded in previous attempts
            let local_size = local.local_bytes();

            // Emit initial progress event right away with our resumed progress
            emit_progress_event(&app_handle, local_size.min(payload_size), payload_size, 0.0);

            let get = db.remote().execute_get(connection, local.missing());
            let mut stats = Stats::default();
            let mut stream = get.stream();
            let mut last_log_offset = 0u64;
            let transfer_start_time = Instant::now();
            let mut download_completed = false;

            while let Some(item) = stream.next().await {
                match item {
                    GetProgressItem::Progress(offset) => {
                        // Emit progress events every 1MB
                        if offset - last_log_offset > 1_000_000 {
                            last_log_offset = offset;

                            // Calculate speed and emit progress event
                            let elapsed = transfer_start_time.elapsed().as_secs_f64();
                            let speed_bps = if elapsed > 0.0 {
                                offset as f64 / elapsed
                            } else {
                                0.0
                            };

                            emit_progress_event(
                                &app_handle,
                                (local_size + offset).min(payload_size),
                                payload_size,
                                speed_bps,
                            );
                        }
                    }
                    GetProgressItem::Done(value) => {
                        stats = value;
                        download_completed = true;

                        // Emit final progress event
                        let elapsed = transfer_start_time.elapsed().as_secs_f64();
                        let speed_bps = if elapsed > 0.0 {
                            payload_size as f64 / elapsed
                        } else {
                            0.0
                        };
                        emit_progress_event(&app_handle, payload_size, payload_size, speed_bps);

                        break;
                    }
                    GetProgressItem::Error(cause) => {
                        tracing::error!("Download error: {:?}", cause);
                        anyhow::bail!(show_get_error(cause));
                    }
                }
            }

            if !download_completed {
                anyhow::bail!(
                    "Download stream ended before completion - sender may have disconnected"
                );
            }

            (stats, total_files, payload_size)
        } else {
            let total_files = local.children().unwrap() - 1;
            let payload_bytes = 0; // todo local.sizes().skip(2).map(Option::unwrap).sum::<u64>();

            // Emit events for already complete data
            emit_event(&app_handle, "receive-started");

            (Stats::default(), total_files, payload_bytes)
        };

        let collection = Collection::load(hash_and_format.hash, db.as_ref()).await?;
        
        // If we skipped download (blobs were already complete), calculate the real payload size now
        let mut final_payload_size = payload_size;
        if payload_size == 0 && !collection.is_empty() {
             use iroh_blobs::api::blobs::BlobStatus;
             let mut sum = 0;
             for (_name, hash) in collection.iter() {
                 if let Ok(BlobStatus::Complete { size }) = db.status(*hash).await {
                     sum += size;
                 }
             }
             final_payload_size = sum;
        }

        if payload_size == 0 && final_payload_size > 0 {
             // We are already at 100% for the download phase
             emit_progress_event(&app_handle, final_payload_size, final_payload_size, 0.0);
        }

        // Extract file names from collection and emit them BEFORE export
        // This allows the UI to show file names during the export phase
        let mut file_names: Vec<String> = Vec::new();
        for (name, _hash) in collection.iter() {
            file_names.push(name.to_string());
        }

        // Emit file names information
        if !file_names.is_empty() {
            let file_names_json =
                serde_json::to_string(&file_names).unwrap_or_else(|_| "[]".to_string());
            emit_event_with_payload(&app_handle, "receive-file-names", &file_names_json);
        }

        // Determine output directory
        let output_dir_base = options.output_dir.clone().unwrap_or_else(|| {
            dirs::download_dir().unwrap_or_else(|| std::env::current_dir().unwrap())
        });

        // 1. Calculate Content Hash (Collection Hash)
        // We use the ticket's data hash as the unique signature for this specific payload.
        let content_hash = ticket.hash().to_hex().to_string();

        // 2. Resolve destination with smart incrementing logic
        let mut final_output_dir = output_dir_base.to_path_buf();
        if !collection.is_empty() {
            let root_name = collection.iter().next().unwrap().0.clone();
            let dir_component = root_name.split('/').next().unwrap_or(&root_name);
            
            let mut attempt = 1;
            loop {
                let name = if attempt == 1 {
                    dir_component.to_string()
                } else {
                    format!("{}_{}", attempt - 1, dir_component)
                };
                
                let check_path = output_dir_base.join(&name);
                if !check_path.exists() {
                    final_output_dir = check_path;
                    break;
                }
                
                // If it exists, check for our databeam_hash.txt signature
                let signature_path = check_path.join("databeam_hash.txt");
                if let Ok(existing_hash) = std::fs::read_to_string(&signature_path) {
                    if existing_hash.trim() == content_hash {
                        // MATCH! We can safely overwrite this version.
                        final_output_dir = check_path;
                        let _ = tokio::fs::remove_dir_all(&final_output_dir).await;
                        break;
                    }
                }
                
                // No match or no signature, try next increment
                attempt += 1;
                if attempt > 100 { anyhow::bail!("Too many conflicting versions at destination"); }
            }
        }

        emit_event(&app_handle, "receive-export-started");
        export(&db, collection, &final_output_dir, &app_handle, &content_hash).await?;

        // Remove the smart-overwrite signature file now that export is finished
        let signature_path = final_output_dir.join("databeam_hash.txt");
        let _ = tokio::fs::remove_file(&signature_path).await;

        // Emit completion event AFTER everything is done
        emit_event(&app_handle, "receive-completed");

        anyhow::Ok((total_files, payload_size, stats, final_output_dir))
    };

    let (total_files, payload_size, _stats, output_dir) = select! {
        x = fut => match x {
            Ok(x) => x,
            Err(e) => {
                tracing::error!("Download operation failed: {}", e);
                // make sure we shutdown the db before exiting
                db2.shutdown().await?;
                // WE DO NOT DELETE the cache here, so it can be resumed later!
                anyhow::bail!("error: {e}");
            }
        },
        _ = tokio::signal::ctrl_c() => {
            tracing::warn!("Operation cancelled by user");
            db2.shutdown().await?;
            // WE DO NOT DELETE the cache here, so it can be resumed later!
            anyhow::bail!("Operation cancelled");
        }
    };

    Ok(ReceiveResult {
        message: format!("Downloaded {} files, {} bytes", total_files, payload_size),
        file_path: output_dir,
    })
}

async fn export(
    db: &Store,
    collection: Collection,
    output_dir: &Path,
    app_handle: &AppHandle,
    content_hash: &str,
) -> anyhow::Result<()> {
    // Ensure the output directory exists
    tokio::fs::create_dir_all(output_dir).await?;

    // Write the signature hash BEFORE starting the export loop
    // This serves as the 'Smart Overwrite' fingerprint
    let signature_path = output_dir.join("databeam_hash.txt");
    if let Err(e) = tokio::fs::write(&signature_path, content_hash.as_bytes()).await {
        tracing::error!("Failed to write smart-overwrite signature to {}: {}", signature_path.display(), e);
    } else {
        tracing::info!("Wrote smart-overwrite signature to {}", signature_path.display());
    }

    for (_i, (name, hash)) in collection.iter().enumerate() {
        // The sender always wraps items in a 'databeam_' folder.
        // We strip this first component to allow remapping into our resolved 'final_output_dir'.
        let mut parts = name.split('/');
        let _wrapper = parts.next(); // Skip the original 'databeam_XXX' name
        
        let mut target = output_dir.to_path_buf();
        let mut has_subcomponents = false;
        for part in parts {
            validate_path_component(part)?;
            target.push(part);
            has_subcomponents = true;
        }

        // Fallback: If for some reason there were no subcomponents, use the raw name joined to parent
        if !has_subcomponents {
             target = get_export_path(output_dir.parent().unwrap_or(output_dir), name)?;
        }

        #[cfg(windows)]
        {
            target = to_windows_extended_path(target);
        }
        let mut last_error: Option<String> = None;
        const MAX_ATTEMPTS: u32 = 5;
        for attempt in 1..=MAX_ATTEMPTS {
            if let Some(parent) = target.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let mut stream = db
                .export_with_opts(ExportOptions {
                    hash: *hash,
                    target: target.clone(),
                    mode: ExportMode::Copy,
                })
                .stream()
                .await;

            let mut current_size = 0u64;
            let mut last_log_offset = 0u64;
            let mut failed: Option<String> = None;

            while let Some(item) = stream.next().await {
                match item {
                    ExportProgressItem::Size(size) => {
                        current_size = size;
                    }
                    ExportProgressItem::CopyProgress(offset) => {
                        if offset - last_log_offset > 1_000_000 {
                            last_log_offset = offset;
                            let payload = format!("{}:{}", offset, current_size);
                            emit_event_with_payload(
                                app_handle,
                                "receive-export-progress",
                                &payload,
                            );
                        }
                    }
                    ExportProgressItem::Done => {
                        if current_size > 0 {
                            let payload = format!("{}:{}", current_size, current_size);
                            emit_event_with_payload(
                                app_handle,
                                "receive-export-progress",
                                &payload,
                            );
                        }
                    }
                    ExportProgressItem::Error(cause) => {
                        failed = Some(cause.to_string());
                        break;
                    }
                }
            }

            if let Some(err) = failed {
                let retryable = attempt < MAX_ATTEMPTS
                    && (err.to_lowercase().contains("os error 2")
                        || err.to_lowercase().contains("os error 3")
                        || err.to_lowercase().contains("os error 32"));
                last_error = Some(err.clone());
                if retryable {
                    let _ = tokio::fs::remove_file(&target).await;
                    // Exponential backoff: 250ms, 500ms, 750ms, 1000ms
                    tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                    continue;
                }
                anyhow::bail!("error exporting {}: {}", name, err);
            }

            last_error = None;
            break;
        }

        if let Some(err) = last_error {
            anyhow::bail!("error exporting {}: {}", name, err);
        }
    }

    Ok(())
}

fn get_export_path(root: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let parts = name.split('/');
    let mut path = root.to_path_buf();
    for part in parts {
        validate_path_component(part)?;
        path.push(part);
    }
    #[cfg(windows)]
    {
        path = to_windows_extended_path(path);
    }
    Ok(path)
}

#[cfg(windows)]
fn to_windows_extended_path(path: PathBuf) -> PathBuf {
    let raw = path.to_string_lossy().to_string();
    if raw.starts_with(r"\\?\") || raw.starts_with(r"\\.\") {
        return path;
    }
    if let Some(unc_tail) = raw.strip_prefix(r"\\") {
        return PathBuf::from(format!(r"\\?\UNC\{}", unc_tail));
    }
    if path.is_absolute() {
        return PathBuf::from(format!(r"\\?\{}", raw));
    }
    path
}

fn validate_path_component(component: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!component.is_empty(), "empty path component");
    anyhow::ensure!(!component.contains('/'), "contains /");
    anyhow::ensure!(!component.contains('\\'), "contains \\");
    anyhow::ensure!(!component.contains(':'), "contains colon");
    anyhow::ensure!(component != "..", "parent directory traversal");
    anyhow::ensure!(component != ".", "current directory reference");
    anyhow::ensure!(!component.contains('\0'), "contains null byte");
    Ok(())
}

/// Checks if all blobs for the given ticket hash are already in the local iroh-blobs store.
/// If yes, runs the export directly to `output_dir` without any network connection (no Croc, no Sendme).
/// Returns `Ok(true)` if export succeeded from local blobs.
/// Returns `Ok(false)` if blobs are incomplete and a full receive is needed.
/// Returns `Err` if something went wrong during the local export itself.
pub async fn check_and_export_local(
    ticket_str: &str,
    output_dir: Option<PathBuf>,
    app_handle: AppHandle,
) -> anyhow::Result<bool> {
    check_and_export_local_in(ticket_str, output_dir, app_handle, None, false, None).await
}

pub fn cleanup_sendme_receive_artifacts_for_ticket(ticket_str: &str) {
    if let Ok(ticket) = BlobTicket::from_str(ticket_str) {
        let dir_name = format!(".sendme-recv-{}", ticket.hash().to_hex());
        let temp_candidate = std::env::temp_dir().join(&dir_name);
        if temp_candidate.exists() {
            let _ = std::fs::remove_dir_all(temp_candidate);
        }
    }
}

pub fn ticket_to_hex_hash(ticket_str: &str) -> Option<String> {
    BlobTicket::from_str(ticket_str).ok().map(|t| t.hash().to_hex())
}


/// Same as `check_and_export_local` but also checks `extra_blob_dir` if the default
/// temp-dir location doesn't have the blobs (e.g., user configured a custom blob dir).
pub async fn check_and_export_local_in(
    ticket_str: &str,
    output_dir: Option<PathBuf>,
    app_handle: AppHandle,
    blob_dir: Option<PathBuf>,
    _overwrite: bool,
    cancel_token: Option<tokio::sync::oneshot::Receiver<()>>,
) -> anyhow::Result<bool> {
    let ticket = match BlobTicket::from_str(ticket_str) {
        Ok(t) => t,
        Err(_) => return Ok(false), // malformed ticket, need full receive
    };

    let dir_name = format!(".sendme-recv-{}", ticket.hash().to_hex());
    // Check both the user-specified blob dir and the default temp dir.
    let candidates: Vec<PathBuf> = {
        let mut v = Vec::new();
        if let Some(ref bd) = blob_dir {
            v.push(bd.join(&dir_name));
        }
        let temp_candidate = std::env::temp_dir().join(&dir_name);
        if !v.contains(&temp_candidate) {
            v.push(temp_candidate);
        }
        v
    };
    let iroh_data_dir = match candidates.into_iter().find(|p| p.exists()) {
        Some(p) => p,
        None => return Ok(false),
    };

    let db = match FsStore::load(&iroh_data_dir).await {
        Ok(db) => db,
        Err(_) => return Ok(false),
    };

    let hash_and_format = ticket.hash_and_format();
    let local = db.remote().local(hash_and_format).await?;

    if !local.is_complete() {
        return Ok(false); // partial download, need to reconnect to sender
    }

    // All blobs are local — run export directly, no network needed
    let output_dir = output_dir.unwrap_or_else(|| {
        dirs::download_dir().unwrap_or_else(|| std::env::current_dir().unwrap())
    });

    // Load collection from local store
    let collection = Collection::load(hash_and_format.hash, db.as_ref()).await?;

    // Emit file names
    let mut file_names: Vec<String> = Vec::new();
    for (name, _hash) in collection.iter() {
        file_names.push(name.to_string());
    }
    if !file_names.is_empty() {
        let json = serde_json::to_string(&file_names).unwrap_or_else(|_| "[]".to_string());
        emit_event_with_payload(&app_handle, "receive-file-names", &json);
    }

    // Use exact same smart resolution logic as the download path
    let content_hash = ticket.hash().to_hex().to_string();
    let mut final_output_dir = output_dir.to_path_buf();
    
    if !collection.is_empty() {
        let root_name = collection.iter().next().unwrap().0.clone();
        let dir_component = root_name.split('/').next().unwrap_or(&root_name);
        
        let mut attempt = 1;
        loop {
            let name = if attempt == 1 {
                dir_component.to_string()
            } else {
                format!("{}_{}", attempt - 1, dir_component)
            };
            
            let check_path = output_dir.join(&name);
            if !check_path.exists() {
                final_output_dir = check_path;
                break;
            }
            
            let signature_path = check_path.join("databeam_hash.txt");
            if let Ok(existing_hash) = std::fs::read_to_string(&signature_path) {
                if existing_hash.trim() == content_hash {
                    final_output_dir = check_path;
                    let _ = tokio::fs::remove_dir_all(&final_output_dir).await;
                    break;
                }
            }
            attempt += 1;
            if attempt > 100 { anyhow::bail!("Too many conflicting versions"); }
        }
    }

    emit_event(&app_handle, "receive-started");
    emit_event(&app_handle, "receive-export-started");
    let export_fut = export(&db, collection, &final_output_dir, &app_handle, &content_hash);

    if let Some(mut cancel_rx) = cancel_token {
        tokio::select! {
            res = export_fut => {
                res?;
            }
            _ = &mut cancel_rx => {
                tracing::warn!("Local export cancelled by token");
                anyhow::bail!("Operation cancelled");
            }
        }
    } else {
        export_fut.await?;
    }

    emit_event(&app_handle, "receive-completed");
    
    // Remove the smart-overwrite signature file now that export is finished
    let signature_path = final_output_dir.join("databeam_hash.txt");
    let _ = tokio::fs::remove_file(&signature_path).await;

    // Clean up the temporary blob store after successful local export
    if let Ok(ticket) = BlobTicket::from_str(ticket_str) {
        let expected_dir = format!(".sendme-recv-{}", ticket.hash().to_hex());
        let iroh_data_dir = std::env::temp_dir().join(expected_dir);
        let _ = tokio::fs::remove_dir_all(&iroh_data_dir).await;
    }

    Ok(true)
}
/// Checks if a specific ticket exists as a local blob directory in temp_dir.
pub fn local_ticket_exists_on_disk(ticket_str: &str) -> bool {
    let Ok(ticket) = BlobTicket::from_str(ticket_str) else {
        return false;
    };
    let expected_dir = format!(".sendme-recv-{}", ticket.hash().to_hex());
    std::env::temp_dir().join(expected_dir).exists()
}

/// Recursively calculates the size of a directory.
fn calculate_dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let metadata = entry.metadata().ok();
            if let Some(m) = metadata {
                if m.is_dir() {
                    total += calculate_dir_size(&entry.path());
                } else {
                    total += m.len();
                }
            }
        }
    }
    total
}

/// Returns the total size in bytes of the local blob directory for the given ticket.
pub fn local_ticket_size_on_disk(ticket_str: &str) -> u64 {
    let Ok(ticket) = BlobTicket::from_str(ticket_str) else {
        return 0;
    };
    let expected_dir = format!(".sendme-recv-{}", ticket.hash().to_hex());
    let path = std::env::temp_dir().join(expected_dir);
    if !path.exists() {
        return 0;
    }
    calculate_dir_size(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_empty() {
        assert!(validate_path_component("").is_err());
    }

    #[test]
    fn validate_rejects_slash() {
        assert!(validate_path_component("a/b").is_err());
    }

    #[test]
    fn validate_rejects_backslash() {
        assert!(validate_path_component("a\\b").is_err());
    }

    #[test]
    fn validate_rejects_parent_traversal() {
        assert!(validate_path_component("..").is_err());
    }

    #[test]
    fn validate_rejects_dot() {
        assert!(validate_path_component(".").is_err());
    }

    #[test]
    fn validate_rejects_null_byte() {
        assert!(validate_path_component("a\0b").is_err());
    }

    #[test]
    fn validate_rejects_colon() {
        assert!(validate_path_component("C:foo").is_err());
    }

    #[test]
    fn validate_accepts_normal() {
        assert!(validate_path_component("file.txt").is_ok());
        assert!(validate_path_component("my-file_v2.tar.gz").is_ok());
    }

    #[test]
    fn get_export_path_blocks_drive_prefix() {
        let root = Path::new("/tmp/test");
        assert!(get_export_path(root, "C:foo").is_err());
    }

    #[test]
    fn get_export_path_blocks_traversal() {
        let root = Path::new("/tmp/test");
        assert!(get_export_path(root, "../etc/passwd").is_err());
        assert!(get_export_path(root, "subdir/../../etc/passwd").is_err());
    }

    #[test]
    fn get_export_path_blocks_backslash() {
        assert!(get_export_path(Path::new("/tmp/test"), "file\\name").is_err());
    }

    #[test]
    fn get_export_path_allows_normal() {
        let p = get_export_path(Path::new("/tmp/test"), "subdir/file.txt").unwrap();
        assert_eq!(p, PathBuf::from("/tmp/test/subdir/file.txt"));
    }
}

fn show_get_error(e: GetError) -> GetError {
    match &e {
        GetError::InitialNext { source, .. } => {
            tracing::error!("initial connection error: {source}");
        }
        GetError::ConnectedNext { source, .. } => {
            tracing::error!("connected error: {source}");
        }
        GetError::AtBlobHeaderNext { source, .. } => {
            tracing::error!("reading blob header error: {source}");
        }
        GetError::Decode { source, .. } => {
            tracing::error!("decoding error: {source}");
        }
        GetError::IrpcSend { source, .. } => {
            tracing::error!("error sending over irpc: {source}");
        }
        GetError::AtClosingNext { source, .. } => {
            tracing::error!("error at closing: {source}");
        }
        GetError::BadRequest { .. } => {
            tracing::error!("bad request");
        }
        GetError::LocalFailure { source, .. } => {
            tracing::error!("local failure {source:?}");
        }
    }
    e
}
