#![recursion_limit = "256"]
//! Nomen — Nostr-native agent memory library.
//!
//! Provides hybrid search (vector + BM25), group-scoped access control,
//! message ingestion, consolidation, and Nostr relay sync backed by SurrealDB.

pub mod access;
pub mod api;
pub mod auth;
pub mod cluster;
pub mod config;
pub mod consolidate;
pub mod db;
pub mod embed;
pub mod entities;
pub mod fs;
pub mod groups;
pub mod ingest;
pub mod kinds;
pub mod memory;
pub mod relay;
pub mod search;
pub mod send;
pub mod session;
pub mod signer;
// Legacy tools module removed — all operations now go through api::dispatch.
// pub mod tools;

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

use anyhow::Result;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use tokio::sync::broadcast;

use std::sync::Arc;

use crate::cluster::{ClusterConfig, ClusterReport, NoopClusterLlmProvider};
use crate::config::Config;
use crate::consolidate::{ConsolidationConfig, ConsolidationReport, NoopLlmProvider};
use crate::embed::Embedder;
use crate::entities::{EntityKind, EntityRecord};
use crate::groups::GroupStore;
use crate::ingest::{MessageQuery, RawMessage, RawMessageRecord, RawMessageSearchResult};
use crate::relay::RelayManager;
use crate::search::{SearchOptions, SearchResult};
use crate::signer::NomenSigner;

/// High-level handle wrapping SurrealDB, embedder, relay, and groups.
pub struct Nomen {
    db: Surreal<Db>,
    embedder: Box<dyn Embedder>,
    relay: Option<RelayManager>,
    groups: GroupStore,
    signer: Option<Arc<dyn NomenSigner>>,
    event_tx: Option<broadcast::Sender<nomen_wire::Event>>,
}

/// Options for creating a new memory directly (without relay event).
pub struct NewMemory {
    pub topic: String,
    pub detail: String,
    pub visibility: String,
    /// Scope/context for d-tag construction (e.g. group ID, pubkey hex).
    /// If empty, derived from visibility.
    pub scope: String,
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
    pub dry_run: bool,
    pub older_than: Option<String>,
    pub tier: Option<String>,
}

