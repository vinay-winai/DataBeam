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
    fn emit_event(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn emit_event_with_payload(&self, name: &str, payload: &str) -> Result<(), String> {
        self.0.lock().unwrap().push((name.into(), payload.into()));
        Ok(())
    }
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
