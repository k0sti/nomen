//! impl Nomen — consolidation, cluster fusion, two-phase consolidation.

use anyhow::Result;

use crate::cluster::{ClusterConfig, ClusterReport, NoopClusterLlmProvider};
use crate::consolidate::{ConsolidationConfig, ConsolidationReport, NoopLlmProvider};
use crate::entities;
use crate::{ClusterOptions, ConsolidateOptions, Nomen};

impl Nomen {
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
            platform: opts.platform,
            community_id: opts.community_id,
            chat_id: opts.chat_id,
            thread_id: opts.thread_id,
            since: opts.since,
            older_than: opts.older_than,
            author_pubkey,
            ..Default::default()
        };
        let report = crate::consolidate::consolidate(
            &self.db,
            self.embedder.as_ref(),
            &config,
            self.relay.as_ref(),
        )
        .await?;

        self.emit_event(
            "consolidation.complete",
            serde_json::json!({
                "memories_created": report.memories_created,
                "messages_processed": report.messages_processed,
            }),
        );

        Ok(report)
    }

    /// Two-phase consolidation: prepare (stages 1-2).
    pub async fn consolidate_prepare(
        &self,
        opts: ConsolidateOptions,
        ttl_minutes: u32,
    ) -> Result<crate::consolidate::PrepareResult> {
        let author_pubkey = self.signer.as_ref().map(|s| s.public_key().to_hex());
        let config = ConsolidationConfig {
            batch_size: opts.batch_size,
            min_messages: opts.min_messages,
            llm_provider: Box::new(NoopLlmProvider),
            entity_extractor: Box::new(entities::HeuristicExtractor),
            platform: opts.platform,
            community_id: opts.community_id,
            chat_id: opts.chat_id,
            thread_id: opts.thread_id,
            since: opts.since,
            older_than: opts.older_than,
            author_pubkey,
            ..Default::default()
        };
        crate::consolidate::prepare(&self.db, &config, ttl_minutes).await
    }

    /// Two-phase consolidation: commit (stages 4-6).
    pub async fn consolidate_commit(
        &self,
        session_id: &str,
        extractions: &[crate::consolidate::BatchExtraction],
    ) -> Result<crate::consolidate::CommitResult> {
        let author_pubkey = self.signer.as_ref().map(|s| s.public_key().to_hex());
        let config = ConsolidationConfig {
            author_pubkey,
            entity_extractor: Box::new(entities::HeuristicExtractor),
            ..Default::default()
        };
        let result = crate::consolidate::commit(
            &self.db,
            self.embedder.as_ref(),
            &config,
            self.relay.as_ref(),
            session_id,
            extractions,
        )
        .await?;

        self.emit_event(
            "consolidation.complete",
            serde_json::json!({
                "memories_created": result.memories_created,
                "messages_consolidated": result.messages_consolidated,
                "session_id": result.session_id,
            }),
        );

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
        let report = crate::cluster::run_cluster_fusion(
            &self.db,
            self.embedder.as_ref(),
            &config,
            self.relay.as_ref(),
        )
        .await?;

        self.emit_event(
            "cluster.fused",
            serde_json::json!({
                "clusters_merged": report.clusters_found,
                "dry_run": report.dry_run,
            }),
        );

        Ok(report)
    }
}
