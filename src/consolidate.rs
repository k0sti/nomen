use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use nostr_sdk::prelude::*;
use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tracing::{debug, info, warn};

use crate::embed::Embedder;
use crate::ingest::RawMessageRecord;
use crate::relay::RelayManager;

/// Time window for grouping messages (4 hours in seconds).
const TIME_WINDOW_SECS: i64 = 4 * 3600;

/// A memory extracted by the LLM from a batch of messages.
#[derive(Debug, Clone)]
pub struct ExtractedMemory {
    pub summary: String,
    pub detail: String,
    pub topic: String,
    pub confidence: f64,
    /// Importance score (1-10). Higher = more important to remember.
    pub importance: Option<i32>,
    /// Whether this memory contradicts existing information (set during merge).
    pub contradicts_existing: bool,
}

/// Trait for LLM-powered consolidation. Implementations call an LLM to
/// summarize and extract structured memories from raw messages.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn consolidate(&self, messages: &[RawMessageRecord]) -> Result<Vec<ExtractedMemory>>;

    /// Merge new information into an existing memory.
    /// Default implementation just returns the new extraction as-is.
    async fn merge(
        &self,
        existing_summary: &str,
        existing_detail: &str,
        messages: &[RawMessageRecord],
    ) -> Result<Vec<ExtractedMemory>> {
        // Default: just consolidate the new messages (no merge logic)
        let _ = (existing_summary, existing_detail);
        self.consolidate(messages).await
    }
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

        // Derive a semantic topic from the group key
        let topic = derive_topic_from_messages(messages);

        let content_lines: Vec<String> = messages
            .iter()
            .map(|m| format!("[{}] {}: {}", m.created_at, m.sender, m.content))
            .collect();
        let detail = content_lines.join("\n");

        let mut senders: Vec<&str> = messages.iter().map(|m| m.sender.as_str()).collect();
        senders.sort();
        senders.dedup();

        let summary = format!(
            "{} messages from {} sender(s)",
            messages.len(),
            senders.len()
        );

        Ok(vec![ExtractedMemory {
            summary,
            detail,
            topic,
            confidence: 0.5,
            importance: Some(5),
            contradicts_existing: false,
        }])
    }
}

/// Derive a semantic topic name from a batch of messages.
///
/// Uses the sender/channel info to produce topics like:
/// - `user/<sender>/<channel>` for private messages
/// - `group/<channel>/conversation` for group messages
/// - `conversation/<channel>` as fallback
fn derive_topic_from_messages(messages: &[RawMessageRecord]) -> String {
    let mut senders: Vec<&str> = messages.iter().map(|m| m.sender.as_str()).collect();
    senders.sort();
    senders.dedup();

    let channel = messages
        .first()
        .map(|m| m.channel.as_str())
        .unwrap_or("general");
    let channel = if channel.is_empty() { "general" } else { channel };

    if senders.len() == 1 {
        let sender = senders[0];
        // Clean up sender name for use as a topic component
        let sender_clean = sanitize_topic_component(sender);
        format!("user/{sender_clean}/conversation")
    } else {
        let channel_clean = sanitize_topic_component(channel);
        format!("group/{channel_clean}/conversation")
    }
}

