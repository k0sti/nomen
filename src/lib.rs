//! Nomen — Nostr-native agent memory library.
//!
//! Provides hybrid search (vector + BM25), group-scoped access control,
//! message ingestion, consolidation, and Nostr relay sync backed by SurrealDB.

pub mod access;
pub mod config;
pub mod consolidate;
pub mod db;
pub mod embed;
pub mod entities;
pub mod groups;
pub mod ingest;
pub mod kinds;
pub mod memory;
pub mod relay;
pub mod search;
pub mod send;
pub mod session;

#[cfg(feature = "migrate")]
pub mod migrate;

// Binary-only modules — not part of the public library API.
#[doc(hidden)]
pub mod display;
#[doc(hidden)]
pub mod mcp;
#[doc(hidden)]
pub mod contextvm;
#[doc(hidden)]
pub mod http;

use anyhow::Result;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

use crate::config::Config;
use crate::consolidate::{ConsolidationConfig, ConsolidationReport, NoopLlmProvider};
use crate::embed::Embedder;
use crate::entities::{EntityKind, EntityRecord};
use crate::groups::GroupStore;
use crate::ingest::{MessageQuery, RawMessage, RawMessageRecord};
use crate::relay::RelayManager;
use crate::search::{SearchOptions, SearchResult};

/// High-level handle wrapping SurrealDB, embedder, relay, and groups.
pub struct Nomen {
    db: Surreal<Db>,
    embedder: Box<dyn Embedder>,
    relay: Option<RelayManager>,
    groups: GroupStore,
}

/// Options for creating a new memory directly (without relay event).
pub struct NewMemory {
    pub topic: String,
    pub summary: String,
    pub detail: String,
    pub tier: String,
    pub confidence: f64,
    /// Source label (e.g. "api", "mcp", "contextvm"). Defaults to "api".
    pub source: Option<String>,
    /// Model label. Defaults to "nomen/api".
    pub model: Option<String>,
}

/// Options for consolidation.
pub struct ConsolidateOptions {
    pub batch_size: usize,
    pub min_messages: usize,
    pub llm_provider: Option<Box<dyn consolidate::LlmProvider>>,
}

impl Default for ConsolidateOptions {
    fn default() -> Self {
        Self {
            batch_size: 50,
            min_messages: 3,
            llm_provider: None,
        }
    }
}

impl Nomen {
    /// Open a Nomen instance from a [`Config`].
    ///
    /// Initialises SurrealDB, builds the embedder, and loads groups.
    /// Does **not** connect to any relay — call methods on [`RelayManager`]
    /// separately if relay interaction is needed.
    pub async fn open(config: &Config) -> Result<Self> {
        let db = db::init_db_with_dimensions(config.embedding_dimensions()).await?;
        let embedder = config.build_embedder();
        let groups = GroupStore::load(&config.groups, &db).await?;

        Ok(Self {
            db,
            embedder,
            relay: None,
            groups,
        })
    }

    /// Open with an explicit relay manager (already connected or not).
    pub async fn open_with_relay(config: &Config, relay: RelayManager) -> Result<Self> {
        let mut nomen = Self::open(config).await?;
        nomen.relay = Some(relay);
        Ok(nomen)
    }

    /// Get a reference to the underlying SurrealDB handle.
    pub fn db(&self) -> &Surreal<Db> {
        &self.db
    }

    /// Get a reference to the embedder.
    pub fn embedder(&self) -> &dyn Embedder {
        self.embedder.as_ref()
    }

    /// Get a reference to the relay manager, if set.
    pub fn relay(&self) -> Option<&RelayManager> {
        self.relay.as_ref()
    }

    /// Get a reference to the group store.
    pub fn groups(&self) -> &GroupStore {
        &self.groups
    }

    /// Hybrid (vector + full-text) search over stored memories.
    pub async fn search(&self, opts: SearchOptions) -> Result<Vec<SearchResult>> {
        search::search(&self.db, self.embedder.as_ref(), &opts).await
    }

