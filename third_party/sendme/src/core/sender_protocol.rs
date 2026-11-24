//! Keep a one-shot sender alive until QUIC acknowledges the response data.
use super::types::AppHandle;
use n0_future::StreamExt;
use std::{
    collections::HashSet,
    io,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use iroh::{
    endpoint::{Connection, ConnectionError, SendStream, StoppedError, VarInt, WriteError},
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

// Provider abort events carry byte counts without a cause. Record the transport
// error against the exact stream before the provider publishes its abort.
pub(super) type PeerInterruptions = Arc<Mutex<HashSet<(u64, u64)>>>;

// Sent once a native receiver has read and verified the complete GET.
pub(super) const PAYLOAD_RECEIVED: VarInt = VarInt::from_u32(u32::from_be_bytes(*b"DBOK"));

#[derive(Clone)]
pub(super) struct AcknowledgedBlobs {
    store: Store,
    events: EventSender,
    route_emitter: AppHandle,
    peer_interruptions: PeerInterruptions,
}

impl std::fmt::Debug for AcknowledgedBlobs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcknowledgedBlobs")
            .field("store", &self.store)
            .finish_non_exhaustive()
    }
}

impl AcknowledgedBlobs {
    pub(super) fn new(store: Store, events: EventSender) -> Self {
        Self {
            store,
            events,
            route_emitter: None,
            peer_interruptions: PeerInterruptions::default(),
        }
    }

    pub(super) fn with_route_emitter(mut self, emitter: AppHandle) -> Self {
        self.route_emitter = emitter;
        self
    }

    pub(super) fn store(&self) -> &Store {
        &self.store
    }

    pub(super) fn peer_interruptions(&self) -> PeerInterruptions {
        self.peer_interruptions.clone()
    }
}

impl ProtocolHandler for AcknowledgedBlobs {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let connection_id = connection.stable_id() as u64;
        // Report each receiver independently: simultaneous receivers may use different paths.
        let route_emitter = self.route_emitter.clone();
        struct RouteClosed {
            emitter: AppHandle,
            id: u64,
        }
        impl Drop for RouteClosed {
            fn drop(&mut self) {
                if let Some(emitter) = &self.emitter {
                    let _ = emitter.emit_event_with_payload(
                        "send-route",
                        &serde_json::json!({"connection":self.id.to_string(),"route":"closed"})
                            .to_string(),
                    );
                }
            }
        }
        let _closed = RouteClosed {
            emitter: route_emitter.clone(),
            id: connection_id,
        };
        if let Some(emitter) = &route_emitter {
            let _ = emitter.emit_event_with_payload(
                "send-route",
                &serde_json::json!({"connection":connection_id.to_string(),"route":"unknown"})
                    .to_string(),
            );
        }
        let _route_watcher = n0_future::task::AbortOnDropHandle::new(tokio::spawn({
            let connection = connection.clone();
            let route_emitter = route_emitter.clone();
            async move {
                let mut paths = connection.paths_stream();
                let mut last = None;
                while let Some(list) = paths.next().await {
                    if let Some(emitter) = &route_emitter {
                        let _ = emitter.emit_event_with_payload("send-paths", &serde_json::json!({
                            "connection": connection_id.to_string(),
                            "paths": super::network::describe_paths(&list),
                        }).to_string());
                    }
                    let selected = (&list)
                        .into_iter()
                        .find(|path| path.is_selected())
                        .map(|path| path.is_relay());
                    if selected != last {
                        if let Some(emitter) = &route_emitter {
                            let route = match selected {
                                Some(true) => "relay",
                                Some(false) => "direct",
                                None => "unknown",
                            };
                            let _=emitter.emit_event_with_payload("send-route",&serde_json::json!({"connection":connection_id.to_string(),"route":route}).to_string());
                        }
                        last = selected;
                    }
                }
            }
        }));
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
                    let request = (connection_id, send.id().index());
                    let writer = AcknowledgedSendStream {
                        stream: send,
                        request: Some(ActiveRequest {
                            interruptions: self.peer_interruptions(),
                            key: request,
                            connection: connection.clone(),
                        }),
                    };
                    let pair = StreamPair::new(connection_id, recv, writer, self.events.clone());
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

struct ActiveRequest {
    interruptions: PeerInterruptions,
    key: (u64, u64),
    connection: Connection,
}

struct AcknowledgedSendStream {
    stream: SendStream,
    request: Option<ActiveRequest>,
}

fn payload_received(error: &ConnectionError) -> bool {
    matches!(error, ConnectionError::ApplicationClosed(close) if close.error_code == PAYLOAD_RECEIVED)
}

impl AcknowledgedSendStream {
    fn record_peer_interruption(&self) {
        if let Some(request) = &self.request {
            request.interruptions.lock().unwrap().insert(request.key);
        }
    }

