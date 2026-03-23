use anyhow::Result;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
#[allow(unused_imports)]
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;
use tracing::debug;

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

/// A raw message as stored in SurrealDB (with DB-assigned id and consolidated flag).
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct RawMessageRecord {
    #[serde(default, deserialize_with = "crate::db::deserialize_thing_as_string")]
    #[surreal(default)]
    pub id: String,
    #[serde(default)]
    #[surreal(default)]
    pub source: String,
    #[serde(default)]
    #[surreal(default)]
    pub source_id: String,
    #[serde(default)]
    #[surreal(default)]
    pub sender: String,
    #[serde(default)]
    #[surreal(default)]
    pub channel: String,
    #[serde(default)]
    #[surreal(default)]
    pub content: String,
    #[serde(default)]
    #[surreal(default)]
    pub metadata: String,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: String,
    #[serde(default)]
    #[surreal(default)]
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
