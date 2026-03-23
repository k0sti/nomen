#![recursion_limit = "256"]
//! Nomen — Nostr-native agent memory library.
//!
//! Provides hybrid search (vector + BM25), group-scoped access control,
//! message ingestion, consolidation, and Nostr relay sync backed by SurrealDB.

pub mod access;
pub mod api;
pub mod cluster;
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
pub mod signer;

#[cfg(feature = "migrate")]
pub mod migrate;

// Binary-only modules — not part of the public library API.
#[doc(hidden)]
pub mod cvm;
#[doc(hidden)]
pub mod display;
#[doc(hidden)]
pub mod http;
#[doc(hidden)]
pub mod mcp;
#[doc(hidden)]
pub mod socket;

// Split impl Nomen blocks
mod nomen_memory;
mod nomen_message;
mod nomen_sync;
mod nomen_consolidate;
mod nomen_admin;

use anyhow::Result;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use tokio::sync::broadcast;

use std::sync::Arc;

use crate::config::{Config, ConfigExt};
use crate::embed::Embedder;
use crate::groups::{GroupStore, GroupStoreExt};
use crate::relay::RelayManager;
use crate::signer::NomenSigner;

/// High-level handle wrapping SurrealDB, embedder, relay, and groups.
pub struct Nomen {
    pub(crate) db: Surreal<Db>,
    pub(crate) embedder: Box<dyn Embedder>,
    pub(crate) relay: Option<RelayManager>,
    pub(crate) groups: GroupStore,
    pub(crate) signer: Option<Arc<dyn NomenSigner>>,
    pub(crate) event_tx: Option<broadcast::Sender<nomen_wire::Event>>,
}

/// Options for creating a new memory directly (without relay event).
pub struct NewMemory {
    pub topic: String,
    /// Plain-text content (the full memory body).
    pub content: String,
    pub tier: String,
    /// Importance score (1-10). Optional.
    pub importance: Option<i32>,
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
    pub entity_extractor: Option<Box<dyn entities::EntityExtractor>>,
}

impl Default for ConsolidateOptions {
    fn default() -> Self {
        Self {
            batch_size: 50,
            min_messages: 3,
            llm_provider: None,
            entity_extractor: None,
        }
    }
}

/// Options for cluster fusion.
pub struct ClusterOptions {
    pub min_members: usize,
    pub namespace_depth: usize,
    pub llm_provider: Option<Box<dyn cluster::ClusterLlmProvider>>,
    pub dry_run: bool,
    pub prefix_filter: Option<String>,
}

impl Default for ClusterOptions {
    fn default() -> Self {
        Self {
            min_members: 3,
            namespace_depth: 2,
            llm_provider: None,
            dry_run: false,
            prefix_filter: None,
        }
    }
}

impl Nomen {
    /// Create a Nomen instance from an existing SurrealDB handle.
    pub fn from_db(db: Surreal<Db>) -> Self {
        Self {
            db,
            embedder: Box::new(embed::NoopEmbedder),
            relay: None,
            groups: GroupStore::empty(),
            signer: None,
            event_tx: None,
        }
    }

    /// Open a Nomen instance from a [`Config`].
    pub async fn open(config: &Config) -> Result<Self> {
        let db = db::init_db_with_dimensions(config.embedding_dimensions()).await?;
        Self::open_with_db(config, db).await
    }

    /// Open with a pre-existing DB handle.
    pub async fn open_with_db(config: &Config, db: surrealdb::Surreal<surrealdb::engine::local::Db>) -> Result<Self> {
        let embedder = config.build_embedder();
        let groups = GroupStore::load(&config.groups, &db).await?;
        let signer = config.build_signer();

        Ok(Self {
            db,
            embedder,
            relay: None,
            groups,
            signer,
            event_tx: None,
        })
    }

    /// Open with a pre-existing DB and relay manager.
    pub async fn open_with_db_and_relay(config: &Config, db: surrealdb::Surreal<surrealdb::engine::local::Db>, relay: RelayManager) -> Result<Self> {
        let mut nomen = Self::open_with_db(config, db).await?;
        if nomen.signer.is_none() {
            nomen.signer = Some(relay.arc_signer().clone());
        }
        nomen.relay = Some(relay);
        Ok(nomen)
    }

    /// Open with an explicit relay manager.
    pub async fn open_with_relay(config: &Config, relay: RelayManager) -> Result<Self> {
        let mut nomen = Self::open(config).await?;
        if nomen.signer.is_none() {
            nomen.signer = Some(relay.arc_signer().clone());
        }
        nomen.relay = Some(relay);
        Ok(nomen)
    }

    /// Get the signer, if available.
    pub fn signer(&self) -> Option<&Arc<dyn NomenSigner>> {
        self.signer.as_ref()
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

    /// Set the event emitter for push notifications (used by socket server).
    pub fn set_event_emitter(&mut self, tx: broadcast::Sender<nomen_wire::Event>) {
        self.event_tx = Some(tx);
    }

    /// Emit a push event if an event emitter is configured.
    pub(crate) fn emit_event(&self, event_type: &str, data: serde_json::Value) {
        if let Some(ref tx) = self.event_tx {
            let event = nomen_wire::Event {
                event: event_type.to_string(),
                ts: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                data,
            };
            let _ = tx.send(event);
        }
    }
}

// ── Report structs ──────────────────────────────────────────────────

/// Report from a sync operation.
pub struct SyncReport {
    pub stored: usize,
    pub skipped: usize,
    pub errors: usize,
}

/// Report from an embed operation.
pub struct EmbedReport {
    pub embedded: usize,
    pub total: usize,
}

/// Options for listing memories.
pub struct ListOptions {
    pub tier: Option<String>,
    pub limit: usize,
    pub include_stats: bool,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            tier: None,
            limit: 100,
            include_stats: false,
        }
    }
}

/// Report from a list operation.
pub struct ListReport {
    pub memories: Vec<db::MemoryRecord>,
    pub stats: Option<ListStats>,
}

/// Memory statistics.
pub struct ListStats {
    pub total: usize,
    pub named: usize,
    pub pending: usize,
}