impl Default for ConsolidateOptions {
    fn default() -> Self {
        Self {
            batch_size: 50,
            min_messages: 3,
            llm_provider: None,
            entity_extractor: None,
            dry_run: false,
            older_than: None,
            tier: None,
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
    ///
    /// Initialises SurrealDB, builds the embedder, and loads groups.
    /// Does **not** connect to any relay — call methods on [`RelayManager`]
    /// separately if relay interaction is needed.
    pub async fn open(config: &Config) -> Result<Self> {
        let db = db::init_db_with_dimensions(config.embedding_dimensions()).await?;
        Self::open_with_db(config, db).await
    }

    /// Open with a pre-existing DB handle (avoids SurrealDB 3.x exclusive lock conflicts).
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

    /// Open with an explicit relay manager (already connected or not).
    pub async fn open_with_relay(config: &Config, relay: RelayManager) -> Result<Self> {
        let mut nomen = Self::open(config).await?;
        // Use relay's signer if no signer was built from config
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
    fn emit_event(&self, event_type: &str, data: serde_json::Value) {
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

    /// Hybrid (vector + full-text) search over stored memories.
    pub async fn search(&self, opts: SearchOptions) -> Result<Vec<SearchResult>> {
        search::search(&self.db, self.embedder.as_ref(), &opts).await
    }

    /// Store a new memory into SurrealDB and optionally publish to relay.
    ///
    /// Includes supersedes logic: if a memory with the same topic already exists
    /// in the local DB, the new event will carry a `supersedes` tag and incremented
    /// version number. For personal/internal tier, content is NIP-44 encrypted
    /// before relay publish.
    pub async fn store(&self, mem: NewMemory) -> Result<String> {
        let author_pubkey_hex = self
            .signer
            .as_ref()
            .map(|s| s.public_key().to_hex())
            .unwrap_or_default();

        let base_tier = memory::base_tier(&mem.visibility);
        let context = if !mem.scope.is_empty() {
            mem.scope.clone()
        } else if base_tier == "personal" || base_tier == "internal" {
            author_pubkey_hex.clone()
        } else if let Some(group_id) = mem.visibility.strip_prefix("group:") {
            group_id.to_string()
        } else {
            String::new()
        };
        let d_tag = memory::build_v2_dtag(base_tier, &context, &mem.topic);

        let source = mem.source.as_deref().unwrap_or("api");
        let model = mem.model.as_deref().unwrap_or("nomen/api");
        let content_str = mem.detail.clone();

        // Supersedes logic: check for existing memory with same topic
        let (version, previous_nostr_id) =
            match db::get_memory_by_topic(&self.db, &mem.topic).await? {
                Some(existing) => {
                    let new_version = existing.version + 1;
                    (new_version, existing.nostr_id)
                }
                None => {
                    // Also check by d_tag
                    match db::get_memory_by_dtag(&self.db, &d_tag).await? {
                        Some(existing) => {
                            let new_version = existing.version + 1;
                            (new_version, existing.nostr_id)
                        }
                        None => (1, None),
                    }
                }
            };

        let parsed = memory::ParsedMemory {
            visibility: mem.visibility.clone(),
            topic: mem.topic.clone(),
            version: version.to_string(),
            model: model.to_string(),
            created_at: nostr_sdk::Timestamp::now(),
            d_tag: d_tag.clone(),
            source: source.to_string(),
            content_raw: content_str.clone(),
            detail: mem.detail.clone(),
        };

        db::store_memory_direct(&self.db, &parsed, source).await?;

        // Generate embedding if embedder is configured
        if self.embedder.dimensions() > 0 {
            let text = parsed.detail.clone();
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
                relay.signer().encrypt(&content_str).unwrap_or(content_str)
            } else {
                content_str
            };

            let mut tags = vec![
                nostr_sdk::Tag::custom(nostr_sdk::TagKind::Custom("d".into()), vec![d_tag.clone()]),
                nostr_sdk::Tag::custom(
                    nostr_sdk::TagKind::Custom("visibility".into()),
                    vec![base_tier.to_string()],
                ),
                nostr_sdk::Tag::custom(
                    nostr_sdk::TagKind::Custom("scope".into()),
                    vec![context.clone()],
                ),
                nostr_sdk::Tag::custom(
                    nostr_sdk::TagKind::Custom("model".into()),
                    vec![model.to_string()],
                ),
                nostr_sdk::Tag::custom(
                    nostr_sdk::TagKind::Custom("version".into()),
                    vec![version.to_string()],
                ),
            ];

            // Add supersedes tag if updating an existing memory
            if let Some(ref prev_id) = previous_nostr_id {
                if !prev_id.is_empty() {
                    tags.push(nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom("supersedes".into()),
                        vec![prev_id.clone()],
                    ));
                }
            }

            // Add topic tags for relay-side filtering
            for part in mem.topic.split('/') {
                if !part.is_empty() {
                    tags.push(nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom("t".into()),
                        vec![part.to_string()],
                    ));
                }
            }

            // Add h tag for group tier (NIP-29)
            if let Some(group_id) = mem.visibility.strip_prefix("group:") {
                tags.push(nostr_sdk::Tag::custom(
                    nostr_sdk::TagKind::Custom("h".into()),
                    vec![group_id.to_string()],
                ));
            }

            let builder = nostr_sdk::EventBuilder::new(
                nostr_sdk::Kind::Custom(crate::kinds::MEMORY_KIND),
                final_content,
            )
            .tags(tags);

            if let Err(e) = relay.publish(builder).await {
                tracing::warn!("Failed to publish memory to relay: {e}");
            }
        }

        self.emit_event("memory.updated", serde_json::json!({
            "topic": mem.topic,
            "visibility": base_tier,
            "scope": context,
            "author": author_pubkey_hex,
            "source": source,
        }));

        Ok(d_tag)
    }

    /// Store a new memory using explicit db/embedder handles.
    ///
    /// This is for use by MCP, Context-VM, and other code that doesn't have
    /// a full Nomen instance but does have db + embedder references.
    /// `author_pubkey_hex` is used for personal/internal tier d-tag scoping;
    /// pass empty string if no signer is available.
    pub async fn store_direct(
        db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
        embedder: &dyn Embedder,
        mem: NewMemory,
    ) -> Result<String> {
        Self::store_direct_with_author(db, embedder, mem, "").await
    }

    /// Store a new memory with explicit author pubkey for d-tag construction.
    pub async fn store_direct_with_author(
        db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
        embedder: &dyn Embedder,
        mem: NewMemory,
        author_pubkey_hex: &str,
    ) -> Result<String> {
        let base_tier = memory::base_tier(&mem.visibility);
        let context = if !mem.scope.is_empty() {
            mem.scope.clone()
        } else if base_tier == "personal" || base_tier == "internal" {
            author_pubkey_hex.to_string()
        } else if let Some(group_id) = mem.visibility.strip_prefix("group:") {
            group_id.to_string()
        } else {
            String::new()
        };
        let d_tag = memory::build_v2_dtag(base_tier, &context, &mem.topic);
        let source = mem.source.as_deref().unwrap_or("api");
        let model = mem.model.as_deref().unwrap_or("nomen/api");
        let content_str = mem.detail.clone();

        let parsed = memory::ParsedMemory {
            visibility: mem.visibility,
            topic: mem.topic,
            version: "1".to_string(),
            model: model.to_string(),
            created_at: nostr_sdk::Timestamp::now(),
            d_tag: d_tag.clone(),
            source: source.to_string(),
            content_raw: content_str,
            detail: mem.detail,
        };

        db::store_memory_direct(db, &parsed, source).await?;

        // Generate embedding if embedder is configured
        if embedder.dimensions() > 0 {
            let text = parsed.detail.clone();
            if let Ok(embeddings) = embedder.embed(&[text]).await {
                if let Some(embedding) = embeddings.into_iter().next() {
                    let _ = db::store_embedding(db, &d_tag, embedding).await;
                }
            }
        }

        Ok(d_tag)
    }

    /// Ingest a raw message for later consolidation.
    ///
    /// Stores the message locally and publishes it to the relay as a kind 1235
    /// raw source event if a relay is configured. On successful publish, the
    /// local row is updated with the relay event ID and `publish_status = "published"`.
    /// On relay failure, `publish_status` is set to `"failed"`.
    pub async fn ingest_message(&self, msg: RawMessage) -> Result<String> {
        let id = ingest::ingest_message(&self.db, &msg).await?;
        if id == "duplicate" {
            return Ok(id);
        }

        // Publish to relay as kind 1235 raw source event
        if let Some(ref relay) = self.relay {
            let builder = ingest::build_raw_source_event(&msg);
            match relay.publish(builder).await {
                Ok(result) => {
                    tracing::debug!(
                        event_id = %result.event_id,
                        "Published raw source event to relay"
                    );
                    db::update_raw_message_publish(
                        &self.db,
                        &id,
                        &result.event_id.to_hex(),
                        "published",
                    )
                    .await?;
                }
                Err(e) => {
                    tracing::warn!("Failed to publish raw source event: {e}");
                    db::update_raw_message_publish(&self.db, &id, "", "failed").await?;
                }
            }
        }

        self.emit_event(
            "message.ingested",
            serde_json::json!({
                "id": id,
                "source": msg.source,
                "sender": msg.sender,
                "channel": msg.channel,
            }),
        );

        Ok(id)
    }

    /// Run the consolidation pipeline on unconsolidated messages.
    pub async fn consolidate(&self, opts: ConsolidateOptions) -> Result<ConsolidationReport> {
        let author_pubkey = self.signer.as_ref().map(|s| s.public_key().to_hex());
        let config = ConsolidationConfig {
            batch_size: opts.batch_size,
            min_messages: opts.min_messages,
            llm_provider: opts
                .llm_provider
                .unwrap_or_else(|| Box::new(NoopLlmProvider)),
            entity_extractor: opts
                .entity_extractor
                .unwrap_or_else(|| Box::new(entities::HeuristicExtractor)),
            author_pubkey,
            dry_run: opts.dry_run,
            older_than: opts.older_than,
            tier: opts.tier,
            ..Default::default()
        };
        let report = consolidate::consolidate(
            &self.db,
            self.embedder.as_ref(),
            &config,
            self.relay.as_ref(),
        )
        .await?;

        self.emit_event("consolidation.complete", serde_json::json!({
            "memories_created": report.memories_created,
            "messages_processed": report.messages_processed,
        }));

        Ok(report)
    }

    /// Two-phase consolidation: prepare (stages 1-2).
    pub async fn consolidate_prepare(&self, opts: ConsolidateOptions, ttl_minutes: u32) -> Result<consolidate::PrepareResult> {
        let author_pubkey = self.signer.as_ref().map(|s| s.public_key().to_hex());
        let config = ConsolidationConfig {
            batch_size: opts.batch_size,
            min_messages: opts.min_messages,
            llm_provider: Box::new(NoopLlmProvider), // not used in prepare
            entity_extractor: Box::new(entities::HeuristicExtractor),
            author_pubkey,
            ..Default::default()
        };
        consolidate::prepare(&self.db, &config, ttl_minutes).await
    }

    /// Two-phase consolidation: commit (stages 4-6).
    pub async fn consolidate_commit(
        &self,
        session_id: &str,
        extractions: &[consolidate::BatchExtraction],
    ) -> Result<consolidate::CommitResult> {
        let author_pubkey = self.signer.as_ref().map(|s| s.public_key().to_hex());
        let config = ConsolidationConfig {
            author_pubkey,
            entity_extractor: Box::new(entities::HeuristicExtractor),
            ..Default::default()
        };
        let result = consolidate::commit(
            &self.db,
            self.embedder.as_ref(),
            &config,
            self.relay.as_ref(),
            session_id,
            extractions,
        )
        .await?;

        self.emit_event("consolidation.complete", serde_json::json!({
            "memories_created": result.memories_created,
            "messages_consolidated": result.messages_consolidated,
            "session_id": result.session_id,
        }));

        Ok(result)
    }

    /// Run the cluster fusion pipeline on named memories.
    pub async fn cluster_fusion(&self, opts: ClusterOptions) -> Result<ClusterReport> {
        let author_pubkey = self.signer.as_ref().map(|s| s.public_key().to_hex());
        let config = ClusterConfig {
            min_members: opts.min_members,
            namespace_depth: opts.namespace_depth,
            llm_provider: opts
                .llm_provider
                .unwrap_or_else(|| Box::new(NoopClusterLlmProvider)),
            dry_run: opts.dry_run,
            prefix_filter: opts.prefix_filter,
            author_pubkey,
        };
        let report = cluster::run_cluster_fusion(
            &self.db,
            self.embedder.as_ref(),
            &config,
            self.relay.as_ref(),
        )
        .await?;

        self.emit_event("cluster.fused", serde_json::json!({
            "clusters_merged": report.clusters_found,
            "dry_run": report.dry_run,
        }));

        Ok(report)
    }

    /// Query raw messages with filters.
    pub async fn get_messages(&self, opts: MessageQuery) -> Result<Vec<RawMessageRecord>> {
        ingest::get_messages(&self.db, &opts).await
    }

    /// Full-text BM25 search over raw messages.
    pub async fn search_messages(
        &self,
        query: &str,
        source: Option<&str>,
        room: Option<&str>,
        topic: Option<&str>,
        sender: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        include_consolidated: bool,
        limit: usize,
    ) -> Result<Vec<RawMessageSearchResult>> {
        db::search_raw_messages(
            &self.db,
            query,
            source,
            room,
            topic,
            sender,
            since,
            until,
            include_consolidated,
            limit,
        )
        .await
    }

    /// Get multiple memories by d-tags in a single query.
    pub async fn get_batch(&self, d_tags: &[String]) -> Result<Vec<db::MemoryRecord>> {
        db::get_memories_by_dtags(&self.db, d_tags).await
    }

    /// List extracted entities, optionally filtered by kind.
    pub async fn entities(&self, kind: Option<&str>) -> Result<Vec<EntityRecord>> {
        let kind = kind.and_then(EntityKind::from_str);
        db::list_entities(&self.db, kind.as_ref()).await
    }

    /// List entity relationships, optionally filtered by entity name.
    pub async fn entity_relationships(
        &self,
        entity_name: Option<&str>,
    ) -> Result<Vec<entities::RelationshipRecord>> {
        db::list_entity_relationships(&self.db, entity_name).await
    }

    /// Delete a memory by topic or event ID.
    ///
    /// If a relay is connected, also publishes a NIP-09 deletion event (kind 5)
    /// to remove the event from the relay.
    pub async fn delete(&self, topic: Option<&str>, id: Option<&str>) -> Result<()> {
        // Find the nostr event ID for relay deletion
        let mut nostr_event_id: Option<String> = None;

        if let Some(topic) = topic {
            // Try to find the event ID before deleting locally
            if let Ok(Some(record)) = db::get_memory_by_topic(&self.db, topic).await {
                nostr_event_id = record.nostr_id.clone();
            }
            // Also try by d_tag
            if nostr_event_id.is_none() {
                if let Ok(Some(record)) = db::get_memory_by_dtag(&self.db, topic).await {
                    nostr_event_id = record.nostr_id.clone();
                }
            }
            db::delete_memory_by_dtag(&self.db, topic).await?;
            db::delete_memory_by_topic(&self.db, topic).await?;
        }
        if let Some(id) = id {
            nostr_event_id = Some(id.to_string());
            db::delete_memory_by_nostr_id(&self.db, id).await?;
        }

        // Publish NIP-09 deletion to relay if available
        if let (Some(ref relay), Some(ref event_id_hex)) = (&self.relay, &nostr_event_id) {
            if !event_id_hex.is_empty() {
                if let Ok(eid) = nostr_sdk::EventId::from_hex(event_id_hex) {
                    let delete_builder =
                        nostr_sdk::EventBuilder::new(nostr_sdk::Kind::Custom(5), "")
                            .tags(vec![nostr_sdk::Tag::event(eid)]);

                    if let Err(e) = relay.publish(delete_builder).await {
                        tracing::warn!("Failed to publish NIP-09 deletion to relay: {e}");
                    }
                }
            }
        }

        let deleted_topic = topic.or(id).unwrap_or_default();
        self.emit_event("memory.deleted", serde_json::json!({
            "topic": deleted_topic,
            "d_tag": topic.unwrap_or_default(),
            "author": self.signer.as_ref().map(|s| s.public_key().to_hex()).unwrap_or_default(),
        }));

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

    /// List memories, optionally filtered by tier.
    pub async fn list_memories(
        &self,
        tier: Option<&str>,
        limit: usize,
        pinned: Option<bool>,
    ) -> Result<Vec<db::MemoryRecord>> {
        db::list_memories(&self.db, tier, limit, pinned).await
    }

    /// Count memories: returns (total, named, pending_raw_messages).
    pub async fn count_memories(&self) -> Result<(usize, usize, usize)> {
        db::count_memories_by_type(&self.db).await
    }

    /// Get a memory by its d_tag.
    pub async fn get_by_topic(&self, d_tag: &str) -> Result<Option<db::MemoryRecord>> {
        db::get_memory_by_dtag(&self.db, d_tag).await
    }

    /// Get a memory by raw topic string (queries the `topic` field, not `d_tag`).
    pub async fn get_by_raw_topic(&self, topic: &str) -> Result<Option<db::MemoryRecord>> {
        db::get_memory_by_topic(&self.db, topic).await
    }

    /// Delete a memory by raw topic string (queries the `topic` field, not `d_tag`).
    pub async fn delete_by_topic(&self, topic: &str) -> Result<()> {
        db::delete_memory_by_topic(&self.db, topic).await
    }

    /// Send a message via relay.
    pub async fn send(&self, opts: send::SendOptions) -> Result<send::SendResult> {
        let relay = self
            .relay
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No relay configured for sending"))?;
        send::send_message(relay, &self.db, &self.groups, opts).await
    }

    // ── New unified methods ─────────────────────────────────────

    /// Sync memories from relay to local DB.
    ///
    /// Fetches all memory events from the relay for the configured signer's pubkeys
    /// and stores them locally. Returns a report of what was synced.
    pub async fn sync(&self) -> Result<SyncReport> {
        let relay = self
            .relay
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No relay configured for sync"))?;
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No signer configured for sync"))?;

        let pubkeys = vec![signer.public_key()];
        let events = relay.fetch_memories(&pubkeys).await?;

        let mut stored = 0usize;
        let mut skipped = 0usize;
        let mut errors = 0usize;

        for event in events.into_iter() {
            // Skip legacy lesson events (kind 31235 and 4129) — migrated to kind 31234
            // Skip legacy NIP-78 events (kind 30078) — obsolete
            if event.kind == nostr_sdk::Kind::Custom(31235)
                || event.kind == nostr_sdk::Kind::Custom(4129)
                || event.kind == nostr_sdk::Kind::Custom(kinds::LEGACY_APP_DATA_KIND)
            {
                continue;
            }

            // Handle kind 1235 raw source events — import as raw messages
            if event.kind == nostr_sdk::Kind::Custom(kinds::RAW_SOURCE_KIND) {
                let nostr_id = event.id.to_hex();
                match db::check_duplicate_by_nostr_id(&self.db, &nostr_id).await {
                    Ok(true) => {
                        skipped += 1;
                    }
                    Ok(false) => {
                        let raw_msg = ingest::parse_raw_source_event(&event);
                        match db::store_raw_message_from_relay(&self.db, &raw_msg, &nostr_id).await
                        {
                            Ok(_) => stored += 1,
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to import raw source event {}: {e}",
                                    nostr_id
                                );
                                errors += 1;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to check duplicate for raw source event {}: {e}",
                            nostr_id
                        );
                        errors += 1;
                    }
                }
                continue;
            }

            let d_tag = memory::get_tag_value(&event.tags, "d").unwrap_or_default();
            if d_tag.starts_with("snowclaw:config:") {
                continue;
            }

            let parsed = memory::parse_event(&event, signer.as_ref());
            match db::store_memory(&self.db, &parsed, &event).await {
                Ok(true) => stored += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    tracing::warn!("Failed to store memory {}: {e}", parsed.topic);
                    errors += 1;
                }
            }
        }

        self.emit_event("sync.complete", serde_json::json!({
            "stored": stored,
            "skipped": skipped,
            "errors": errors,
        }));

        Ok(SyncReport {
            stored,
            skipped,
            errors,
        })
    }

