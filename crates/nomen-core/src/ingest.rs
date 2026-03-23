use serde::{Deserialize, Serialize};

/// A raw message from any source (telegram, nostr, webhook, CLI, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawMessage {
    pub source: String,
    pub source_id: Option<String>,
    pub sender: String,
    pub channel: Option<String>,
    pub content: String,
    pub metadata: Option<String>,
    pub created_at: Option<String>,
}

/// Query options for fetching raw messages.
#[derive(Debug, Default)]
pub struct MessageQuery {
    pub source: Option<String>,
    pub channel: Option<String>,
    pub sender: Option<String>,
    pub since: Option<String>,
    pub limit: Option<usize>,
    pub consolidated_only: bool,
}
