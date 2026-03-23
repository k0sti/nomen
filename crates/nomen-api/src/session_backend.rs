//! SessionBackend: per-session signer override for multi-user identity.
//!
//! Wraps an existing `NomenBackend` and overrides `signer()` to return
//! a session-specific signer. All other methods delegate to the inner backend.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use nomen_core::groups::Group;
use nomen_core::ingest::{MessageQuery, RawMessage};
use nomen_core::ops::{
    ClusterParams, ConsolidateParams, EmbedReport, ListOptions, SyncReport,
};
use nomen_core::search::{SearchOptions, SearchResult};
use nomen_core::send::{SendOptions, SendResult};
use nomen_core::session::ResolvedSession;
use nomen_core::signer::NomenSigner;
use nomen_core::NewMemory;
use nomen_db::{EntityRecord, MemoryRecord, PruneReport, RawMessageRecord, RelationshipRecord};
use nomen_llm::cluster::ClusterReport;
use nomen_llm::consolidate::{BatchExtraction, CommitResult, ConsolidationReport, PrepareResult};

use crate::{ListReport, NomenBackend};

/// A backend wrapper that overrides the signer for a specific session.
///
/// Created when a client authenticates via `identity.auth`. All operations
/// delegate to the inner backend, but `signer()` returns the session-specific
/// signer instead of the global one.
pub struct SessionBackend {
    inner: Arc<dyn NomenBackend>,
    signer: Arc<dyn NomenSigner>,
}

impl SessionBackend {
    /// Create a new session backend with an overridden signer.
    pub fn new(inner: Arc<dyn NomenBackend>, signer: Arc<dyn NomenSigner>) -> Self {
        Self { inner, signer }
    }
}

#[async_trait]
impl NomenBackend for SessionBackend {
    async fn search(&self, opts: SearchOptions) -> Result<Vec<SearchResult>> {
        self.inner.search(opts).await
    }

    async fn store(&self, mem: NewMemory) -> Result<String> {
        self.inner.store(mem).await
    }

    async fn get_by_topic(&self, d_tag: &str) -> Result<Option<MemoryRecord>> {
        self.inner.get_by_topic(d_tag).await
    }

    async fn get_by_raw_topic(&self, topic: &str) -> Result<Option<MemoryRecord>> {
        self.inner.get_by_raw_topic(topic).await
    }

    async fn delete(&self, topic: Option<&str>, id: Option<&str>) -> Result<()> {
        self.inner.delete(topic, id).await
    }

    async fn list(&self, opts: ListOptions) -> Result<ListReport> {
        self.inner.list(opts).await
    }

    async fn ingest_message(&self, msg: RawMessage) -> Result<String> {
        self.inner.ingest_message(msg).await
    }

    async fn get_messages(&self, opts: MessageQuery) -> Result<Vec<RawMessageRecord>> {
        self.inner.get_messages(opts).await
    }

    async fn send(&self, opts: SendOptions) -> Result<SendResult> {
        self.inner.send(opts).await
    }

    fn resolve_session(
        &self,
        session_id: &str,
        default_channel: &str,
    ) -> Result<ResolvedSession> {
        self.inner.resolve_session(session_id, default_channel)
    }

    async fn entities(&self, kind: Option<&str>) -> Result<Vec<EntityRecord>> {
        self.inner.entities(kind).await
    }

    async fn entity_relationships(
        &self,
        entity_name: Option<&str>,
    ) -> Result<Vec<RelationshipRecord>> {
        self.inner.entity_relationships(entity_name).await
    }

    async fn group_list(&self) -> Result<Vec<Group>> {
        self.inner.group_list().await
    }

    async fn group_members(&self, group_id: &str) -> Result<Vec<String>> {
        self.inner.group_members(group_id).await
    }

    async fn group_create(
        &self,
        id: &str,
        name: &str,
        members: &[String],
        nostr_group: Option<&str>,
        relay: Option<&str>,
    ) -> Result<()> {
        self.inner
            .group_create(id, name, members, nostr_group, relay)
            .await
    }

    async fn group_add_member(&self, group_id: &str, npub: &str) -> Result<()> {
        self.inner.group_add_member(group_id, npub).await
    }

    async fn group_remove_member(&self, group_id: &str, npub: &str) -> Result<()> {
        self.inner.group_remove_member(group_id, npub).await
    }

    async fn consolidate(&self, opts: ConsolidateParams) -> Result<ConsolidationReport> {
        self.inner.consolidate(opts).await
    }

    async fn consolidate_prepare(
        &self,
        opts: ConsolidateParams,
        ttl_minutes: u32,
    ) -> Result<PrepareResult> {
        self.inner.consolidate_prepare(opts, ttl_minutes).await
    }

    async fn consolidate_commit(
        &self,
        session_id: &str,
        extractions: &[BatchExtraction],
    ) -> Result<CommitResult> {
        self.inner.consolidate_commit(session_id, extractions).await
    }

    async fn cluster_fusion(&self, opts: ClusterParams) -> Result<ClusterReport> {
        self.inner.cluster_fusion(opts).await
    }

    async fn sync(&self) -> Result<SyncReport> {
        self.inner.sync().await
    }

    async fn embed(&self, limit: usize) -> Result<EmbedReport> {
        self.inner.embed(limit).await
    }

    async fn prune(&self, days: u64, dry_run: bool) -> Result<PruneReport> {
        self.inner.prune(days, dry_run).await
    }

    async fn count_memories(&self) -> Result<(usize, usize, usize)> {
        self.inner.count_memories().await
    }

    async fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.inner.get_meta(key).await
    }

    async fn entity_count(&self, kind: Option<&str>) -> Result<usize> {
        self.inner.entity_count(kind).await
    }

    fn signer(&self) -> Option<&Arc<dyn NomenSigner>> {
        Some(&self.signer)
    }

    fn has_relay(&self) -> bool {
        self.inner.has_relay()
    }
}
