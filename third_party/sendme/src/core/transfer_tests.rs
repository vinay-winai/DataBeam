//! Loopback regression coverage for the upstream security and route changes.
use super::{receive::download, send::start_share, types::*};
use anyhow::Context;
use iroh::{endpoint::presets, Endpoint, EndpointAddr, RelayMode};
use iroh_blobs::{
    api::blobs::BlobStatus,
    protocol::{ChunkRangesSeq, PushRequest, RequestType, ERR_PERMISSION},
    ticket::BlobTicket,
    BlobFormat,
};
use irpc::util::WriteVarintExt;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Default)]
struct Events(Mutex<Vec<(String, String)>>);

impl EventEmitter for Events {
    fn emit_event(&self, name: &str) -> Result<(), String> {
        self.0.lock().unwrap().push((name.into(), String::new()));
        Ok(())
    }
    fn emit_event_with_payload(&self, name: &str, payload: &str) -> Result<(), String> {
        self.0.lock().unwrap().push((name.into(), payload.into()));
        Ok(())
    }
}

#[tokio::test]
async fn delayed_payload_after_repeated_size_probes_does_not_complete_sender() -> anyhow::Result<()>
{
    use iroh_blobs::{
        api::remote::GetProgressItem, format::collection::Collection,
        get::request::get_hash_seq_and_sizes, store::fs::FsStore,
    };
    use n0_future::StreamExt;

    tokio::time::timeout(Duration::from_secs(40), async {
        for kind in ["file", "directory", "empty", "unprobed", "resumed"] {
            let dir = tempfile::tempdir()?;
            let source = dir.path().join(kind);
            let expected = match kind {
                "directory" | "resumed" => {
                    std::fs::create_dir(&source)?;
                    std::fs::write(source.join("first.bin"), vec![0x5a; 44_100])?;
                    std::fs::write(source.join("second.bin"), vec![0x33; 8_000])?;
                    vec![vec![0x5a; 44_100], vec![0x33; 8_000]]
                }
                "empty" => {
                    std::fs::write(&source, [])?;
                    vec![vec![]]
                }
                _ => {
                    std::fs::write(&source, vec![0x5a; 44_100])?;
                    vec![vec![0x5a; 44_100]]
                }
            };
            let events = Arc::new(Events::default());
            let share = start_share(
                vec![source],
                "payload".into(),
                SendOptions {
                    relay_mode: RelayModeOption::Disabled,
                    magic_ipv4_addr: Some("127.0.0.1:0".parse()?),
                    blob_dir: Some(dir.path().join("sender")),
                    ..Default::default()
                },
                Some(events.clone()),
            )
            .await?;
            let endpoint = share.router.endpoint();
            let bound = endpoint
                .bound_sockets()
                .into_iter()
                .find(|a| a.is_ipv4())
                .unwrap();
            let addr = EndpointAddr::new(endpoint.id()).with_ip_addr(bound);
            let peer = Endpoint::builder(presets::N0)
                .relay_mode(RelayMode::Disabled)
                .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddr>()?)?
                .bind()
                .await?;
            let connection = peer.connect(addr, iroh_blobs::ALPN).await?;
            // Size probes fetch the root and the last chunk of each child. They
            // are not payload completion, even on request IDs greater than zero.
            for _ in 0..if kind == "unprobed" { 0 } else { 2 } {
                get_hash_seq_and_sizes(&connection, &share.temp_tag.hash(), 32 * 1024 * 1024, None)
                    .await?;
                tokio::time::sleep(Duration::from_millis(1_100)).await;
                assert!(
                    !events
                        .0
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|(name, _)| name == "transfer-completed"),
                    "{kind}: sender completed after metadata, before any payload request"
                );
            }
            let db = FsStore::load(dir.path().join("receiver")).await?;
            // A resumed receiver can already have one file and request only
            // the remaining child. Completion must still work in that case.
            let _cached_tag = if kind == "resumed" {
                Some(db.add_bytes(expected[0].clone()).await?)
            } else {
                None
            };
            let content = iroh_blobs::HashAndFormat::hash_seq(share.temp_tag.hash());
            let local = db.remote().local(content).await?;
            let get = db.remote().execute_get(connection, local.missing());
            let mut stream = get.stream();
            let mut finished = false;
            while let Some(item) = stream.next().await {
                match item {
                    GetProgressItem::Done(_) => {
                        finished = true;
                        break;
                    }
                    GetProgressItem::Error(error) => anyhow::bail!("{kind}: {error}"),
                    _ => {}
                }
            }
            assert!(finished, "{kind}: payload stream ended early");
            assert!(db.remote().local(content).await?.is_complete());
            let collection = Collection::load(content.hash, db.as_ref()).await?;
            assert_eq!(collection.len(), expected.len());
            for ((_, hash), bytes) in collection.iter().zip(&expected) {
                assert_eq!(db.get_bytes(*hash).await?.as_ref(), bytes.as_slice());
            }
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if events
                        .0
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|(name, _)| name == "transfer-completed")
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .context("sender did not complete after the real payload request")?;
            peer.close().await;
            share.router.shutdown().await?;
            db.shutdown().await?;
        }
        anyhow::Ok(())
    })
    .await?
}

