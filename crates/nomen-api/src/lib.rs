//! nomen-api — canonical API v2 dispatch layer for MCP and CVM.
//!
//! This crate provides the shared dispatch logic and operation modules
//! that both MCP and CVM transports use. Operations are defined against
//! the [`NomenBackend`] trait, which the main `nomen` crate implements.

pub mod dispatch;
pub mod operations;
pub mod types;

pub use dispatch::{dispatch, mcp_tool_to_action};
pub use nomen_core::api::errors;
pub use nomen_core::api::types::ApiResponse;

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use nomen_core::groups::Group;
use nomen_core::ingest::{MessageQuery, RawMessage};
use nomen_core::ops::{
    ClusterParams, ConsolidateParams, EmbedReport, ListOptions, ListStats, SyncReport,
};
use nomen_core::search::{SearchOptions, SearchResult};
use nomen_core::send::{SendOptions, SendResult};
use nomen_core::session::ResolvedSession;
use nomen_core::signer::NomenSigner;
use nomen_core::NewMemory;
use nomen_db::{EntityRecord, MemoryRecord, PruneReport, RawMessageRecord, RelationshipRecord};
use nomen_llm::cluster::{ClusterReport};
use nomen_llm::consolidate::{
    BatchExtraction, CommitResult, ConsolidationReport, PrepareResult,
};

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

    /// Ingest a raw message; returns its id.
    async fn ingest_message(&self, msg: RawMessage) -> Result<String>;

    /// Query raw messages.
    async fn get_messages(&self, opts: MessageQuery) -> Result<Vec<RawMessageRecord>>;

    /// Send a message via relay.
    async fn send(&self, opts: SendOptions) -> Result<SendResult>;

    // -- Session --

    /// Resolve a session ID to tier/scope/channel.
    fn resolve_session(
        &self,
        session_id: &str,
        default_channel: &str,
    ) -> Result<ResolvedSession>;

    // -- Entities --

    /// List entities, optionally filtered by kind.
    async fn entities(&self, kind: Option<&str>) -> Result<Vec<EntityRecord>>;

    /// List entity relationships, optionally filtered by name.
    async fn entity_relationships(
        &self,
        entity_name: Option<&str>,
    ) -> Result<Vec<RelationshipRecord>>;

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
