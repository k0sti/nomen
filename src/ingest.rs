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

/// Build tags for a kind 1235 raw source event from a RawMessage.
///
/// Content should be set separately as JSON: `{"text": msg.content, "metadata": msg.metadata}`.
pub fn build_raw_source_event(msg: &RawMessage) -> (Vec<nostr_sdk::Tag>, String) {
    use nostr_sdk::{Tag, TagKind};

    let mut tags = vec![
        Tag::custom(TagKind::Custom("source".into()), vec![msg.source.clone()]),
        Tag::custom(
            TagKind::Custom("channel".into()),
            vec![msg.channel.clone().unwrap_or_default()],
        ),
        Tag::custom(
            TagKind::Custom("sender".into()),
            vec![msg.sender.clone()],
        ),
        Tag::custom(TagKind::Custom("t".into()), vec!["raw".to_string()]),
    ];

    if let Some(ref room) = msg.room {
        tags.push(Tag::custom(TagKind::Custom("room".into()), vec![room.clone()]));
    }
    if let Some(ref topic) = msg.topic {
        tags.push(Tag::custom(TagKind::Custom("topic".into()), vec![topic.clone()]));
    }
    if let Some(ref thread) = msg.thread {
        tags.push(Tag::custom(TagKind::Custom("thread".into()), vec![thread.clone()]));
    }
    if let Some(ref sender_name) = msg.sender_name {
        tags.push(Tag::custom(
            TagKind::Custom("sender_name".into()),
            vec![sender_name.clone()],
        ));
    }
    if let Some(ref provider_id) = msg.provider_id {
        tags.push(Tag::custom(
            TagKind::Custom("provider_id".into()),
            vec![provider_id.clone()],
        ));
    }
    if let Some(ref source_ts) = msg.source_ts {
        tags.push(Tag::custom(
            TagKind::Custom("source_ts".into()),
            vec![source_ts.clone()],
        ));
    }
    if let Some(ref scope) = msg.scope {
        tags.push(Tag::custom(TagKind::Custom("scope".into()), vec![scope.clone()]));
    }

    // Build content JSON
    let content_json = if let Some(ref metadata) = msg.metadata {
        serde_json::json!({"text": msg.content, "metadata": serde_json::from_str::<serde_json::Value>(metadata).unwrap_or(serde_json::Value::Null)})
    } else {
        serde_json::json!({"text": msg.content})
    };

    (tags, content_json.to_string())
}

/// Parse a kind 1235 Nostr event into a RawMessage.
pub fn parse_raw_source_event(event: &nostr_sdk::Event) -> Result<RawMessage> {
    use crate::memory::get_tag_value;

    let source = get_tag_value(&event.tags, "source")
        .ok_or_else(|| anyhow::anyhow!("Missing 'source' tag in raw source event"))?;
    let channel = get_tag_value(&event.tags, "channel");
    let sender = get_tag_value(&event.tags, "sender").unwrap_or_default();
    let sender_name = get_tag_value(&event.tags, "sender_name");
    let room = get_tag_value(&event.tags, "room");
    let topic = get_tag_value(&event.tags, "topic");
    let thread = get_tag_value(&event.tags, "thread");
    let provider_id = get_tag_value(&event.tags, "provider_id");
    let source_ts = get_tag_value(&event.tags, "source_ts");
    let scope = get_tag_value(&event.tags, "scope");

    // Parse content JSON
    let (content, metadata) = if let Ok(parsed) =
        serde_json::from_str::<serde_json::Value>(&event.content)
    {
        let text = parsed
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or(&event.content)
            .to_string();
        let meta = parsed
            .get("metadata")
            .map(|v| v.to_string())
            .filter(|s| s != "null");
        (text, meta)
    } else {
        (event.content.clone(), None)
    };

    // Use event.created_at as created_at (Nostr publish time)
    let created_at = {
        let secs = event.created_at.as_u64() as i64;
        chrono::DateTime::from_timestamp(secs, 0)
            .map(|dt| dt.to_rfc3339())
    };

    Ok(RawMessage {
        source,
        source_id: provider_id.clone(), // backward compat: source_id = provider_id
        sender,
        channel,
        content,
        metadata,
        created_at,
        provider_id,
        sender_name,
        room,
        topic,
        thread,
        scope,
        source_ts,
    })
}
