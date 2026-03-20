use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use nostr_sdk::prelude::*;
use tracing::{debug, info, warn};

use crate::kinds::{MEMORY_KIND, RAW_SOURCE_KIND};
use crate::signer::NomenSigner;

/// Result of publishing an event to relays.
pub struct PublishResult {
    pub event_id: EventId,
    /// Relay URLs that accepted the event.
    pub accepted: Vec<String>,
    /// Relay URLs that rejected the event, with reason.
    pub rejected: Vec<(String, String)>,
}

impl PublishResult {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.accepted.is_empty() {
            parts.push(format!("accepted by {} relay(s)", self.accepted.len()));
        }
        if !self.rejected.is_empty() {
            parts.push(format!("rejected by {} relay(s)", self.rejected.len()));
        }
        if parts.is_empty() {
            "no relay responses".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// Configuration for the relay manager.
pub struct RelayConfig {
    pub relay_url: String,
    pub timeout: Duration,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            relay_url: "wss://zooid.atlantislabs.space".to_string(),
            timeout: Duration::from_secs(10),
        }
    }
}

/// Manages Nostr relay connections, event publishing, and NIP-44 encryption.
pub struct RelayManager {
    client: Client,
    signer: Arc<dyn NomenSigner>,
    config: RelayConfig,
}

impl RelayManager {
    /// Create a new RelayManager with a signer (does not connect yet).
    pub fn new(signer: Arc<dyn NomenSigner>, config: RelayConfig) -> Self {
        // Build Client with Keys if the signer has a secret key (needed for
        // nostr-sdk's internal signing in send_event_builder, gift_wrap, etc.)
        let client = if let Some(sk) = signer.secret_key() {
            let keys = Keys::new(sk.clone());
            ClientBuilder::new().signer(keys).build()
        } else {
            ClientBuilder::new().build()
        };

        Self {
            client,
            signer,
            config,
        }
    }

    /// Connect to the relay with NIP-42 auth (handled automatically by nostr-sdk
    /// when a signer is configured).
    pub async fn connect(&self) -> Result<()> {
        self.client
            .add_relay(&self.config.relay_url)
            .await
            .with_context(|| format!("Failed to add relay: {}", self.config.relay_url))?;

        self.client.connect().await;

        debug!("Connected to {}", self.config.relay_url);
        info!(
            relay = %self.config.relay_url,
            pubkey = %self.signer.public_key().to_bech32().unwrap_or_default(),
            "Relay connection established (NIP-42 auth enabled)"
        );

        Ok(())
    }

    /// Fetch memory events (kind 31234) and raw source events (kind 1235).
    pub async fn fetch_memories(&self, pubkeys: &[PublicKey]) -> Result<Events> {
        let filter = Filter::new()
            .kinds(vec![
                Kind::Custom(MEMORY_KIND),
                Kind::Custom(RAW_SOURCE_KIND),
            ])
            .authors(pubkeys.to_vec());

        debug!("Fetching events for {} pubkeys", pubkeys.len());
        let events = self
            .client
            .fetch_events(filter, self.config.timeout)
            .await
            .context("Failed to fetch events from relay")?;

        info!(count = events.len(), "Fetched memory events");
        Ok(events)
    }

    /// Fetch legacy lesson events (kind 31235 + 4129) for migration purposes.
    pub async fn fetch_lessons(&self, pubkeys: &[PublicKey]) -> Result<Events> {
        let filter = Filter::new()
            .kinds(vec![
                Kind::Custom(31235), // LESSON_KIND
                Kind::Custom(4129),  // LEGACY_LESSON_KIND
            ])
            .authors(pubkeys.to_vec());

        debug!("Fetching lesson events for {} pubkeys", pubkeys.len());
        let events = self
            .client
            .fetch_events(filter, self.config.timeout)
            .await
            .context("Failed to fetch lesson events from relay")?;

        info!(count = events.len(), "Fetched lesson events");
        Ok(events)
    }