    /// Generate embeddings for memories that lack them.
    pub async fn embed(&self, limit: usize) -> Result<EmbedReport> {
        if self.embedder.dimensions() == 0 {
            anyhow::bail!("No embedding provider configured");
        }

        let missing = db::get_memories_without_embeddings(&self.db, limit).await?;
        let total = missing.len();

        if missing.is_empty() {
            return Ok(EmbedReport {
                embedded: 0,
                total: 0,
            });
        }

        let texts: Vec<String> = missing
            .iter()
            .map(|m| m.search_text.clone())
            .collect();

        let embeddings = self.embedder.embed(&texts).await?;
        let mut embedded = 0usize;

        for (row, embedding) in missing.iter().zip(embeddings.into_iter()) {
            if let Some(ref d_tag) = row.d_tag {
                db::store_embedding(&self.db, d_tag, embedding).await?;
                embedded += 1;
            }
        }

        Ok(EmbedReport { embedded, total })
    }

    /// Prune old/unused memories and consolidated raw messages.
    pub async fn prune(&self, days: u64, dry_run: bool) -> Result<db::PruneReport> {
        db::prune_memories(&self.db, days, dry_run).await
    }

    /// List memories from local DB with optional filters.
    pub async fn list(&self, opts: ListOptions) -> Result<ListReport> {
        let memories = db::list_memories(&self.db, opts.tier.as_deref(), opts.limit, opts.pinned).await?;
        let stats = if opts.include_stats {
            let (total, named, pending) = db::count_memories_by_type(&self.db).await?;
            let detailed = db::get_detailed_stats(&self.db).await.ok();
            Some(ListStats {
                total,
                named,
                pending,
                detailed,
            })
        } else {
            None
        };

        Ok(ListReport { memories, stats })
    }

