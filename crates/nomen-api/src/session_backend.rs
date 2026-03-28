//! SessionBackend: per-session signer override for multi-user identity.
//!
//! Wraps an existing `NomenBackend` and overrides `signer()` to return
//! a session-specific signer. All other methods delegate to the inner backend.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use nomen_core::collected::{CollectedEvent, CollectedEventFilter};
use nomen_core::groups::Group;
use nomen_core::ops::{ClusterParams, ConsolidateParams, EmbedReport, ListOptions, SyncReport};
use nomen_core::search::{SearchOptions, SearchResult};
use nomen_core::send::{SendOptions, SendResult};
use nomen_core::signer::NomenSigner;
use nomen_core::NewMemory;
use nomen_db::{CollectedMessageRecord, CollectedSearchResult, MemoryRecord, PruneReport};
use nomen_llm::cluster::ClusterReport;
use nomen_llm::consolidate::{BatchExtraction, CommitResult, ConsolidationReport, PrepareResult};
use nomen_media::MediaRef;

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

    async fn send(&self, opts: SendOptions) -> Result<SendResult> {
        self.inner.send(opts).await
    }

    async fn store_collected_event(
        &self,
        event: CollectedEvent,
    ) -> Result<nomen_db::collected::StoreResult> {
        self.inner.store_collected_event(event).await
    }

    async fn query_collected_events(
        &self,
        filter: CollectedEventFilter,
    ) -> Result<Vec<CollectedMessageRecord>> {
        self.inner.query_collected_events(filter).await
    }

    async fn search_collected_events(
        &self,
        query: &str,
        filter: CollectedEventFilter,
    ) -> Result<Vec<CollectedSearchResult>> {
        self.inner.search_collected_events(query, filter).await
    }

    async fn store_media(&self, data: &[u8], mime_type: &str) -> Result<Option<MediaRef>> {
        self.inner.store_media(data, mime_type).await
    }

    async fn entity_memories(&self, type_filter: Option<&str>) -> Result<Vec<MemoryRecord>> {
        self.inner.entity_memories(type_filter).await
    }

    async fn entity_relationships(
        &self,
        d_tag: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        self.inner.entity_relationships(d_tag).await
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