#[tokio::test]
async fn loopback_download_reports_direct_and_sender_rejects_push() -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(30), async {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("payload.bin");
        let bytes = vec![0x5a; 2 * 1024 * 1024];
        std::fs::write(&source, &bytes)?;
        let share = start_share(
            vec![source],
            "payload".into(),
            SendOptions {
                relay_mode: RelayModeOption::Disabled,
                magic_ipv4_addr: Some("127.0.0.1:0".parse()?),
                blob_dir: Some(dir.path().join("sender")),
                ..Default::default()
            },
            None,
        )
        .await.context("start sharing")?;
        let sender = share.router.endpoint();
        let bound = sender
            .bound_sockets()
            .into_iter()
            .find(|addr| addr.is_ipv4())
            .unwrap();
        let addr = EndpointAddr::new(sender.id()).with_ip_addr(bound);

        // A peer with the sender's address/ticket may download, but must not
        // be able to write arbitrary blobs into the sender's store.
        let peer = Endpoint::builder(presets::N0)
            .relay_mode(RelayMode::Disabled)
            .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddr>()?)?
            .bind()
            .await?;
        let connection = peer.connect(addr.clone(), iroh_blobs::ALPN).await?;
        let unwanted = iroh_blobs::Hash::new(b"unsolicited upload");
        let (mut send, mut recv) = connection.open_bi().await?;
        let mut header = vec![RequestType::Push as u8];
        header.write_length_prefixed(PushRequest::new(unwanted, ChunkRangesSeq::root()))?;
        send.write_all(&header).await?;
        // execute_push() only confirms writes to local QUIC buffers. Check the
        // actual remote permission rejection before sending any blob body.
        let response = recv.read(&mut [0u8; 1]).await;
        assert!(
            matches!(response, Err(iroh::endpoint::ReadError::Reset(code)) if code == ERR_PERMISSION),
            "expected remote permission rejection, got {response:?}"
        );
        assert!(matches!(
            share._store.status(unwanted).await.context("check rejected blob")?,
            BlobStatus::NotFound
        ));
        peer.close().await;

        let events = Arc::new(Events::default());
        let ticket = BlobTicket::new(addr, share.temp_tag.hash(), BlobFormat::HashSeq);
        let result = download(
            ticket.to_string(),
            ReceiveOptions {
                relay_mode: RelayModeOption::Disabled,
                output_dir: Some(dir.path().join("received")),
                blob_dir: Some(dir.path().join("receiver")),
                ..Default::default()
            },
            Some(events.clone()),
        )
        .await.context("download and export")?;
        assert_eq!(std::fs::read(result.file_path.join("payload.bin"))?, bytes);
        assert!(
            events
                .0
                .lock()
                .unwrap()
                .iter()
                .any(|(name, payload)| { name == "receive-route" && payload == "direct" }),
            "download did not report its selected direct path"
        );
        share.router.shutdown().await.context("stop sharing router")?;
        anyhow::Ok(())
    })
    .await?
}