    /// Delete ephemeral (raw) messages older than a duration string (e.g. "7d", "24h").
    pub async fn delete_ephemeral(&self, older_than: &str) -> Result<usize> {
        let secs = crate::consolidate::parse_duration_str(older_than)?;
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(secs);
        let cutoff_str = cutoff.to_rfc3339();
        db::delete_ephemeral_before(&self.db, &cutoff_str).await
    }

    // ── D-tag migration ────────────────────────────────────────

    /// Migrate all memory d-tags from colon format to slash format.
    /// Returns (migrated_count, skipped_count).
    pub async fn migrate_dtags(&self, dry_run: bool) -> Result<(usize, usize)> {
        let memories = db::list_memories(&self.db, None, 10000, None).await?;
        let mut migrated = 0;
        let mut skipped = 0;

        for mem in &memories {
            let old_dtag = match &mem.d_tag {
                Some(d) if !d.is_empty() => d.clone(),
                _ => {
                    skipped += 1;
                    continue;
                }
            };

            if !memory::is_colon_format(&old_dtag) {
                skipped += 1;
                continue;
            }

            let new_dtag = memory::migrate_dtag_to_slash(&old_dtag);
            if new_dtag == old_dtag {
                skipped += 1;
                continue;
            }

            let (_, new_scope) = memory::extract_visibility_scope(&new_dtag);
            let new_topic = memory::v2_dtag_topic(&new_dtag)
                .unwrap_or(&new_dtag)
                .to_string();

            if dry_run {
                tracing::info!("{old_dtag} → {new_dtag}");
                migrated += 1;
                continue;
            }

            match db::update_memory_dtag(&self.db, &old_dtag, &new_dtag, &new_scope, &new_topic)
                .await
            {
                Ok(true) => {
                    tracing::info!("{old_dtag} → {new_dtag}");
                    migrated += 1;
                }
                Ok(false) => skipped += 1,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("already contains") {
                        // New d_tag exists (from fs push), delete the old duplicate
                        tracing::info!("Deleting duplicate old record: {old_dtag}");
                        if let Err(del_e) = db::delete_memory_by_dtag(&self.db, &old_dtag).await {
                            tracing::warn!("Failed to delete old duplicate {old_dtag}: {del_e}");
                        } else {
                            migrated += 1;
                        }
                    } else {
                        tracing::warn!("Failed to migrate {old_dtag}: {e}");
                        skipped += 1;
                    }
                }
            }
        }