    /// Store a new memory into SurrealDB and optionally publish to relay.
    ///
    /// Builds the ParsedMemory, stores in DB, generates embeddings,
    /// and publishes to relay if available. For personal/internal tier,
    /// content is NIP-44 encrypted before relay publish.
    pub async fn store(&self, mem: NewMemory) -> Result<String> {
        let d_tag = mem.topic.clone();
        let source = mem.source.as_deref().unwrap_or("api");
        let model = mem.model.as_deref().unwrap_or("nomen/api");
        let detail_text = if mem.detail.is_empty() { &mem.summary } else { &mem.detail };
        let content = serde_json::json!({
            "summary": mem.summary,
            "detail": detail_text,
        });
        let content_str = content.to_string();
        let base_tier = memory::base_tier(&mem.tier);

        let parsed = memory::ParsedMemory {
            tier: mem.tier.clone(),
            topic: mem.topic,
            version: "1".to_string(),
            confidence: format!("{:.2}", mem.confidence),
            model: model.to_string(),
            summary: mem.summary.clone(),
            created_at: nostr_sdk::Timestamp::now(),
            d_tag: d_tag.clone(),
            source: source.to_string(),
            content_raw: content_str.clone(),
            detail: detail_text.to_string(),
        };

        db::store_memory_direct(&self.db, &parsed, source).await?;

        // Generate embedding if embedder is configured
        if self.embedder.dimensions() > 0 {
            let text = format!("{} {}", parsed.summary, parsed.detail);
            if let Ok(embeddings) = self.embedder.embed(&[text]).await {
                if let Some(embedding) = embeddings.into_iter().next() {
                    let _ = db::store_embedding(&self.db, &d_tag, embedding).await;
                }
            }
        }

        // Publish to relay if available
        if let Some(ref relay) = self.relay {
            // NIP-44 encrypt for personal/internal tier
            let final_content = if base_tier == "personal" || base_tier == "internal" {
                relay.encrypt_private(&content_str).unwrap_or(content_str)
            } else {
                content_str
            };

            let mut tags = vec![
                nostr_sdk::Tag::custom(nostr_sdk::TagKind::Custom("d".into()), vec![d_tag.clone()]),
                nostr_sdk::Tag::custom(nostr_sdk::TagKind::Custom("model".into()), vec![model.to_string()]),
                nostr_sdk::Tag::custom(nostr_sdk::TagKind::Custom("confidence".into()), vec![format!("{:.2}", mem.confidence)]),
                nostr_sdk::Tag::custom(nostr_sdk::TagKind::Custom("version".into()), vec!["1".to_string()]),
            ];

            // Add h tag for group tier (NIP-29)
            if let Some(group_id) = mem.tier.strip_prefix("group:") {
                tags.push(nostr_sdk::Tag::custom(nostr_sdk::TagKind::Custom("h".into()), vec![group_id.to_string()]));
            }

            let builder = nostr_sdk::EventBuilder::new(
                nostr_sdk::Kind::Custom(crate::kinds::MEMORY_KIND),
                final_content,
            ).tags(tags);

            if let Err(e) = relay.publish(builder).await {
                tracing::warn!("Failed to publish memory to relay: {e}");
            }
        }

        Ok(d_tag)
    }

    /// Store a new memory using explicit db/embedder handles.
    ///
    /// This is for use by MCP, Context-VM, and other code that doesn't have
    /// a full Nomen instance but does have db + embedder references.
    pub async fn store_direct(
        db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
        embedder: &dyn Embedder,
        mem: NewMemory,
    ) -> Result<String> {
        let d_tag = mem.topic.clone();
        let source = mem.source.as_deref().unwrap_or("api");
        let model = mem.model.as_deref().unwrap_or("nomen/api");
        let detail_text = if mem.detail.is_empty() { &mem.summary } else { &mem.detail };
        let content = serde_json::json!({
            "summary": mem.summary,
            "detail": detail_text,
        });

        let parsed = memory::ParsedMemory {
            tier: mem.tier,
            topic: mem.topic,
            version: "1".to_string(),
            confidence: format!("{:.2}", mem.confidence),
            model: model.to_string(),
            summary: mem.summary.clone(),
            created_at: nostr_sdk::Timestamp::now(),
            d_tag: d_tag.clone(),
            source: source.to_string(),
            content_raw: content.to_string(),
            detail: detail_text.to_string(),
        };

        db::store_memory_direct(db, &parsed, source).await?;

        // Generate embedding if embedder is configured
        if embedder.dimensions() > 0 {
            let text = format!("{} {}", parsed.summary, parsed.detail);
            if let Ok(embeddings) = embedder.embed(&[text]).await {
                if let Some(embedding) = embeddings.into_iter().next() {
                    let _ = db::store_embedding(db, &d_tag, embedding).await;
                }
            }
        }

        Ok(d_tag)
    }

    /// Ingest a raw message for later consolidation.
    pub async fn ingest_message(&self, msg: RawMessage) -> Result<String> {
        ingest::ingest_message(&self.db, &msg).await
    }

    /// Run the consolidation pipeline on unconsolidated messages.
    pub async fn consolidate(&self, opts: ConsolidateOptions) -> Result<ConsolidationReport> {
        let author_pubkey = self.relay.as_ref().map(|r| r.keys().public_key().to_hex());
        let config = ConsolidationConfig {
            batch_size: opts.batch_size,
            min_messages: opts.min_messages,
            llm_provider: opts.llm_provider.unwrap_or_else(|| Box::new(NoopLlmProvider)),
            author_pubkey,
            ..Default::default()
        };
        consolidate::consolidate(&self.db, self.embedder.as_ref(), &config, self.relay.as_ref()).await
    }

    /// Query raw messages with filters.
    pub async fn get_messages(&self, opts: MessageQuery) -> Result<Vec<RawMessageRecord>> {
        ingest::get_messages(&self.db, &opts).await
    }

    /// List extracted entities, optionally filtered by kind.
    pub async fn entities(&self, kind: Option<&str>) -> Result<Vec<EntityRecord>> {
        let kind = kind.and_then(EntityKind::from_str);
        db::list_entities(&self.db, kind.as_ref()).await
    }

    /// Delete a memory by topic or event ID.
    pub async fn delete(&self, topic: Option<&str>, id: Option<&str>) -> Result<()> {
        if let Some(topic) = topic {
            db::delete_memory_by_dtag(&self.db, topic).await?;
        }
        if let Some(id) = id {
            db::delete_memory_by_nostr_id(&self.db, id).await?;
        }
        Ok(())
    }

    /// Resolve a session ID to tier/scope/channel using the loaded groups.
    pub fn resolve_session(
        &self,
        session_id: &str,
        default_channel: &str,
    ) -> Result<session::ResolvedSession> {
        session::resolve_session(session_id, &self.groups, default_channel)
    }

    /// Send a message via relay.
    pub async fn send(&self, opts: send::SendOptions) -> Result<send::SendResult> {
        let relay = self
            .relay
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No relay configured for sending"))?;
        send::send_message(relay, &self.db, &self.groups, opts).await
    }
}
