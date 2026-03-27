use serde::{Deserialize, Serialize};

/// Time window for grouping messages (4 hours in seconds).
pub(crate) const TIME_WINDOW_SECS: i64 = 4 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationMessage {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub source: String,
    pub community: String,
    pub chat: String,
    pub thread: String,
    pub container: String,
    pub created_at: String,
    pub created_at_ts: i64,
}

/// A memory extracted by the LLM from a batch of messages.
#[derive(Debug, Clone)]
pub struct ExtractedMemory {
    pub content: String,
    pub topic: String,
    /// Importance score (1-10). Higher = more important to remember.
    pub importance: Option<i32>,
    /// Whether this memory contradicts existing information (set during merge).
    pub contradicts_existing: bool,
}

/// Configuration for the consolidation pipeline.
pub struct ConsolidationConfig {
    pub batch_size: usize,
    pub min_messages: usize,
    pub llm_provider: Box<dyn super::LlmProvider>,
    /// Entity extractor (heuristic, LLM, or composite).
    pub entity_extractor: Box<dyn crate::entities::EntityExtractor>,
    /// If true, preview what would be consolidated without publishing.
    pub dry_run: bool,
    /// Only consolidate messages older than this duration string (e.g. "30m", "1h", "7d").
    pub older_than: Option<String>,
    /// Only consolidate messages matching this tier.
    pub tier: Option<String>,
    /// Restrict consolidation input to matching normalized platform(s).
    pub platform: Option<Vec<String>>,
    /// Restrict consolidation input to matching normalized community/container ids.
    pub community_id: Option<Vec<String>>,
    /// Restrict consolidation input to matching normalized chat ids.
    pub chat_id: Option<Vec<String>>,
    /// Restrict consolidation input to matching normalized thread/topic ids.
    pub thread_id: Option<Vec<String>>,
    /// Restrict consolidation input to messages created at/after this unix timestamp.
    pub since: Option<i64>,
    /// Author's hex pubkey, used for v0.2 d-tag context on personal/internal memories.
    pub author_pubkey: Option<String>,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            batch_size: 50,
            min_messages: 3,
            llm_provider: Box::new(super::NoopLlmProvider),
            entity_extractor: Box::new(crate::entities::HeuristicExtractor),
            dry_run: false,
            older_than: None,
            tier: None,
            platform: None,
            community_id: None,
            chat_id: None,
            thread_id: None,
            since: None,
            author_pubkey: None,
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
    pub events_published: usize,
    /// Legacy reporting field name. Values represent the primary conversation
    /// container used during grouping (typically chat or chat/thread identity).
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
pub(crate) struct GroupKey {
    /// Sender identity for private batches, or primary conversation container
    /// identity for group/public batches. Prefer canonical chat/thread
    /// identity, with legacy `channel` only as a fallback.
    pub identity: String,
    /// Time window index (created_at / TIME_WINDOW_SECS)
    pub window: i64,
    /// Resolved scope prevents cross-group consolidation (TODO #7).
    /// Messages from different scopes are never mixed in the same batch.
    pub scope: String,
}

/// A prepared batch for two-phase consolidation.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PreparedBatch {
    pub batch_id: String,
    pub scope: String,
    pub visibility: String,
    pub message_count: usize,
    pub time_range: TimeRange,
    pub messages: Vec<BatchMessage>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TimeRange {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BatchMessage {
    pub id: String,
    pub sender: String,
    pub content: String,
    /// Legacy compatibility field. Prefer `chat`/`thread` and `container`.
    pub channel: String,
    /// Primary conversation identity used for grouping/reporting.
    pub container: String,
    pub chat: String,
    pub thread: String,
    pub source: String,
    pub created_at: String,
}

/// Result of the prepare phase.
#[derive(Debug, Serialize, Deserialize)]
pub struct PrepareResult {
    pub session_id: Option<String>,
    pub expires_at: Option<String>,
    pub batch_count: usize,
    pub message_count: usize,
    pub batches: Vec<PreparedBatch>,
}

/// An extraction provided by the agent for one batch.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BatchExtraction {
    pub batch_id: String,
    pub memories: Vec<AgentExtractedMemory>,
}

/// A memory extracted by the agent.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentExtractedMemory {
    pub topic: String,
    pub content: String,
    pub importance: u8,
    #[serde(default)]
    pub entities: Vec<String>,
}

/// Result of the commit phase.
#[derive(Debug, Serialize, Deserialize)]
pub struct CommitResult {
    pub session_id: String,
    pub memories_created: usize,
    pub memories_merged: usize,
    pub memories_deduped: usize,
    pub messages_consolidated: usize,
    pub events_published: usize,
    pub events_deleted: usize,
}

/// Status of whether consolidation is due.
#[derive(Debug, serde::Serialize)]
pub struct ConsolidationStatus {
    /// Whether consolidation should run now.
    pub due: bool,
    /// Reason why consolidation is or isn't due.
    pub reason: String,
    /// Timestamp of last consolidation run (RFC3339), if any.
    pub last_run: Option<String>,
    /// Hours since last run.
    pub hours_since_last_run: Option<f64>,
    /// Current unconsolidated message count.
    pub pending_messages: usize,
    /// Configured interval in hours.
    pub interval_hours: u32,
    /// Configured max ephemeral count threshold.
    pub max_ephemeral_count: usize,
}

/// Existing memory data for merge operations.
pub(crate) struct ExistingMemory {
    pub content: String,
    #[allow(dead_code)]
    pub version: i64,
}

/// Parse a duration string like "30m", "1h", "7d" into seconds.
pub fn parse_duration_str(s: &str) -> anyhow::Result<i64> {
    let s = s.trim();
    if s.is_empty() {
        anyhow::bail!("Empty duration string");
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid duration number: {num_str}"))?;

    match unit {
        "s" => Ok(num),
        "m" => Ok(num * 60),
        "h" => Ok(num * 3600),
        "d" => Ok(num * 86400),
        "w" => Ok(num * 604800),
        _ => anyhow::bail!("Unknown duration unit: {unit}. Use s, m, h, d, or w"),
    }
}

/// Calculate hours since an RFC3339 timestamp.
pub(crate) fn hours_since(timestamp: &str) -> Option<f64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| {
            let duration = chrono::Utc::now() - dt.with_timezone(&chrono::Utc);
            duration.num_minutes() as f64 / 60.0
        })
}
