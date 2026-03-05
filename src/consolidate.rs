use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tracing::{debug, info};

use crate::embed::Embedder;
use crate::ingest::RawMessageRecord;

/// A memory extracted by the LLM from a batch of messages.
#[derive(Debug, Clone)]
pub struct ExtractedMemory {
    pub summary: String,
    pub detail: String,
    pub topic: String,
    pub confidence: f64,
}

/// Trait for LLM-powered consolidation. Implementations call an LLM to
/// summarize and extract structured memories from raw messages.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn consolidate(&self, messages: &[RawMessageRecord]) -> Result<Vec<ExtractedMemory>>;
}

/// Noop LLM provider — creates a simple summary from message content
/// without calling any external service.
pub struct NoopLlmProvider;

#[async_trait]
impl LlmProvider for NoopLlmProvider {
    async fn consolidate(&self, messages: &[RawMessageRecord]) -> Result<Vec<ExtractedMemory>> {
        if messages.is_empty() {
            return Ok(vec![]);
        }

        // Group by channel
        let mut by_channel: HashMap<String, Vec<&RawMessageRecord>> = HashMap::new();
        for msg in messages {
            let channel = msg.channel.clone().unwrap_or_else(|| "general".to_string());
            by_channel.entry(channel).or_default().push(msg);
        }

        let mut extracted = Vec::new();
        for (channel, msgs) in &by_channel {
            let content_lines: Vec<String> = msgs
                .iter()
                .map(|m| format!("[{}] {}: {}", m.created_at, m.sender, m.content))
                .collect();
            let detail = content_lines.join("\n");
            let summary = format!(
                "{} messages in #{} from {} sender(s)",
                msgs.len(),
                channel,
                {
                    let mut senders: Vec<&str> = msgs.iter().map(|m| m.sender.as_str()).collect();
                    senders.sort();
                    senders.dedup();
                    senders.len()
                }
            );

            extracted.push(ExtractedMemory {
                summary,
                detail,
                topic: format!("consolidated/{}", channel),
                confidence: 0.5,
            });
        }

        Ok(extracted)
    }
}

/// Configuration for the consolidation pipeline.
pub struct ConsolidationConfig {
    pub batch_size: usize,
    pub min_messages: usize,
    pub llm_provider: Box<dyn LlmProvider>,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            batch_size: 50,
            min_messages: 3,
            llm_provider: Box::new(NoopLlmProvider),
        }
    }
}

/// Report from a consolidation run.
#[derive(Debug, Default)]
pub struct ConsolidationReport {
    pub messages_processed: usize,
    pub memories_created: usize,
    pub channels: Vec<String>,
}

/// Run the consolidation pipeline.
///
/// 1. Query unconsolidated raw messages
/// 2. Group by channel
/// 3. Send to LLM provider for summarization
/// 4. Store consolidated memories
/// 5. Mark raw messages as consolidated
/// 6. Create consolidated_from edges
pub async fn consolidate(
    db: &Surreal<Db>,
    embedder: &dyn Embedder,
    config: &ConsolidationConfig,
) -> Result<ConsolidationReport> {
    let messages = crate::db::get_unconsolidated_messages(db, config.batch_size).await?;

    if messages.len() < config.min_messages {
        info!(
            count = messages.len(),
            min = config.min_messages,
            "Not enough unconsolidated messages to consolidate"
        );
        return Ok(ConsolidationReport::default());
    }

    debug!(count = messages.len(), "Processing unconsolidated messages");

    let extracted = config.llm_provider.consolidate(&messages).await?;

    let mut report = ConsolidationReport {
        messages_processed: messages.len(),
        ..Default::default()
    };

    for memory in &extracted {
        // Build content JSON
        let content = serde_json::json!({
            "summary": memory.summary,
            "detail": memory.detail,
        });

        let d_tag = format!("snow:memory:{}", memory.topic);

        // Store as a memory record
        let parsed = crate::memory::ParsedMemory {
            tier: "public".to_string(),
            topic: memory.topic.clone(),
            version: "1".to_string(),
            confidence: format!("{:.2}", memory.confidence),
            model: "nomen/consolidation".to_string(),
            summary: memory.summary.clone(),
            created_at: nostr_sdk::prelude::Timestamp::now(),
            d_tag: d_tag.clone(),
            source: "consolidation".to_string(),
            content_raw: content.to_string(),
            detail: memory.detail.clone(),
        };

        crate::db::store_memory_direct(db, &parsed, "consolidation").await?;

        // Generate embedding if embedder is configured
        if embedder.dimensions() > 0 {
            let text = format!("{} {}", memory.summary, memory.detail);
            match embedder.embed(&[text]).await {
                Ok(embeddings) => {
                    if let Some(embedding) = embeddings.into_iter().next() {
                        let _ = crate::db::store_embedding(db, &d_tag, embedding).await;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to generate embedding for consolidation: {e}");
                }
            }
        }

        report.memories_created += 1;
        if let Some(channel) = memory.topic.strip_prefix("consolidated/") {
            report.channels.push(channel.to_string());
        }
    }

    // Mark all processed messages as consolidated
    let msg_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
    crate::db::mark_messages_consolidated(db, &msg_ids).await?;

    // Create consolidated_from edges (memory → raw_message)
    // We link each consolidated memory to the messages from its channel
    let mut by_channel: HashMap<String, Vec<String>> = HashMap::new();
    for msg in &messages {
        let channel = msg.channel.clone().unwrap_or_else(|| "general".to_string());
        by_channel.entry(channel).or_default().push(msg.id.clone());
    }

    // Note: edges require record IDs from SurrealDB. For now we log that
    // edges would be created; full graph linking needs the memory record IDs
    // returned from store_memory_direct (which currently doesn't return them).
    debug!(
        memories = report.memories_created,
        messages = report.messages_processed,
        "Consolidation complete, edges pending full record ID support"
    );

    Ok(report)
}
