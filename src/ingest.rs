use anyhow::Result;
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
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
    /// Provider's native message/event ID (e.g. Telegram message_id, Discord snowflake).
    pub provider_id: Option<String>,
    /// Human-readable sender name (for display, not identity).
    pub sender_name: Option<String>,
    /// Room/group identity (provider-qualified).
    pub room: Option<String>,
    /// Forum topic / thread ID within a room.
    pub topic: Option<String>,
    /// Thread ID if distinct from topic (e.g. Discord nested threads).
    pub thread: Option<String>,
    /// Resolved Nomen scope if known at ingest time.
    pub scope: Option<String>,
    /// Original provider timestamp (unix seconds string).
    pub source_ts: Option<String>,
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
    /// Nostr event ID for this raw message (durable identity).
    #[serde(default)]
    #[surreal(default)]
    pub nostr_event_id: String,
    /// Provider's native message/event ID.
    #[serde(default)]
    #[surreal(default)]
    pub provider_id: String,
    /// Provider user/sender ID (distinct from human-readable `sender`).
    #[serde(default)]
    #[surreal(default)]
    pub sender_id: String,
    /// Room/group identity (provider-qualified).
    #[serde(default)]
    #[surreal(default)]
    pub room: String,
    /// Forum topic / thread ID within a room.
    #[serde(default)]
    #[surreal(default)]
    pub topic: String,
    /// Thread ID if distinct from topic.
    #[serde(default)]
    #[surreal(default)]
    pub thread: String,
    /// Resolved Nomen scope.
    #[serde(default)]
    #[surreal(default)]
    pub scope: String,
    /// Original provider timestamp.
    #[serde(default)]
    #[surreal(default)]
    pub source_created_at: String,
    /// Publish status: "published", "pending", "failed".
    #[serde(default)]
    #[surreal(default)]
    pub publish_status: String,
}

/// A raw message search result (RawMessageRecord + BM25 score).
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct RawMessageSearchResult {
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
    #[serde(default)]
    #[surreal(default)]
    pub nostr_event_id: String,
    #[serde(default)]
    #[surreal(default)]
    pub provider_id: String,
    #[serde(default)]
    #[surreal(default)]
    pub sender_id: String,
    #[serde(default)]
    #[surreal(default)]
    pub room: String,
    #[serde(default)]
    #[surreal(default)]
    pub topic: String,
    #[serde(default)]
    #[surreal(default)]
    pub thread: String,
    #[serde(default)]
    #[surreal(default)]
    pub scope: String,
    #[serde(default)]
    #[surreal(default)]
    pub source_created_at: String,
    #[serde(default)]
    #[surreal(default)]
    pub publish_status: String,
    #[serde(default)]
    #[surreal(default)]
    pub score: f64,
}

/// Query options for fetching raw messages.
#[derive(Debug, Default)]
pub struct MessageQuery {
    pub source: Option<String>,
    pub channel: Option<String>,
    pub sender: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub room: Option<String>,
    pub topic: Option<String>,
    pub thread: Option<String>,
    pub limit: Option<usize>,
    /// When true, don't filter by consolidated status at all.
    /// When false (default), only return unconsolidated messages.
    pub include_consolidated: bool,
    /// Sort order: "asc" or "desc". Default is "desc".
    pub order: Option<String>,
}

/// Ingest a raw message into SurrealDB, with dedup check.
pub async fn ingest_message(db: &Surreal<Db>, msg: &RawMessage) -> Result<String> {
    debug!(source = %msg.source, sender = %msg.sender, "Ingesting message");

    // Dedup: check by provider_id + channel if provider_id is present
    if let Some(ref pid) = msg.provider_id {
        let channel = msg.channel.as_deref().unwrap_or("");
        if crate::db::check_duplicate_raw_message(db, pid, channel).await? {
            debug!(provider_id = %pid, "Duplicate raw message, skipping");
            return Ok("duplicate".to_string());
        }
    }

    let id = crate::db::store_raw_message(db, msg).await?;
    Ok(id)
}