/// Clean a string for use in a topic path (lowercase, replace non-alphanum with dash).
fn sanitize_topic_component(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect();
    // Collapse multiple dashes
    let mut result = String::new();
    let mut prev_dash = false;
    for c in cleaned.chars() {
        if c == '-' {
            if !prev_dash {
                result.push(c);
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }
    result.trim_matches('-').to_string()
}

/// Derive the memory tier from a group of source messages.
///
/// - DM messages (source "nostr" with sender npub, no group channel) → "private"
/// - Group messages (channel matches a group pattern) → "group"
/// - Public/CLI/other → "public"
fn derive_tier_from_messages(messages: &[RawMessageRecord]) -> String {
    // Check sources — if any message is from a DM-like source, treat as private
    let has_dm = messages.iter().any(|m| {
        // nostr DMs have source "nostr" and either empty channel or "dm" channel
        (m.source == "nostr" && (m.channel.is_empty() || m.channel == "dm"))
            || m.source == "telegram_dm"
            || m.source == "dm"
    });

    let has_group = messages.iter().any(|m| {
        // Group messages have a non-empty channel that isn't "dm" or "general"
        !m.channel.is_empty()
            && m.channel != "dm"
            && m.channel != "general"
            && (m.source == "nostr" || m.source == "telegram" || m.source.starts_with("group"))
    });

    if has_dm {
        "private".to_string()
    } else if has_group {
        "group".to_string()
    } else {
        "public".to_string()
    }
}

/// Enforce cross-group consolidation guard: private sources must never produce
/// group or public tier memories. Returns the tier, potentially downgraded.
fn enforce_tier_guard(derived_tier: &str, source_tier: &str) -> String {
    match source_tier {
        "private" => {
            // Private sources can only produce private memories
            if derived_tier != "private" {
                warn!(
                    derived = derived_tier,
                    "Cross-group guard: downgrading tier to private (source is private)"
                );
            }
            "private".to_string()
        }
        "group" => {
            // Group sources can produce group or private, but not public
            if derived_tier == "public" {
                warn!("Cross-group guard: downgrading tier to group (source is group)");
                "group".to_string()
            } else {
                derived_tier.to_string()
            }
        }
        _ => derived_tier.to_string(),
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
    #[serde(default)]
    importance: Option<i32>,
    #[serde(default)]
    contradicts_existing: Option<bool>,
}

#[async_trait]
impl LlmProvider for OpenAiLlmProvider {
    async fn merge(
        &self,
        existing_summary: &str,
        existing_detail: &str,
        messages: &[RawMessageRecord],
    ) -> Result<Vec<ExtractedMemory>> {
        if messages.is_empty() {
            return Ok(vec![]);
        }

        let mut transcript = String::new();
        for msg in messages {
            let channel = if msg.channel.is_empty() { "general" } else { &msg.channel };
            transcript.push_str(&format!(
                "[{}] #{} {}: {}\n",
                msg.created_at, channel, msg.sender, msg.content
            ));
        }

        let system_prompt = "You are a memory consolidation agent. You are merging new information \
into an existing memory. Return JSON with this exact structure: {\"memories\": [{\"topic\": \"category/subcategory\", \
\"summary\": \"one-line summary\", \"detail\": \"full detail\", \"confidence\": 0.8, \"importance\": 7, \
\"contradicts_existing\": false}]}. \
Merge the new information into the existing memory. Update what changed. Keep what's still true. \
Set contradicts_existing to true if the new information directly contradicts facts in the existing memory. \
Set importance 1-10: 1=trivial, 5=normal, 8=important decision, 10=critical fact. \
The topic should remain the same as the existing memory's topic.";

        let user_prompt = format!(
            "Existing memory:\nSummary: {existing_summary}\nDetail: {existing_detail}\n\n\
             New messages:\n{transcript}\n\n\
             Merge the new information into the existing memory."
        );

        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
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
                warn!("Failed to parse LLM merge response as JSON: {e}");
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
                importance: m.importance.map(|i| i.clamp(1, 10)),
                contradicts_existing: m.contradicts_existing.unwrap_or(false),
            })
            .collect())
    }

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
\"summary\": \"one-line summary\", \"detail\": \"full detail\", \"confidence\": 0.8, \"importance\": 7}]}. \
Use semantic topic names following this convention: \
- user/<name>/<aspect> for per-user knowledge (preferences, timezone, projects) \
- project/<name>/<aspect> for project knowledge \
- group/<id>/<aspect> for group context \
- fact/<domain>/<topic> for general knowledge \
Only extract genuinely significant information. Set confidence 0.5-1.0 based on how certain the information is. \
Set importance 1-10: 1=trivial, 5=normal, 8=important decision, 10=critical fact. \
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
                importance: m.importance.map(|i| i.clamp(1, 10)),
                contradicts_existing: false,
            })
            .collect())
    }
}

/// Configuration for the consolidation pipeline.
pub struct ConsolidationConfig {
    pub batch_size: usize,
    pub min_messages: usize,
    pub llm_provider: Box<dyn LlmProvider>,
    /// If true, preview what would be consolidated without publishing.
    pub dry_run: bool,
    /// Only consolidate messages older than this duration string (e.g. "30m", "1h", "7d").
    pub older_than: Option<String>,
    /// Only consolidate messages matching this tier.
    pub tier: Option<String>,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            batch_size: 50,
            min_messages: 3,
            llm_provider: Box::new(NoopLlmProvider),
            dry_run: false,
            older_than: None,
            tier: None,
        }
    }
}