        Ok((migrated, skipped))
    }

    // ── Group management ────────────────────────────────────────

    /// Create a new group.
    pub async fn group_create(
        &self,
        id: &str,
        name: &str,
        members: &[String],
        nostr_group: Option<&str>,
        relay: Option<&str>,
    ) -> Result<()> {
        crate::groups::create_group(&self.db, id, name, members, nostr_group, relay).await
    }

    /// List all groups.
    pub async fn group_list(&self) -> Result<Vec<crate::groups::Group>> {
        crate::groups::list_groups(&self.db).await
    }

    /// Get members of a group.
    pub async fn group_members(&self, group_id: &str) -> Result<Vec<String>> {
        crate::groups::get_members(&self.db, group_id).await
    }

    /// Add a member to a group.
    pub async fn group_add_member(&self, group_id: &str, npub: &str) -> Result<()> {
        crate::groups::add_member(&self.db, group_id, npub).await
    }

    /// Remove a member from a group.
    pub async fn group_remove_member(&self, group_id: &str, npub: &str) -> Result<()> {
        crate::groups::remove_member(&self.db, group_id, npub).await
    }

    /// Publish local DB items that are missing from the relay.
    ///
    /// Reconciles local DB with relay by publishing memories and raw messages
    /// that have not been successfully published yet.
    pub async fn publish(&self, opts: PublishOptions) -> Result<PublishReport> {
        let relay = self
            .relay
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No relay configured for publish"))?;

        let mut published = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;

        // Publish memories (kind 31234)
        if opts.memories {
            let memories = db::get_unpublished_memories(&self.db, opts.limit).await?;
            for mem in &memories {
                if opts.dry_run {
                    skipped += 1;
                    continue;
                }

                let d_tag = match &mem.d_tag {
                    Some(dt) if !dt.is_empty() => dt.clone(),
                    _ => {
                        skipped += 1;
                        continue;
                    }
                };

                let base_tier = memory::base_tier(&mem.visibility);
                let content_str = mem.search_text.clone();

                // NIP-44 encrypt for personal/internal tier
                let final_content = if base_tier == "personal" || base_tier == "internal" {
                    relay.signer().encrypt(&content_str).unwrap_or(content_str)
                } else {
                    content_str
                };

                let mut tags = vec![
                    nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom("d".into()),
                        vec![d_tag.clone()],
                    ),
                    nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom("visibility".into()),
                        vec![base_tier.to_string()],
                    ),
                    nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom("scope".into()),
                        vec![mem.scope.clone()],
                    ),
                    nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom("version".into()),
                        vec![mem.version.to_string()],
                    ),
                ];

                if let Some(ref model) = mem.model {
                    tags.push(nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom("model".into()),
                        vec![model.clone()],
                    ));
                }

                // Add topic tags for relay-side filtering
                for part in mem.topic.split('/') {
                    if !part.is_empty() {
                        tags.push(nostr_sdk::Tag::custom(
                            nostr_sdk::TagKind::Custom("t".into()),
                            vec![part.to_string()],
                        ));
                    }
                }

                // Add h tag for group tier (NIP-29)
                if base_tier == "group" && !mem.scope.is_empty() {
                    tags.push(nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom("h".into()),
                        vec![mem.scope.clone()],
                    ));
                }

                let builder = nostr_sdk::EventBuilder::new(
                    nostr_sdk::Kind::Custom(crate::kinds::MEMORY_KIND),
                    final_content,
                )
                .tags(tags);

                match relay.publish(builder).await {
                    Ok(result) => {
                        let event_id_hex = result.event_id.to_hex();
                        let _ = db::update_memory_nostr_id(&self.db, &d_tag, &event_id_hex).await;
                        published += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to publish memory {}: {e}", d_tag);
                        failed += 1;
                    }
                }
            }
        }

        // Publish raw messages (kind 1235)
        if opts.messages {
            let remaining = opts.limit.saturating_sub(published + failed + skipped);
            if remaining > 0 {
                let messages = db::get_unpublished_messages(&self.db, remaining).await?;
                for msg_record in &messages {
                    if opts.dry_run {
                        skipped += 1;
                        continue;
                    }

                    // Convert RawMessageRecord back to RawMessage for event building
                    let raw_msg = ingest::RawMessage {
                        source: msg_record.source.clone(),
                        source_id: if msg_record.source_id.is_empty() {
                            None
                        } else {
                            Some(msg_record.source_id.clone())
                        },
                        sender: msg_record.sender_id.clone(),
                        channel: if msg_record.channel.is_empty() {
                            None
                        } else {
                            Some(msg_record.channel.clone())
                        },
                        content: msg_record.content.clone(),
                        metadata: if msg_record.metadata.is_empty() {
                            None
                        } else {
                            Some(msg_record.metadata.clone())
                        },
                        created_at: Some(msg_record.created_at.clone()),
                        provider_id: if msg_record.provider_id.is_empty() {
                            None
                        } else {
                            Some(msg_record.provider_id.clone())
                        },
                        sender_name: if msg_record.sender.is_empty() {
                            None
                        } else {
                            Some(msg_record.sender.clone())
                        },
                        room: if msg_record.room.is_empty() {
                            None
                        } else {
                            Some(msg_record.room.clone())
                        },
                        topic: if msg_record.topic.is_empty() {
                            None
                        } else {
                            Some(msg_record.topic.clone())
                        },
                        thread: if msg_record.thread.is_empty() {
                            None
                        } else {
                            Some(msg_record.thread.clone())
                        },
                        scope: if msg_record.scope.is_empty() {
                            None
                        } else {
                            Some(msg_record.scope.clone())
                        },
                        source_ts: if msg_record.source_created_at.is_empty() {
                            None
                        } else {
                            Some(msg_record.source_created_at.clone())
                        },
                    };

                    let builder = ingest::build_raw_source_event(&raw_msg);

                    // Extract the raw_message DB id (strip table prefix if present)
                    let db_id = msg_record
                        .id
                        .strip_prefix("raw_message:")
                        .unwrap_or(&msg_record.id);

                    match relay.publish(builder).await {
                        Ok(result) => {
                            let event_id_hex = result.event_id.to_hex();
                            let _ = db::update_raw_message_publish(
                                &self.db,
                                db_id,
                                &event_id_hex,
                                "published",
                            )
                            .await;
                            published += 1;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to publish raw message {}: {e}", db_id);
                            let _ = db::update_raw_message_publish(
                                &self.db,
                                db_id,
                                "",
                                "failed",
                            )
                            .await;
                            failed += 1;
                        }
                    }
                }
            }
        }

        Ok(PublishReport {
            published,
            failed,
            skipped,
        })
    }
}

/// Options for the publish command.
pub struct PublishOptions {
    /// Publish memories (kind 31234)
    pub memories: bool,
    /// Publish raw messages (kind 1235)
    pub messages: bool,
    /// Dry run (preview only)
    pub dry_run: bool,
    /// Max items to publish
    pub limit: usize,
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            memories: true,
            messages: true,
            dry_run: false,
            limit: 100,
        }
    }
}

// ── Report structs ──────────────────────────────────────────────────

/// Report from a publish operation.
pub struct PublishReport {
    pub published: usize,
    pub failed: usize,
    pub skipped: usize,
}

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
    pub pinned: Option<bool>,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            tier: None,
            limit: 100,
            include_stats: false,
            pinned: None,
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
    pub detailed: Option<db::DetailedStats>,
}