/// Query raw messages with filters.
pub async fn get_messages(db: &Surreal<Db>, opts: &MessageQuery) -> Result<Vec<RawMessageRecord>> {
    crate::db::query_raw_messages(db, opts).await
}

/// Build a kind 1235 raw source event from a RawMessage.
///
/// Content is JSON: `{"text": ..., "metadata": ...}`.
/// Tags encode structured identity and container fields per the raw-source-event-spec.
pub fn build_raw_source_event(msg: &RawMessage) -> EventBuilder {
    let metadata_value: serde_json::Value = msg
        .metadata
        .as_deref()
        .and_then(|m| serde_json::from_str(m).ok())
        .unwrap_or(serde_json::Value::Null);

    let content = json!({
        "text": &msg.content,
        "metadata": metadata_value,
    });

    let mut tags = vec![
        Tag::custom(TagKind::Custom("source".into()), vec![msg.source.clone()]),
        Tag::custom(TagKind::Custom("sender".into()), vec![msg.sender.clone()]),
        Tag::custom(TagKind::Custom("t".into()), vec!["raw".to_string()]),
    ];
    if let Some(ref ch) = msg.channel {
        tags.push(Tag::custom(
            TagKind::Custom("channel".into()),
            vec![ch.clone()],
        ));
    }
    if let Some(ref room) = msg.room {
        tags.push(Tag::custom(
            TagKind::Custom("room".into()),
            vec![room.clone()],
        ));
    }
    if let Some(ref topic) = msg.topic {
        tags.push(Tag::custom(
            TagKind::Custom("topic".into()),
            vec![topic.clone()],
        ));
    }
    if let Some(ref thread) = msg.thread {
        tags.push(Tag::custom(
            TagKind::Custom("thread".into()),
            vec![thread.clone()],
        ));
    }
    if let Some(ref pid) = msg.provider_id {
        tags.push(Tag::custom(
            TagKind::Custom("provider_id".into()),
            vec![pid.clone()],
        ));
    }
    if let Some(ref sender_name) = msg.sender_name {
        tags.push(Tag::custom(
            TagKind::Custom("sender_name".into()),
            vec![sender_name.clone()],
        ));
    }
    if let Some(ref scope) = msg.scope {
        tags.push(Tag::custom(
            TagKind::Custom("scope".into()),
            vec![scope.clone()],
        ));
    }
    if let Some(ref ts) = msg.source_ts {
        tags.push(Tag::custom(
            TagKind::Custom("source_ts".into()),
            vec![ts.clone()],
        ));
    }

    EventBuilder::new(Kind::Custom(crate::kinds::RAW_SOURCE_KIND), content.to_string()).tags(tags)
}

/// Parse a kind 1235 event back into a RawMessage (for sync/import).
pub fn parse_raw_source_event(event: &Event) -> RawMessage {
    let tags = &event.tags;

    let get = |name: &str| -> Option<String> {
        crate::memory::get_tag_value(tags, name)
    };

    // Parse content JSON for text and metadata
    let (text, metadata) = match serde_json::from_str::<serde_json::Value>(&event.content) {
        Ok(obj) => {
            let text = obj
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or(&event.content)
                .to_string();
            let metadata = obj.get("metadata").map(|v| v.to_string());
            (text, metadata)
        }
        Err(_) => (event.content.to_string(), None),
    };

    RawMessage {
        source: get("source").unwrap_or_default(),
        source_id: None,
        sender: get("sender").unwrap_or_default(),
        channel: get("channel"),
        content: text,
        metadata,
        created_at: Some(
            chrono::DateTime::from_timestamp(event.created_at.as_u64() as i64, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
        ),
        provider_id: get("provider_id"),
        sender_name: get("sender_name"),
        room: get("room"),
        topic: get("topic"),
        thread: get("thread"),
        scope: get("scope"),
        source_ts: get("source_ts"),
    }
}
