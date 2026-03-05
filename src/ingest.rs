use anyhow::Result;
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tracing::debug;

/// A raw message from any source (telegram, nostr, webhook, CLI, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMessage {
    pub source: String,
    pub source_id: Option<String>,
    pub sender: String,
    pub channel: Option<String>,
    pub content: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: Option<String>,
}

/// A raw message as stored in SurrealDB (with DB-assigned id and consolidated flag).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMessageRecord {
    #[serde(default)]
    pub id: String,
    pub source: String,
    pub source_id: Option<String>,
    pub sender: String,
    pub channel: Option<String>,
    pub content: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
    #[serde(default)]
    pub consolidated: bool,
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

/// Ingest a raw message into SurrealDB.
pub async fn ingest_message(db: &Surreal<Db>, msg: &RawMessage) -> Result<String> {
    debug!(source = %msg.source, sender = %msg.sender, "Ingesting message");
    let id = crate::db::store_raw_message(db, msg).await?;
    Ok(id)
}

/// Query raw messages with filters.
pub async fn get_messages(db: &Surreal<Db>, opts: &MessageQuery) -> Result<Vec<RawMessageRecord>> {
    crate::db::query_raw_messages(db, opts).await
}
