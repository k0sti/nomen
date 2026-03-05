use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tracing::{debug, info, warn};

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
            let channel = { let c = msg.channel.clone(); if c.is_empty() { "general".to_string() } else { c } };
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

/// OpenAI/OpenRouter-compatible LLM provider for real consolidation.
pub struct OpenAiLlmProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiLlmProvider {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    /// Create from config, returning None if API key is missing.
    pub fn from_config(config: &crate::config::ConsolidationLlmConfig) -> Option<Self> {
        let api_key = std::env::var(&config.api_key_env).unwrap_or_default();
        if api_key.is_empty() {
            warn!(
                "Consolidation API key env {} not set, will use NoopLlmProvider",
                config.api_key_env
            );
            return None;
        }

        let base_url = config.base_url.clone().unwrap_or_else(|| {
            match config.provider.as_str() {
                "openai" => "https://api.openai.com/v1".to_string(),
                "openrouter" => "https://openrouter.ai/api/v1".to_string(),
                _ => "https://openrouter.ai/api/v1".to_string(),
            }
        });

        Some(Self::new(&base_url, &api_key, &config.model))
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Deserialize)]
struct LlmExtracted {
    memories: Vec<LlmMemory>,
}

#[derive(Deserialize)]
struct LlmMemory {
    topic: String,
    summary: String,
    detail: String,
    confidence: f64,
}

#[async_trait]
impl LlmProvider for OpenAiLlmProvider {
    async fn consolidate(&self, messages: &[RawMessageRecord]) -> Result<Vec<ExtractedMemory>> {
        if messages.is_empty() {
            return Ok(vec![]);
        }

        // Build message transcript
        let mut transcript = String::new();
        for msg in messages {
            let channel = if msg.channel.is_empty() { "general" } else { &msg.channel };
            transcript.push_str(&format!(
                "[{}] #{} {}: {}\n",
                msg.created_at, channel, msg.sender, msg.content
            ));
        }

        let system_prompt = "You are a memory consolidation agent. Given a batch of raw messages, \
extract significant facts, decisions, and context into structured memories. \
Return JSON with this exact structure: {\"memories\": [{\"topic\": \"category/subcategory\", \
\"summary\": \"one-line summary\", \"detail\": \"full detail\", \"confidence\": 0.8}]}. \
Only extract genuinely significant information. Set confidence 0.5-1.0 based on how certain the information is. \
Return an empty memories array if nothing significant is found.";

        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": format!("Extract memories from these messages:\n\n{transcript}") }
            ],
            "temperature": 0.3,
            "response_format": { "type": "json_object" }
        });

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM API error {status}: {text}");
        }

        let chat_resp: ChatResponse = resp.json().await?;
        let content = chat_resp
            .choices
            .first()
            .map(|c| c.message.content.as_str())
            .unwrap_or("{}");

        let extracted: LlmExtracted = serde_json::from_str(content)
            .unwrap_or_else(|e| {
                warn!("Failed to parse LLM response as JSON: {e}");
                LlmExtracted { memories: vec![] }
            });

        Ok(extracted
            .memories
            .into_iter()
            .map(|m| ExtractedMemory {
                summary: m.summary,
                detail: m.detail,
                topic: m.topic,
                confidence: m.confidence.clamp(0.0, 1.0),
            })
            .collect())
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

    // Group messages by channel for edge creation later
    let mut by_channel: HashMap<String, Vec<String>> = HashMap::new();
    for msg in &messages {
        let channel = { let c = msg.channel.clone(); if c.is_empty() { "general".to_string() } else { c } };
        by_channel.entry(channel).or_default().push(msg.id.clone());
    }

    // Track memory record IDs per channel for edge creation
    let mut memory_ids_by_channel: HashMap<String, String> = HashMap::new();

    for memory in &extracted {
        let mem = crate::NewMemory {
            topic: memory.topic.clone(),
            summary: memory.summary.clone(),
            detail: memory.detail.clone(),
            tier: "public".to_string(),
            confidence: memory.confidence,
            source: Some("consolidation".to_string()),
            model: Some("nomen/consolidation".to_string()),
        };

        let d_tag = crate::Nomen::store_direct(db, embedder, mem).await?;

        report.memories_created += 1;
        if let Some(channel) = memory.topic.strip_prefix("consolidated/") {
            report.channels.push(channel.to_string());
            // Look up the record ID we just created
            if let Ok(record_id) = get_memory_record_id(db, &d_tag).await {
                memory_ids_by_channel.insert(channel.to_string(), record_id);
            }
        }
    }

    // Mark all processed messages as consolidated
    let msg_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
    crate::db::mark_messages_consolidated(db, &msg_ids).await?;

    // Create consolidated_from edges (memory → raw_message)
    for (channel, memory_record_id) in &memory_ids_by_channel {
        if let Some(raw_msg_ids) = by_channel.get(channel) {
            for raw_msg_id in raw_msg_ids {
                if let Err(e) = crate::db::create_consolidated_edge(db, memory_record_id, raw_msg_id).await {
                    warn!("Failed to create consolidated_from edge: {e}");
                }
            }
        }
    }

    debug!(
        memories = report.memories_created,
        messages = report.messages_processed,
        "Consolidation complete"
    );

    Ok(report)
}

/// Get the SurrealDB record ID for a memory by its d_tag.
async fn get_memory_record_id(db: &Surreal<Db>, d_tag: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct IdRow {
        #[serde(deserialize_with = "crate::db::deserialize_thing_as_string")]
        id: String,
    }
    let rows: Vec<IdRow> = db
        .query("SELECT id FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;
    rows.first()
        .map(|r| r.id.clone())
        .ok_or_else(|| anyhow::anyhow!("Memory not found for d_tag: {d_tag}"))
}
