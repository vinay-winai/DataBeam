//! Keep a one-shot sender alive until QUIC acknowledges the response data.
use std::io;

use bytes::Bytes;
use iroh::{
    endpoint::{Connection, SendStream, VarInt},
    protocol::{AcceptError, ProtocolHandler},
};
use iroh_blobs::{
    api::Store,
    provider::{
        self,
        events::{ClientConnected, ConnectionClosed, EventSender, HasErrorCode},
        StreamPair,
    },
    util::SendStream as BlobSendStream,
};

#[derive(Debug, Clone)]
pub(super) struct AcknowledgedBlobs {
    store: Store,
    events: EventSender,
}

impl AcknowledgedBlobs {
    pub(super) fn new(store: Store, events: EventSender) -> Self {
        Self { store, events }
    }

    pub(super) fn store(&self) -> &Store {
        &self.store
    }
}

impl ProtocolHandler for AcknowledgedBlobs {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let connection_id = connection.stable_id() as u64;
        if let Err(error) = self
            .events
            .client_connected(|| ClientConnected {
                connection_id,
                endpoint_id: Some(connection.remote_id()),
            })
            .await
        {
            connection.close(error.code(), error.reason());
            return Ok(());
        }
        let mut streams = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                stream = connection.accept_bi() => {
                    let Ok((send, recv)) = stream else { break };
                    let pair = StreamPair::new(
                        connection_id, recv, AcknowledgedSendStream(send), self.events.clone(),
                    );
                    let store = self.store.clone();
                    streams.spawn(async move {
                        if let Err(error) = provider::handle_stream(pair, store).await {
                            tracing::debug!("Sendme response failed: {error:#}");
                        }
                    });
                }
                _ = streams.join_next(), if !streams.is_empty() => {}
            }
        }
        while streams.join_next().await.is_some() {}
        let _ = self
            .events
            .connection_closed(|| ConnectionClosed { connection_id })
            .await;
        Ok(())
    }

    async fn shutdown(&self) {
        if let Err(error) = self.store.shutdown().await {
            tracing::error!("Sendme store shutdown failed: {error:#}");
        }
    }
}

struct AcknowledgedSendStream(SendStream);

impl BlobSendStream for AcknowledgedSendStream {
    async fn send_bytes(&mut self, bytes: Bytes) -> io::Result<()> {
        self.0.write_chunk(bytes).await.map_err(io::Error::other)
    }

    async fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.write_all(bytes).await.map_err(io::Error::other)
    }

    async fn sync(&mut self) -> io::Result<()> {
        // Upstream's QUIC adapter makes sync() a no-op. Its Completed event
        // therefore means queued, not delivered. Finish and await the ACK of
        // all bytes plus FIN before allowing one-shot mode to stop the runtime.
        self.0.finish().map_err(io::Error::other)?;
        match self.0.stopped().await.map_err(io::Error::other)? {
            None => Ok(()),
            Some(code) => Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                format!("receiver stopped the response: {code}"),
            )),
        }
    }

    fn reset(&mut self, code: VarInt) -> io::Result<()> {
        self.0.reset(code).map_err(io::Error::other)
    }

    async fn stopped(&mut self) -> io::Result<Option<VarInt>> {
        self.0.stopped().await.map_err(io::Error::other)
    }

    fn id(&self) -> u64 {
        self.0.id().index()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };
    use tokio::{net::UdpSocket, sync::Notify};

    #[tokio::test]
    async fn response_completion_waits_for_delayed_network_delivery() -> anyhow::Result<()> {
        tokio::time::timeout(Duration::from_secs(20), async {
            for wait_for_ack in [false, true] {
            // Use iroh's underlying QUIC implementation directly here so
            // multipath cannot bypass the deliberately delayed UDP proxy.
            let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
            let cert_der = rustls::pki_types::CertificateDer::from(cert.cert);
            let key = rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
            let config = noq::ServerConfig::with_single_cert(vec![cert_der.clone()], key.into())?;
            let server = noq::Endpoint::server(config, "127.0.0.1:0".parse()?)?;
            let server_addr = server.local_addr()?;
            // Hold server datagrams after the handshake, simulating a slow
            // tunnel without changing host routing or VPN settings.
            let proxy = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
            let proxy_addr = proxy.local_addr()?;
            let hold = Arc::new(AtomicBool::new(false));
            let held = Arc::new(AtomicUsize::new(0));
            let release = Arc::new(Notify::new());
            let proxy_task = tokio::spawn({
                let proxy = proxy.clone(); let hold = hold.clone();
                let held = held.clone(); let release = release.clone();
                async move {
                    let mut client = None;
                    let mut pending: Vec<Vec<u8>> = Vec::new();
                    let mut buf = vec![0; 65_536];
                    loop {
                        tokio::select! {
                            received = proxy.recv_from(&mut buf) => {
                                let (len, from) = received?;
                                if from == server_addr {
                                    if hold.load(Ordering::SeqCst) {
                                        held.fetch_add(1, Ordering::SeqCst);
                                        pending.push(buf[..len].to_vec());
                                    } else if let Some(client) = client {
                                        proxy.send_to(&buf[..len], client).await?;
                                    }
                                } else {
                                    client = Some(from);
                                    proxy.send_to(&buf[..len], server_addr).await?;
                                }
                            }
                            _ = release.notified() => {
                                for packet in pending.drain(..) {
                                    if let Some(client) = client { proxy.send_to(&packet, client).await?; }
                                }
                            }
                        }
                    }
                    #[allow(unreachable_code)]
                    Ok::<(),io::Error>(())
                }
            });
            let _proxy_guard = n0_future::task::AbortOnDropHandle::new(proxy_task);
            let accept = tokio::spawn({ let server = server.clone(); async move {
                server.accept().await.unwrap().await
            }});
            let mut roots = rustls::RootCertStore::empty(); roots.add(cert_der)?;
            let client = noq::Endpoint::client("127.0.0.1:0".parse()?)?;
            client.set_default_client_config(noq::ClientConfig::with_root_certificates(Arc::new(roots))?);
            let connection = client.connect(proxy_addr, "localhost")?.await?;
            let remote = accept.await??;
            let (mut request, mut response) = connection.open_bi().await?;
            request.write_all(b"x").await?; request.finish()?;
            let (send, mut recv) = remote.accept_bi().await?;
            recv.read_to_end(8).await?;
            hold.store(true, Ordering::SeqCst);
            let completion = tokio::spawn(async move {
                if wait_for_ack {
                    let mut send = AcknowledgedSendStream(send);
                    send.send(b"fully delivered response").await?;
                    send.sync().await
                } else {
                    // Control: the upstream adapter completes while bytes are
                    // still queued locally, before the delayed link delivers.
                    let mut send = send;
                    BlobSendStream::send(&mut send, b"fully delivered response").await?;
                    BlobSendStream::sync(&mut send).await
                }
            });
            tokio::time::sleep(Duration::from_millis(1_200)).await;
            assert!(held.load(Ordering::SeqCst) > 0, "test did not delay response datagrams");
            assert_eq!(completion.is_finished(), !wait_for_ack,
                "completion must wait for network delivery only with the acknowledgment adapter");
            hold.store(false, Ordering::SeqCst); release.notify_one();
            assert_eq!(response.read_to_end(1024).await?, b"fully delivered response");
            completion.await??;
            client.close(0u32.into(), b"done"); server.close(0u32.into(), b"done");
            }
            anyhow::Ok(())
        }).await?
    }
}
