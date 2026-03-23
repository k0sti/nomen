//! impl NomenBackend for Nomen — bridges the API trait to concrete Nomen methods.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use nomen_api::{ListReport, NomenBackend};
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

use crate::{ClusterOptions, ConsolidateOptions, Nomen};

#[async_trait]
impl NomenBackend for Nomen {
    async fn search(&self, opts: SearchOptions) -> Result<Vec<SearchResult>> {
        self.search(opts).await
    }

    async fn store(&self, mem: NewMemory) -> Result<String> {
        self.store(mem).await
    }

    async fn get_by_topic(&self, d_tag: &str) -> Result<Option<MemoryRecord>> {
        self.get_by_topic(d_tag).await
    }

    async fn get_by_raw_topic(&self, topic: &str) -> Result<Option<MemoryRecord>> {
        self.get_by_raw_topic(topic).await
    }

    async fn delete(&self, topic: Option<&str>, id: Option<&str>) -> Result<()> {
        self.delete(topic, id).await
    }

    async fn list(&self, opts: ListOptions) -> Result<ListReport> {
        let report = Nomen::list(self, opts).await?;
        Ok(ListReport {
            memories: report.memories,
            stats: report.stats,
        })
    }

    async fn ingest_message(&self, msg: RawMessage) -> Result<String> {
        self.ingest_message(msg).await
    }

    async fn get_messages(&self, opts: MessageQuery) -> Result<Vec<RawMessageRecord>> {
        self.get_messages(opts).await
    }

    async fn send(&self, opts: SendOptions) -> Result<SendResult> {
        self.send(opts).await
    }

    fn resolve_session(
        &self,
        session_id: &str,
        default_channel: &str,
    ) -> Result<ResolvedSession> {
        self.resolve_session(session_id, default_channel)
    }

    async fn entities(&self, kind: Option<&str>) -> Result<Vec<EntityRecord>> {
        self.entities(kind).await
    }

    async fn entity_relationships(
        &self,
        entity_name: Option<&str>,
    ) -> Result<Vec<RelationshipRecord>> {
        self.entity_relationships(entity_name).await
    }

    async fn group_list(&self) -> Result<Vec<Group>> {
        self.group_list().await
    }

    async fn group_members(&self, group_id: &str) -> Result<Vec<String>> {
        self.group_members(group_id).await
    }

    async fn group_create(
        &self,
        id: &str,
        name: &str,
        members: &[String],
        nostr_group: Option<&str>,
        relay: Option<&str>,
    ) -> Result<()> {
        self.group_create(id, name, members, nostr_group, relay)
            .await
    }

    async fn group_add_member(&self, group_id: &str, npub: &str) -> Result<()> {
        self.group_add_member(group_id, npub).await
    }

    async fn group_remove_member(&self, group_id: &str, npub: &str) -> Result<()> {
        self.group_remove_member(group_id, npub).await
    }

    async fn consolidate(&self, opts: ConsolidateParams) -> Result<ConsolidationReport> {
        let full_opts = ConsolidateOptions {
            batch_size: opts.batch_size,
            min_messages: opts.min_messages,
            ..Default::default()
        };
        self.consolidate(full_opts).await
    }

    async fn consolidate_prepare(
        &self,
        opts: ConsolidateParams,
        ttl_minutes: u32,
    ) -> Result<PrepareResult> {
        let full_opts = ConsolidateOptions {
            batch_size: opts.batch_size,
            min_messages: opts.min_messages,
            ..Default::default()
        };
        self.consolidate_prepare(full_opts, ttl_minutes).await
    }

    async fn consolidate_commit(
        &self,
        session_id: &str,
        extractions: &[BatchExtraction],
    ) -> Result<CommitResult> {
        self.consolidate_commit(session_id, extractions).await
    }

    async fn cluster_fusion(&self, opts: ClusterParams) -> Result<ClusterReport> {
        let full_opts = ClusterOptions {
            min_members: opts.min_members,
            namespace_depth: opts.namespace_depth,
            dry_run: opts.dry_run,
            prefix_filter: opts.prefix_filter,
            ..Default::default()
        };
        self.cluster_fusion(full_opts).await
    }

    async fn sync(&self) -> Result<SyncReport> {
        self.sync().await
    }

    async fn embed(&self, limit: usize) -> Result<EmbedReport> {
        self.embed(limit).await
    }

    async fn prune(&self, days: u64, dry_run: bool) -> Result<PruneReport> {
        self.prune(days, dry_run).await
    }

    async fn count_memories(&self) -> Result<(usize, usize, usize)> {
        Nomen::count_memories(self).await
    }

    async fn get_meta(&self, key: &str) -> Result<Option<String>> {
        nomen_db::get_meta(&self.db, key).await
    }

    async fn entity_count(&self, kind: Option<&str>) -> Result<usize> {
        let entities = self.entities(kind).await?;
        Ok(entities.len())
    }

    fn signer(&self) -> Option<&Arc<dyn NomenSigner>> {
        self.signer.as_ref()
    }

    fn has_relay(&self) -> bool {
        self.relay.is_some()
    }
}