    fn write_result(&self, result: Result<(), WriteError>) -> io::Result<()> {
        match &result {
            Err(WriteError::Stopped(_)) => self.record_peer_interruption(),
            Err(WriteError::ConnectionLost(error)) if super::network::peer_connection_lost(error) => {
                self.record_peer_interruption();
            }
            _ => {}
        }
        result.map_err(io::Error::other)
    }
}

impl BlobSendStream for AcknowledgedSendStream {
    async fn send_bytes(&mut self, bytes: Bytes) -> io::Result<()> {
        let result = self.stream.write_chunk(bytes).await;
        self.write_result(result)
    }

    async fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        let result = self.stream.write_all(bytes).await;
        self.write_result(result)
    }

    async fn sync(&mut self) -> io::Result<()> {
        // Upstream's QUIC adapter makes sync() a no-op. Its Completed event
        // therefore means queued, not delivered. Finish and await the ACK of
        // all bytes plus FIN before allowing one-shot mode to stop the runtime.
        self.stream.finish().map_err(io::Error::other)?;
        match self.stream.stopped().await {
            Ok(None) => Ok(()),
            Err(StoppedError::ConnectionLost(error)) if payload_received(&error) => Ok(()),
            Err(error) => {
                if matches!(&error, StoppedError::ConnectionLost(reason) if super::network::peer_connection_lost(reason)) {
                    self.record_peer_interruption();
                }
                Err(io::Error::other(error))
            }
            Ok(Some(code)) => {
                // The GET reader can drop the stream after verifying its final
                // range, before reading FIN. Give its explicit receipt time
                // to arrive if the stream closed with the default drop code.
                if code == VarInt::from_u32(0) {
                    if let Some(request) = &self.request {
                        let close = tokio::time::timeout(
                            std::time::Duration::from_secs(2),
                            request.connection.closed(),
                        ).await;
                        if close.as_ref().is_ok_and(payload_received) {
                            return Ok(());
                        }
                    }
                }
                self.record_peer_interruption();
                Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    format!("receiver stopped the response: {code}"),
                ))
            }
        }
    }

    fn reset(&mut self, code: VarInt) -> io::Result<()> {
        self.stream.reset(code).map_err(io::Error::other)
    }

    async fn stopped(&mut self) -> io::Result<Option<VarInt>> {
        self.stream.stopped().await.map_err(io::Error::other)
    }

    fn id(&self) -> u64 {
        self.stream.id().index()
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
    async fn final_stream_stop_only_completes_with_verified_receipt() -> anyhow::Result<()> {
        use iroh::{endpoint::presets, Endpoint, EndpointAddr, RelayMode};

        tokio::time::timeout(Duration::from_secs(15), async {
            let listener = Endpoint::builder(presets::Minimal)
                .alpns(vec![iroh_blobs::ALPN.to_vec()])
                .relay_mode(RelayMode::Disabled)
                .bind()
                .await?;
            let port = listener.bound_sockets().into_iter().find(|addr| addr.is_ipv4())
                .expect("IPv4 listener").port();
            let address = EndpointAddr::new(listener.id())
                .with_ip_addr(([127, 0, 0, 1], port).into());
            let client = Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Disabled)
                .bind()
                .await?;
            for verified in [true, false] {
                let interruptions = PeerInterruptions::default();
                let (peer_stopped, wait_for_stop) = tokio::sync::oneshot::channel();
                let sender = async {
                    let connection = listener.accept().await.expect("incoming connection").await?;
                    let (send, mut req) = connection.accept_bi().await?;
                    req.read_to_end(1).await?;
                    let mut response = AcknowledgedSendStream {
                        stream: send,
                        request: Some(ActiveRequest {
                            interruptions: interruptions.clone(),
                            key: (1, 0),
                            connection,
                        }),
                    };
                    response.send(b"data").await?;
                    assert_eq!(response.stream.stopped().await?, Some(0u32.into()));
                    peer_stopped.send(()).expect("receiver is waiting for STOP");
                    response.sync().await
                        .map_err(anyhow::Error::from)
                        .map(|_| true)
                        .or_else(|error| {
                            if interruptions.lock().unwrap().contains(&(1, 0)) {
                                Ok(false)
                            } else {
                                Err(error)
                            }
                        })
                };
                let receiver = async {
                    let connection = client.connect(address.clone(), iroh_blobs::ALPN).await?;
                    let (mut req, mut response) = connection.open_bi().await?;
                    req.write_all(b"x").await?;
                    req.finish()?;
                    let mut bytes = [0; 4];
                    response.read_exact(&mut bytes).await?;
                    assert_eq!(&bytes, b"data");
                    response.stop(0u32.into())?;
                    wait_for_stop.await?;
                    let code = if verified { PAYLOAD_RECEIVED } else { 0u32.into() };
                    connection.close(code, b"");
                    anyhow::Ok(())
                };
                let (completed, ()) = tokio::try_join!(sender, receiver)?;
                assert_eq!(completed, verified);
                assert_eq!(interruptions.lock().unwrap().contains(&(1, 0)), !verified);
            }
            anyhow::Ok(())
        }).await?
    }

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
                    let mut send = AcknowledgedSendStream { stream: send, request: None };
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
