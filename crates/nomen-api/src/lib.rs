//! nomen-api — canonical API v2 dispatch layer for MCP and CVM.
//!
//! This crate provides the shared dispatch logic and operation modules
//! that both MCP and CVM transports use. Operations are defined against
//! the [`NomenBackend`] trait, which the main `nomen` crate implements.

pub mod dispatch;
pub mod operations;
pub mod session_backend;
pub mod types;

pub use dispatch::{dispatch, mcp_tool_to_action};
pub use nomen_core::api::errors;
pub use nomen_core::api::types::ApiResponse;
pub use session_backend::SessionBackend;

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use nomen_core::collected::{CollectedEvent, CollectedEventFilter};
use nomen_core::groups::Group;
use nomen_core::ops::{
    ClusterParams, ConsolidateParams, EmbedReport, ListOptions, ListStats, SyncReport,
};
use nomen_core::search::{SearchOptions, SearchResult};
use nomen_core::send::{SendOptions, SendResult};
use nomen_core::signer::NomenSigner;
use nomen_core::NewMemory;
use nomen_db::{CollectedMessageRecord, CollectedSearchResult, MemoryRecord, PruneReport};
use nomen_llm::cluster::ClusterReport;
use nomen_llm::consolidate::{BatchExtraction, CommitResult, ConsolidationReport, PrepareResult};
use nomen_media::MediaRef;

/// Trait abstracting the Nomen backend for API operations.
///
/// The main `nomen` crate implements this on its `Nomen` struct.
/// This allows the API layer to live in a separate crate without
/// depending on the full `nomen` crate.
#[async_trait]
pub trait NomenBackend: Send + Sync {
    // -- Memory --

    /// Hybrid search over stored memories.
    async fn search(&self, opts: SearchOptions) -> Result<Vec<SearchResult>>;

    /// Store a new memory; returns the d_tag.
    async fn store(&self, mem: NewMemory) -> Result<String>;

    /// Get a memory by d_tag.
    async fn get_by_topic(&self, d_tag: &str) -> Result<Option<MemoryRecord>>;

    /// Get a memory by raw topic (fallback lookup).
    async fn get_by_raw_topic(&self, topic: &str) -> Result<Option<MemoryRecord>>;

    /// Delete a memory by topic/d_tag or event id.
    async fn delete(&self, topic: Option<&str>, id: Option<&str>) -> Result<()>;

    /// List memories with options.
    async fn list(&self, opts: ListOptions) -> Result<ListReport>;

    // -- Messages --

    /// Send a message via relay.
    async fn send(&self, opts: SendOptions) -> Result<SendResult>;

    /// Store a kind 30100 collected event.
    async fn store_collected_event(
        &self,
        event: CollectedEvent,
    ) -> Result<nomen_db::collected::StoreResult>;

    /// Query collected events with tag-based filtering.
    async fn query_collected_events(
        &self,
        filter: CollectedEventFilter,
    ) -> Result<Vec<CollectedMessageRecord>>;

    /// BM25 fulltext search over collected messages.
    async fn search_collected_events(
        &self,
        query: &str,
        filter: CollectedEventFilter,
    ) -> Result<Vec<CollectedSearchResult>>;

    // -- Media --

    /// Upload media to the configured media store.
    /// Returns None if no media store is configured.
    async fn store_media(&self, data: &[u8], mime_type: &str) -> Result<Option<MediaRef>>;

    // -- Session --

    /// Resolve a session ID to tier/scope/delivery-channel.

    // -- Entities (entity = memory with type=entity:*) --

    /// List entity memories, optionally filtered by type (e.g. "entity:person").
    async fn entity_memories(&self, type_filter: Option<&str>) -> Result<Vec<MemoryRecord>>;

    /// List references edges for an entity memory (by d_tag), returns JSON values.
    async fn entity_relationships(&self, d_tag: Option<&str>) -> Result<Vec<serde_json::Value>>;

    // -- Groups --

    /// List all groups.
    async fn group_list(&self) -> Result<Vec<Group>>;

    /// Get members of a group.
    async fn group_members(&self, group_id: &str) -> Result<Vec<String>>;

    /// Create a new group.
    async fn group_create(
        &self,
        id: &str,
        name: &str,
        members: &[String],
        nostr_group: Option<&str>,
        relay: Option<&str>,
    ) -> Result<()>;

    /// Add a member to a group.
    async fn group_add_member(&self, group_id: &str, npub: &str) -> Result<()>;

    /// Remove a member from a group.
    async fn group_remove_member(&self, group_id: &str, npub: &str) -> Result<()>;

    // -- Maintenance --

    /// Run the consolidation pipeline.
    async fn consolidate(&self, opts: ConsolidateParams) -> Result<ConsolidationReport>;

    /// Two-phase consolidation: prepare.
    async fn consolidate_prepare(
        &self,
        opts: ConsolidateParams,
        ttl_minutes: u32,
    ) -> Result<PrepareResult>;

    /// Two-phase consolidation: commit.
    async fn consolidate_commit(
        &self,
        session_id: &str,
        extractions: &[BatchExtraction],
    ) -> Result<CommitResult>;

    /// Run cluster fusion.
    async fn cluster_fusion(&self, opts: ClusterParams) -> Result<ClusterReport>;

    /// Sync memories from relay.
    async fn sync(&self) -> Result<SyncReport>;

    /// Generate embeddings for memories missing them.
    async fn embed(&self, limit: usize) -> Result<EmbedReport>;

    /// Prune old memories and messages.
    async fn prune(&self, days: u64, dry_run: bool) -> Result<PruneReport>;

    // -- Stats / meta --

    /// Count memories: returns (total, named, pending/ephemeral).
    async fn count_memories(&self) -> Result<(usize, usize, usize)>;

    /// Get a metadata value by key.
    async fn get_meta(&self, key: &str) -> Result<Option<String>>;

    /// Count entities, optionally filtered by kind.
    async fn entity_count(&self, kind: Option<&str>) -> Result<usize>;

    // -- Accessors --

    /// Get the signer, if available.
    fn signer(&self) -> Option<&Arc<dyn NomenSigner>>;

    /// Check if relay is configured.
    fn has_relay(&self) -> bool;
}

/// Report from a list operation (uses db-specific MemoryRecord).
pub struct ListReport {
    pub memories: Vec<MemoryRecord>,
    pub stats: Option<ListStats>,
}
