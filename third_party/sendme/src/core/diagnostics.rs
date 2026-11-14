use std::time::Duration;

/// Establish the same Iroh public-relay connection used by ordinary transfers.
/// The result describes this endpoint's selected relay, not global service health.
pub async fn check_public_relay() -> anyhow::Result<Vec<String>> {
    let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
        .bind()
        .await?;
    let connected = tokio::time::timeout(Duration::from_secs(12), endpoint.online()).await;
    let relays = endpoint.addr().relay_urls().map(ToString::to_string).collect::<Vec<_>>();
    endpoint.close().await;
    anyhow::ensure!(connected.is_ok() && !relays.is_empty(), "No Sendme public relay connection established within 12 seconds from this device");
    Ok(relays)
}