/// Report from a consolidation run.
#[derive(Debug, Default)]
pub struct ConsolidationReport {
    pub messages_processed: usize,
    pub memories_created: usize,
    pub memories_updated: usize,
    pub events_deleted: usize,
    pub channels: Vec<String>,
    pub groups: Vec<GroupSummary>,
    pub dry_run: bool,
}

/// Summary of a message group for reporting.
#[derive(Debug)]
pub struct GroupSummary {
    pub key: String,
    pub message_count: usize,
    pub topic: String,
}

/// A group key for time-window grouping.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct GroupKey {
    /// sender (for private) or channel (for group/general)
    identity: String,
    /// Time window index (created_at / TIME_WINDOW_SECS)
    window: i64,
}

/// Parse a duration string like "30m", "1h", "7d" into seconds.
pub fn parse_duration_str(s: &str) -> Result<i64> {
    let s = s.trim();
    if s.is_empty() {
        anyhow::bail!("Empty duration string");
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str.parse().map_err(|_| anyhow::anyhow!("Invalid duration number: {num_str}"))?;

    match unit {
        "s" => Ok(num),
        "m" => Ok(num * 60),
        "h" => Ok(num * 3600),
        "d" => Ok(num * 86400),
        "w" => Ok(num * 604800),
        _ => anyhow::bail!("Unknown duration unit: {unit}. Use s, m, h, d, or w"),
    }
}

/// Group messages by sender + time window (4-hour blocks).
fn group_messages(messages: Vec<RawMessageRecord>) -> HashMap<GroupKey, Vec<RawMessageRecord>> {
    let mut groups: HashMap<GroupKey, Vec<RawMessageRecord>> = HashMap::new();

    for msg in messages {
        let timestamp = chrono::DateTime::parse_from_rfc3339(&msg.created_at)
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        let window = timestamp / TIME_WINDOW_SECS;

        // Group by sender for DMs, by channel for group messages
        let identity = if msg.channel.is_empty() || msg.channel == "general" {
            msg.sender.clone()
        } else {
            msg.channel.clone()
        };

        let key = GroupKey { identity, window };
        groups.entry(key).or_default().push(msg);
    }

    groups
}

/// Run the consolidation pipeline.
///
/// 1. Query unconsolidated raw messages (with optional filters)
/// 2. Group by sender/channel + 4-hour time windows
/// 3. Send each group to LLM provider for summarization
/// 4. Store consolidated memories with provenance tags
/// 5. Mark raw messages as consolidated
/// 6. Create consolidated_from edges
/// 7. Publish NIP-09 deletion events for consumed ephemerals
pub async fn consolidate(
    db: &Surreal<Db>,
    embedder: &dyn Embedder,
    config: &ConsolidationConfig,
    relay: Option<&RelayManager>,
) -> Result<ConsolidationReport> {
    // Build cutoff time from older_than
    let cutoff = if let Some(ref duration_str) = config.older_than {
        let secs = parse_duration_str(duration_str)?;
        let cutoff_time = chrono::Utc::now() - chrono::Duration::seconds(secs);
        Some(cutoff_time.to_rfc3339())
    } else {
        None
    };

    let messages = crate::db::get_unconsolidated_messages_filtered(
        db,
        config.batch_size,
        cutoff.as_deref(),
        config.tier.as_deref(),
    )
    .await?;

    if messages.len() < config.min_messages {
        info!(
            count = messages.len(),
            min = config.min_messages,
            "Not enough unconsolidated messages to consolidate"
        );
        return Ok(ConsolidationReport { dry_run: config.dry_run, ..Default::default() });
    }

    debug!(count = messages.len(), "Processing unconsolidated messages");

    // Group messages by sender/channel + time window
    let grouped = group_messages(messages.clone());
    debug!(groups = grouped.len(), "Grouped messages into time windows");

    let mut report = ConsolidationReport {
        messages_processed: messages.len(),
        dry_run: config.dry_run,
        ..Default::default()
    };

    let now_timestamp = chrono::Utc::now().timestamp();
    let mut all_consumed_msg_ids: Vec<String> = Vec::new();

    for (key, group_msgs) in &grouped {
        if group_msgs.len() < config.min_messages {
            debug!(
                key = %key.identity,
                count = group_msgs.len(),
                "Skipping group with too few messages"
            );
            continue;
        }

        let extracted = config.llm_provider.consolidate(group_msgs).await?;

        // Derive tier from source messages (TODO #1)
        let derived_tier = derive_tier_from_messages(group_msgs);
        // Apply cross-group consolidation guard (TODO #7)
        let most_restrictive_source = if group_msgs.iter().any(|m| {
            m.source == "dm" || m.source == "telegram_dm"
                || (m.source == "nostr" && (m.channel.is_empty() || m.channel == "dm"))
        }) {
            "private"
        } else if group_msgs.iter().any(|m| {
            !m.channel.is_empty() && m.channel != "dm" && m.channel != "general"
        }) {
            "group"
        } else {
            "public"
        };
        let tier = enforce_tier_guard(&derived_tier, most_restrictive_source);

        for memory in &extracted {
            let group_summary = GroupSummary {
                key: format!("{}:{}", key.identity, key.window),
                message_count: group_msgs.len(),
                topic: memory.topic.clone(),
            };
            report.groups.push(group_summary);

            if config.dry_run {
                report.memories_created += 1;
                continue;
            }

            let d_tag = format!("snow:memory:{}", memory.topic);

            // Check if a memory with this topic already exists (TODO #2: merge)
            let existing = get_existing_memory(db, &d_tag).await;

            let (final_summary, final_detail, final_confidence, final_importance, contradicts, is_merge) = if let Ok(Some(existing_mem)) = existing {
                // Merge: re-prompt LLM with existing + new
                debug!(topic = %memory.topic, "Merging into existing memory");
                let existing_summary = existing_mem.summary.as_deref().unwrap_or("");
                let existing_detail = &existing_mem.content;

                match config.llm_provider.merge(existing_summary, existing_detail, group_msgs).await {
                    Ok(merged) if !merged.is_empty() => {
                        let m = &merged[0];
                        (m.summary.clone(), m.detail.clone(), m.confidence, m.importance, m.contradicts_existing, true)
                    }
                    Ok(_) => {
                        // Merge returned empty, use extracted as-is
                        (memory.summary.clone(), memory.detail.clone(), memory.confidence, memory.importance, false, true)
                    }
                    Err(e) => {
                        warn!("LLM merge failed, using extracted memory: {e}");
                        (memory.summary.clone(), memory.detail.clone(), memory.confidence, memory.importance, false, true)
                    }
                }
            } else {
                // No existing memory — check for near-duplicates via embedding (TODO #6)
                let mut is_dedup_merge = false;
                if embedder.dimensions() > 0 {
                    let text = format!("{} {}", memory.summary, memory.detail);
                    if let Ok(emb) = embedder.embed_one(&text).await {
                        if let Ok(similar) = find_similar_memory(db, &emb, 0.92).await {
                            if let Some(sim_dtag) = similar {
                                debug!(
                                    topic = %memory.topic,
                                    similar_dtag = %sim_dtag,
                                    "Found near-duplicate memory, merging"
                                );
                                // Fetch the similar memory and merge
                                if let Ok(Some(sim_mem)) = get_existing_memory(db, &sim_dtag).await {
                                    let sim_summary = sim_mem.summary.as_deref().unwrap_or("");
                                    match config.llm_provider.merge(sim_summary, &sim_mem.content, group_msgs).await {
                                        Ok(merged) if !merged.is_empty() => {
                                            let m = &merged[0];
                                            is_dedup_merge = true;
                                            // Store using the similar memory's d_tag
                                            let mem = crate::NewMemory {
                                                topic: sim_dtag.strip_prefix("snow:memory:").unwrap_or(&memory.topic).to_string(),
                                                summary: m.summary.clone(),
                                                detail: m.detail.clone(),
                                                tier: tier.clone(),
                                                confidence: m.confidence,
                                                source: Some("consolidation".to_string()),
                                                model: Some("nomen/consolidation".to_string()),
                                            };
                                            let stored_dtag = crate::Nomen::store_direct(db, embedder, mem).await?;
                                            // Bump version
                                            bump_memory_version(db, &stored_dtag).await.ok();
                                            crate::db::set_consolidation_tags(
                                                db, &stored_dtag,
                                                &group_msgs.len().to_string(),
                                                &now_timestamp.to_string(),
                                            ).await.ok();
                                            report.memories_updated += 1;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }

                if is_dedup_merge {
                    // Track channel for reporting
                    let channel = group_msgs.first().map(|m| m.channel.as_str()).unwrap_or("general");
                    if !channel.is_empty() && !report.channels.contains(&channel.to_string()) {
                        report.channels.push(channel.to_string());
                    }
                    continue;
                }

                (memory.summary.clone(), memory.detail.clone(), memory.confidence, memory.importance, false, false)
            };

            let summary_for_entities = final_summary.clone();
            let detail_for_entities = final_detail.clone();

            let mem = crate::NewMemory {
                topic: memory.topic.clone(),
                summary: final_summary,
                detail: final_detail,
                tier: tier.clone(),
                confidence: final_confidence,
                source: Some("consolidation".to_string()),
                model: Some("nomen/consolidation".to_string()),
            };

            // Build extra tags for provenance
            let consolidated_from_count = group_msgs.len().to_string();
            let consolidated_at = now_timestamp.to_string();

            let d_tag = crate::Nomen::store_direct(db, embedder, mem).await?;

            if is_merge {
                // Bump version for merged memories (TODO #2)
                bump_memory_version(db, &d_tag).await.ok();
                report.memories_updated += 1;
            } else {
                report.memories_created += 1;
            }

            // Update the memory record with consolidation tags
            crate::db::set_consolidation_tags(
                db,
                &d_tag,
                &consolidated_from_count,
                &consolidated_at,
            )
            .await
            .ok();

            // Store importance score
            if let Some(imp) = final_importance {
                crate::db::set_importance(db, &d_tag, imp).await.ok();
            }

            // Handle conflict detection: create contradicts edge
            if contradicts && is_merge {
                let existing_d_tag = format!("snow:memory:{}", memory.topic);
                if let Err(e) = crate::db::create_references_edge(
                    db,
                    &d_tag,
                    &existing_d_tag,
                    "contradicts",
                ).await {
                    warn!("Failed to create contradicts edge: {e}");
                } else {
                    debug!(topic = %memory.topic, "Created contradicts edge for conflicting merge");
                }
            }

            // Create consolidated_from edges
            if let Ok(record_id) = get_memory_record_id(db, &d_tag).await {
                for msg in group_msgs {
                    if let Err(e) = crate::db::create_consolidated_edge(db, &record_id, &msg.id).await {
                        warn!("Failed to create consolidated_from edge: {e}");
                    }
                }
            }

            // Entity extraction from consolidated memory
            {
                let entity_text = format!("{} {}", summary_for_entities, detail_for_entities);
                let extracted_entities = crate::entities::extract_entities_heuristic(&entity_text, &[]);
                if let Ok(record_id) = get_memory_record_id(db, &d_tag).await {
                    for entity in &extracted_entities {
                        match crate::db::store_entity(db, &entity.name, &entity.kind).await {
                            Ok(entity_id) => {
                                // Parse entity_id to get just the ID part
                                let eid = entity_id.split_once(':').map(|(_, id)| id).unwrap_or(&entity_id);
                                let mid = record_id.split_once(':').map(|(_, id)| id).unwrap_or(&record_id);
                                if let Err(e) = crate::db::create_mention_edge(db, mid, eid, entity.relevance).await {
                                    warn!("Failed to create mention edge for entity '{}': {e}", entity.name);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to store entity '{}': {e}", entity.name);
                            }
                        }
                    }
                    if !extracted_entities.is_empty() {
                        debug!(
                            topic = %memory.topic,
                            count = extracted_entities.len(),
                            "Extracted entities from consolidated memory"
                        );
                    }
                }
            }

            // Track channel for reporting
            let channel = group_msgs.first().map(|m| m.channel.as_str()).unwrap_or("general");
            if !channel.is_empty() && !report.channels.contains(&channel.to_string()) {
                report.channels.push(channel.to_string());
            }
        }

        // Collect message IDs for this group
        for msg in group_msgs {
            all_consumed_msg_ids.push(msg.id.clone());
        }
    }

    if config.dry_run {
        return Ok(report);
    }

    // Mark all processed messages as consolidated
    if !all_consumed_msg_ids.is_empty() {
        crate::db::mark_messages_consolidated(db, &all_consumed_msg_ids).await?;
    }

    // Publish NIP-09 deletion events for consumed ephemerals
    if let Some(relay) = relay {
        let deleted = publish_deletion_events(relay, &messages).await?;
        report.events_deleted = deleted;
    }

    debug!(
        memories = report.memories_created,
        messages = report.messages_processed,
        deleted = report.events_deleted,
        "Consolidation complete"
    );

    Ok(report)
}

/// Publish NIP-09 kind 5 deletion events for consumed ephemeral messages.
///
/// Groups deletions by batch to avoid excessively large events.
async fn publish_deletion_events(
    relay: &RelayManager,
    messages: &[RawMessageRecord],
) -> Result<usize> {
    // Collect any messages that have nostr source_ids (these are the event IDs on relay)
    let event_ids: Vec<&str> = messages
        .iter()
        .filter(|m| !m.source_id.is_empty() && m.source == "nostr")
        .map(|m| m.source_id.as_str())
        .collect();

    if event_ids.is_empty() {
        debug!("No Nostr event IDs to delete (messages may be from non-Nostr sources)");
        return Ok(0);
    }

    // Batch deletion events (max 50 e-tags per event)
    let mut deleted = 0usize;
    for chunk in event_ids.chunks(50) {
        let mut tags = Vec::new();
        for eid_str in chunk {
            if let Ok(eid) = EventId::from_hex(eid_str) {
                tags.push(Tag::event(eid));
            }
        }

        if tags.is_empty() {
            continue;
        }

        let delete_builder = EventBuilder::new(Kind::Custom(5), "consolidated")
            .tags(tags);

        match relay.publish(delete_builder).await {
            Ok(result) => {
                deleted += chunk.len();
                debug!(
                    event_id = %result.event_id,
                    count = chunk.len(),
                    "Published NIP-09 deletion event"
                );
            }
            Err(e) => {
                warn!("Failed to publish deletion event: {e}");
            }
        }
    }

    Ok(deleted)
}

/// Fetch an existing memory record by d_tag for merge checks.
async fn get_existing_memory(db: &Surreal<Db>, d_tag: &str) -> Result<Option<ExistingMemory>> {
    #[derive(Deserialize)]
    struct Row {
        content: String,
        #[serde(default, deserialize_with = "crate::db::deserialize_option_string")]
        summary: Option<String>,
        version: i64,
        confidence: Option<f64>,
    }
    let rows: Vec<Row> = db
        .query("SELECT content, summary, version, confidence FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(rows.into_iter().next().map(|r| ExistingMemory {
        content: r.content,
        summary: r.summary,
        version: r.version,
        confidence: r.confidence,
    }))
}

/// Existing memory data for merge operations.
struct ExistingMemory {
    content: String,
    summary: Option<String>,
    #[allow(dead_code)]
    version: i64,
    #[allow(dead_code)]
    confidence: Option<f64>,
}

/// Bump the version field on a memory record.
async fn bump_memory_version(db: &Surreal<Db>, d_tag: &str) -> Result<()> {
    db.query("UPDATE memory SET version = version + 1 WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?;
    Ok(())
}

/// Find a similar existing memory by embedding cosine similarity.
/// Returns the d_tag of the most similar memory if similarity > threshold.
async fn find_similar_memory(
    db: &Surreal<Db>,
    embedding: &[f32],
    threshold: f64,
) -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct SimRow {
        #[serde(default, deserialize_with = "crate::db::deserialize_option_string")]
        d_tag: Option<String>,
        similarity: Option<f64>,
    }
    let rows: Vec<SimRow> = db
        .query(
            "SELECT d_tag, vector::similarity::cosine(embedding, $vec) AS similarity \
             FROM memory WHERE embedding IS NOT NONE \
             ORDER BY similarity DESC LIMIT 1"
        )
        .bind(("vec", embedding.to_vec()))
        .await?
        .check()?
        .take(0)?;

    if let Some(row) = rows.first() {
        if let (Some(ref dtag), Some(sim)) = (&row.d_tag, row.similarity) {
            if sim >= threshold {
                return Ok(Some(dtag.clone()));
            }
        }
    }
    Ok(None)
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
