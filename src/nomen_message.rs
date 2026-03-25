//! impl Nomen — message storage, query, search, send, session resolution, collected events.

use anyhow::Result;

use nomen_core::collected::{CollectedEvent, CollectedEventFilter};
use nomen_db::{CollectedMessageRecord, CollectedSearchResult};

use crate::Nomen;

impl Nomen {
    /// Store a kind 30100 collected event.
    ///
    /// If a relay is configured, signs and publishes the event first, then
    /// populates `id` and `sig` before storing in DB. Falls back to DB-only
    /// if no relay is available.
    pub async fn store_collected_event(
        &self,
        mut event: CollectedEvent,
    ) -> Result<nomen_db::collected::StoreResult> {
        // Publish to relay if available
        if let Some(ref relay) = self.relay {
            let mut tags: Vec<nostr_sdk::Tag> = Vec::new();
            for tag in &event.tags {
                if tag.len() >= 2 {
                    tags.push(nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom(tag[0].clone().into()),
                        tag[1..].to_vec(),
                    ));
                }
            }

            let builder = nostr_sdk::EventBuilder::new(
                nostr_sdk::Kind::Custom(nomen_core::kinds::COLLECTED_MESSAGE_KIND),
                &event.content,
            )
            .tags(tags);

            match relay.sign_and_publish(builder).await {
                Ok((signed_event, _publish_result)) => {
                    event.id = Some(signed_event.id.to_hex());
                    event.sig = Some(signed_event.sig.to_string());
                    event.pubkey = signed_event.pubkey.to_hex();
                }
                Err(e) => {
                    tracing::warn!("Failed to publish collected event to relay: {e}");
                }
            }
        }

        nomen_db::store_collected_event(&self.db, &event).await
    }

    /// Query collected events with tag-based filtering.
    pub async fn query_collected_events(
        &self,
        filter: CollectedEventFilter,
    ) -> Result<Vec<CollectedMessageRecord>> {
        nomen_db::query_collected_events(&self.db, &filter).await
    }

    /// BM25 fulltext search over collected messages.
    pub async fn search_collected_events(
        &self,
        query: &str,
        filter: CollectedEventFilter,
    ) -> Result<Vec<CollectedSearchResult>> {
        nomen_db::search_collected_events(&self.db, query, &filter).await
    }

    /// Store media via the configured media store.
    /// Returns None if no media store is configured.
    pub async fn store_media(
        &self,
        data: &[u8],
        mime_type: &str,
    ) -> Result<Option<nomen_media::MediaRef>> {
        match &self.media_store {
            Some(store) => Ok(Some(store.store(data, mime_type).await?)),
            None => Ok(None),
        }
    }

    /// Resolve a session ID to tier/scope/delivery-channel using the loaded groups.
    pub fn resolve_session(
        &self,
        session_id: &str,
        default_channel: &str,
    ) -> Result<crate::session::ResolvedSession> {
        crate::session::resolve_session(session_id, &self.groups, default_channel)
    }

    /// Send a message via relay.
    pub async fn send(&self, opts: crate::send::SendOptions) -> Result<crate::send::SendResult> {
        let relay = self
            .relay
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No relay configured for sending"))?;
        crate::send::send_message(relay, &self.db, &self.groups, opts).await
    }
}
