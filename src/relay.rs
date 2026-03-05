use std::time::Duration;

use anyhow::{Context, Result};
use nostr_sdk::prelude::*;
use nostr_sdk::prelude::nip44;
use tracing::{debug, info, warn};

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
    keys: Keys,
    config: RelayConfig,
}

impl RelayManager {
    /// Create a new RelayManager (does not connect yet).
    pub fn new(keys: Keys, config: RelayConfig) -> Self {
        let client = ClientBuilder::new()
            .signer(keys.clone())
            .build();

        Self {
            client,
            keys,
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
            pubkey = %self.keys.public_key().to_bech32().unwrap_or_default(),
            "Relay connection established (NIP-42 auth enabled)"
        );

        Ok(())
    }

    /// Fetch memory events (kind 30078) and agent lessons (kind 4129) for the given pubkeys.
    pub async fn fetch_memories(&self, pubkeys: &[PublicKey]) -> Result<Events> {
        let filter = Filter::new()
            .kinds(vec![Kind::Custom(30078), Kind::Custom(4129)])
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

    /// Publish an event and inspect the Output for accepted/rejected relay status.
    pub async fn publish(&self, builder: EventBuilder) -> Result<PublishResult> {
        let output = self
            .client
            .send_event_builder(builder)
            .await
            .context("Failed to publish event")?;

        let event_id = *output.id();

        let accepted: Vec<String> = output
            .success
            .iter()
            .map(|url| url.to_string())
            .collect();

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

    /// Encrypt content for private tier using NIP-44 (self-encrypt: encrypt to own pubkey).
    pub fn encrypt_private(&self, content: &str) -> Result<String> {
        let encrypted = nip44::encrypt(
            self.keys.secret_key(),
            &self.keys.public_key(),
            content,
            nip44::Version::default(),
        )
        .map_err(|e| anyhow::anyhow!("NIP-44 encryption failed: {e}"))?;
        Ok(encrypted)
    }

    /// Decrypt NIP-44 encrypted content (self-encrypted).
    pub fn decrypt_private(&self, encrypted: &str) -> Result<String> {
        let decrypted = nip44::decrypt(
            self.keys.secret_key(),
            &self.keys.public_key(),
            encrypted,
        )
        .map_err(|e| anyhow::anyhow!("NIP-44 decryption failed: {e}"))?;
        Ok(decrypted)
    }

    /// Get a reference to the keys.
    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    /// Disconnect from the relay.
    pub async fn disconnect(&self) {
        self.client.disconnect().await;
    }
}
