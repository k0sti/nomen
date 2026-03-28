//! nomen-relay — Nostr relay management, event parsing, and key signing.
//!
//! Handles relay connections, event publishing/fetching, NIP-44 encryption,
//! and parsing Nostr events into Nomen memory types.

pub mod events;
pub mod send;
pub mod signer;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use nostr_sdk::prelude::*;
use tracing::{debug, info, warn};

use nomen_core::kinds::{
    COLLECTED_MESSAGE_KIND, LEGACY_APP_DATA_KIND, LEGACY_LESSON_KIND, LESSON_KIND, MEMORY_KIND,
};
use nomen_core::signer::NomenSigner;

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

    /// Fetch memory events (kind 31234 + legacy 30078) and agent lessons (kind 31235 + legacy 4129).
    pub async fn fetch_memories(&self, pubkeys: &[PublicKey]) -> Result<Events> {
        let filter = Filter::new()
            .kinds(vec![
                Kind::Custom(MEMORY_KIND),
                Kind::Custom(LESSON_KIND),
                Kind::Custom(LEGACY_APP_DATA_KIND),
                Kind::Custom(LEGACY_LESSON_KIND),
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

    /// Fetch collected message events (kind 30100) for the given pubkeys.
    pub async fn fetch_messages(&self, pubkeys: &[PublicKey]) -> Result<Events> {
        let filter = Filter::new()
            .kinds(vec![Kind::Custom(COLLECTED_MESSAGE_KIND)])
            .authors(pubkeys.to_vec());

        debug!("Fetching collected messages for {} pubkeys", pubkeys.len());
        let events = self
            .client
            .fetch_events(filter, self.config.timeout)
            .await
            .context("Failed to fetch collected messages from relay")?;

        info!(count = events.len(), "Fetched collected message events");
        Ok(events)
    }

    /// Sign an event builder, publish, and return the signed event alongside relay status.
    ///
    /// Use this when you need the signed event's id/sig (e.g. to store in DB).
    pub async fn sign_and_publish(&self, builder: EventBuilder) -> Result<(Event, PublishResult)> {
        let signed = self
            .client
            .sign_event_builder(builder)
            .await
            .context("Failed to sign event")?;

        let output = self
            .client
            .send_event(&signed)
            .await
            .context("Failed to publish signed event")?;

        let event_id = signed.id;

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
                "Event signed and published"
            );
        }

        let publish_result = PublishResult {
            event_id,
            accepted,
            rejected,
        };

        Ok((signed, publish_result))
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
                Kind::Custom(LESSON_KIND),
                Kind::Custom(LEGACY_APP_DATA_KIND),
                Kind::Custom(LEGACY_LESSON_KIND),
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

    /// Disconnect from the relay.
    pub async fn disconnect(&self) {
        self.client.disconnect().await;
    }

    /// Publish a group definition as a kind 30000 event.
    pub async fn publish_group(
        &self,
        group_id: &str,
        name: &str,
        members: &[String],
        relay_url: Option<&str>,
        parent: Option<&str>,
    ) -> Result<PublishResult> {
        let mut tags = vec![
            Tag::custom(TagKind::Custom("d".into()), vec![group_id.to_string()]),
            Tag::custom(TagKind::Custom("name".into()), vec![name.to_string()]),
        ];
        for member in members {
            tags.push(Tag::custom(
                TagKind::Custom("member".into()),
                vec![member.clone()],
            ));
        }
        if let Some(url) = relay_url {
            if !url.is_empty() {
                tags.push(Tag::custom(
                    TagKind::Custom("relay".into()),
                    vec![url.to_string()],
                ));
            }
        }
        if let Some(p) = parent {
            if !p.is_empty() {
                tags.push(Tag::custom(
                    TagKind::Custom("parent".into()),
                    vec![p.to_string()],
                ));
            }
        }

        let builder = EventBuilder::new(Kind::Custom(nomen_core::kinds::GROUP_KIND), "")
            .tags(tags);

        self.publish(builder).await
    }

    /// Fetch group events (kind 30000) from relay for the given author pubkeys.
    pub async fn fetch_groups(&self, pubkeys: &[PublicKey]) -> Result<Vec<Event>> {
        let filter = Filter::new()
            .kinds(vec![Kind::Custom(nomen_core::kinds::GROUP_KIND)])
            .authors(pubkeys.to_vec());

        let events = self
            .client
            .fetch_events(filter, Duration::from_secs(10))
            .await?;

        Ok(events.into_iter().collect())
    }
}