    /// Publish an event and inspect the Output for accepted/rejected relay status.
    pub async fn publish(&self, builder: EventBuilder) -> Result<PublishResult> {
        let output = self
            .client
            .send_event_builder(builder)
            .await
            .context("Failed to publish event")?;

        let event_id = *output.id();

        let accepted: Vec<String> = output.success.iter().map(|url| url.to_string()).collect();

        let rejected: Vec<(String, String)> = output
            .failed
            .iter()
            .map(|(url, reason)| (url.to_string(), reason.clone()))
            .collect();

        if accepted.is_empty() && !rejected.is_empty() {
            warn!(
                event_id = %event_id,
                "Event rejected by all relays: {:?}",
                rejected
            );
        } else {
            debug!(
                event_id = %event_id,
                accepted = accepted.len(),
                rejected = rejected.len(),
                "Event published"
            );
        }

        Ok(PublishResult {
            event_id,
            accepted,
            rejected,
        })
    }

    /// Get a reference to the signer.
    pub fn signer(&self) -> &dyn NomenSigner {
        self.signer.as_ref()
    }

    /// Get a cloneable handle to the signer.
    pub fn arc_signer(&self) -> &Arc<dyn NomenSigner> {
        &self.signer
    }

    /// Get the public key from the signer.
    pub fn public_key(&self) -> PublicKey {
        self.signer.public_key()
    }

    /// Get a reference to the underlying nostr-sdk Client.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Subscribe to memory events for the given pubkeys and return new events
    /// via a tokio broadcast channel. Used for daemon/incremental sync.
    pub async fn subscribe(
        &self,
        pubkeys: &[PublicKey],
    ) -> Result<tokio::sync::mpsc::Receiver<Event>> {
        let filter = Filter::new()
            .kinds(vec![
                Kind::Custom(MEMORY_KIND),
                Kind::Custom(RAW_SOURCE_KIND),
            ])
            .authors(pubkeys.to_vec());

        self.client.subscribe(filter, None).await?;
        info!("Live subscription started for {} pubkeys", pubkeys.len());

        let (tx, rx) = tokio::sync::mpsc::channel::<Event>(256);
        let client = self.client.clone();

        tokio::spawn(async move {
            let _ = client
                .handle_notifications(|notification| {
                    let tx = tx.clone();
                    async move {
                        if let RelayPoolNotification::Event { event, .. } = notification {
                            if tx.send((*event).clone()).await.is_err() {
                                return Ok(true); // receiver dropped, stop
                            }
                        }
                        Ok(false)
                    }
                })
                .await;
        });

        Ok(rx)
    }

    /// Delete events from the relay using NIP-09 (event deletion).
    /// Publishes a kind 5 deletion event referencing the given event IDs.
    pub async fn delete_events(&self, event_ids: &[EventId], reason: &str) -> Result<PublishResult> {
        let builder = EventBuilder::delete(
            EventDeletionRequest::new()
                .ids(event_ids.to_vec())
                .reason(reason)
        );

        info!(
            count = event_ids.len(),
            reason = %reason,
            "Publishing NIP-09 deletion event"
        );

        self.publish(builder).await
    }

    /// Delete events matching a filter (fetch then delete).
    /// Returns the number of events deleted.
    pub async fn delete_matching(&self, filter: Filter, reason: &str) -> Result<usize> {
        let events = self
            .client
            .fetch_events(filter, self.config.timeout)
            .await
            .context("Failed to fetch events for deletion")?;

        if events.is_empty() {
            return Ok(0);
        }

        let ids: Vec<EventId> = events.iter().map(|e| e.id).collect();
        let count = ids.len();

        self.delete_events(&ids, reason).await?;

        Ok(count)
    }

    /// Disconnect from the relay.
    pub async fn disconnect(&self) {
        self.client.disconnect().await;
    }
}
