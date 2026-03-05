use std::time::Duration;

use anyhow::{Context, Result};
use nostr_sdk::prelude::*;
use tracing::debug;

/// Connect to relay with the first key as signer (for NIP-42 AUTH).
pub async fn connect(relay_url: &str, signer_keys: &Keys) -> Result<Client> {
    let client = ClientBuilder::new()
        .signer(signer_keys.clone())
        .build();

    client.add_relay(relay_url).await?;
    client.connect().await;
    debug!("Connected to {relay_url}");
    Ok(client)
}

/// Fetch memory events (kind 30078) and agent lessons (kind 4129) for the given pubkeys.
pub async fn fetch_memory_events(
    client: &Client,
    pubkeys: &[PublicKey],
) -> Result<Events> {
    let filter = Filter::new()
        .kinds(vec![Kind::Custom(30078), Kind::Custom(4129)])
        .authors(pubkeys.to_vec());

    debug!("Fetching events...");
    let events = client
        .fetch_events(filter, Duration::from_secs(10))
        .await
        .context("Failed to fetch events")?;

    Ok(events)
}

/// Publish a signed event to the relay. Returns the event ID.
pub async fn publish_event(client: &Client, builder: EventBuilder) -> Result<EventId> {
    let output = client
        .send_event_builder(builder)
        .await
        .context("Failed to publish event")?;
    Ok(*output.id())
}
