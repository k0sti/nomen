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
pub mod memory;
pub mod relay;
pub mod search;

#[cfg(feature = "migrate")]
pub mod migrate;

#[cfg(feature = "snowclaw")]
pub mod snowclaw_adapter;

// Binary-only modules — not part of the public library API.
#[doc(hidden)]
pub mod display;
#[doc(hidden)]
pub mod mcp;
#[doc(hidden)]
pub mod contextvm;

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
        let db = db::init_db().await?;
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

    /// Store a new memory directly into SurrealDB (no relay publish).
    ///
    /// Builds the ParsedMemory, stores in DB, and generates embeddings
    /// automatically. This is the single source of truth for local memory
    /// storage — MCP, Context-VM, and CLI all delegate here.
    pub async fn store(&self, mem: NewMemory) -> Result<String> {
        let d_tag = format!("snow:memory:{}", mem.topic);
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
        let d_tag = format!("snow:memory:{}", mem.topic);
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
        let config = ConsolidationConfig {
            batch_size: opts.batch_size,
            min_messages: opts.min_messages,
            llm_provider: opts.llm_provider.unwrap_or_else(|| Box::new(NoopLlmProvider)),
        };
        consolidate::consolidate(&self.db, self.embedder.as_ref(), &config).await
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
            let d_tag = format!("snow:memory:{topic}");
            db::delete_memory_by_dtag(&self.db, &d_tag).await?;
        }
        if let Some(id) = id {
            db::delete_memory_by_nostr_id(&self.db, id).await?;
        }
        Ok(())
    }
}
