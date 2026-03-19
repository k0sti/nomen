//! Message domain operations: ingest, list, context, search, send.

use serde_json::{json, Value};

use crate::api::errors::ApiError;
use crate::ingest;
use crate::send;
use crate::Nomen;

/// Serialize a RawMessageRecord to a JSON Value with all fields.
fn raw_message_to_json(m: &ingest::RawMessageRecord) -> Value {
    json!({
        "id": m.id,
        "source": m.source,
        "source_id": m.source_id,
        "sender": m.sender,
        "channel": m.channel,
        "content": m.content,
        "metadata": m.metadata,
        "created_at": m.created_at,
        "consolidated": m.consolidated,
        "nostr_event_id": m.nostr_event_id,
        "provider_id": m.provider_id,
        "sender_id": m.sender_id,
        "room": m.room,
        "topic": m.topic,
        "thread": m.thread,
        "scope": m.scope,
        "source_created_at": m.source_created_at,
        "publish_status": m.publish_status,
    })
}

pub async fn ingest(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let content = params
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if content.is_empty() {
        return Err(ApiError::invalid_params("content is required"));
    }

    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let sender = params
        .get("sender")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let channel = params
        .get("channel")
        .and_then(|v| v.as_str())
        .map(String::from);
    let source_id = params
        .get("source_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let metadata = params.get("metadata").map(|v| v.to_string());

    let msg = ingest::RawMessage {
        source: source.clone(),
        source_id,
        sender: sender.clone(),
        channel: channel.clone(),
        content,
        metadata,
        created_at: None,
        ..Default::default()
    };

    let id = nomen
        .ingest_message(msg)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "id": id,
        "source": source,
        "channel": channel,
    }))
}

pub async fn list(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let include_consolidated = params
        .get("include_consolidated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let opts = ingest::MessageQuery {
        source: params
            .get("source")
            .and_then(|v| v.as_str())
            .map(String::from),
        channel: params
            .get("channel")
            .and_then(|v| v.as_str())
            .map(String::from),
        sender: params
            .get("sender")
            .and_then(|v| v.as_str())
            .map(String::from),
        since: params
            .get("since")
            .and_then(|v| v.as_str())
            .map(String::from),
        until: params
            .get("until")
            .and_then(|v| v.as_str())
            .map(String::from),
        room: params
            .get("room")
            .and_then(|v| v.as_str())
            .map(String::from),
        topic: params
            .get("topic")
            .and_then(|v| v.as_str())
            .map(String::from),
        thread: params
            .get("thread")
            .and_then(|v| v.as_str())
            .map(String::from),
        limit: Some(params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize),
        include_consolidated,
        order: params
            .get("order")
            .and_then(|v| v.as_str())
            .map(String::from),
    };

    let messages = nomen
        .get_messages(opts)
        .await
        .map_err(ApiError::from_anyhow)?;

    let msg_values: Vec<Value> = messages.iter().map(raw_message_to_json).collect();

    Ok(json!({
        "count": msg_values.len(),
        "messages": msg_values,
    }))
}

pub async fn context(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    // Accept multiple anchor types
    let anchor_id = params.get("id").and_then(|v| v.as_str());
    let anchor_nostr = params.get("nostr_event_id").and_then(|v| v.as_str());
    let anchor_provider = params.get("provider_id").and_then(|v| v.as_str());
    let anchor_channel = params.get("channel").and_then(|v| v.as_str());
    let anchor_source_id = params
        .get("source_id")
        .and_then(|v| v.as_str());

    // At least one anchor must be provided
    if anchor_id.is_none()
        && anchor_nostr.is_none()
        && anchor_provider.is_none()
        && anchor_source_id.is_none()
    {
        return Err(ApiError::invalid_params(
            "At least one anchor is required: id, nostr_event_id, provider_id, or source_id",
        ));
    }

    let before = params.get("before").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let after = params.get("after").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let same_container_only = params
        .get("same_container_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // Look up the target message directly
    let target = crate::db::get_message_by_anchor(
        nomen.db(),
        anchor_id,
        anchor_nostr,
        anchor_provider,
        anchor_channel,
        anchor_source_id,
    )
    .await
    .map_err(ApiError::from_anyhow)?;

    let target = match target {
        Some(t) => t,
        None => {
            return Err(ApiError::not_found("Anchor message not found"));
        }
    };

    // Fetch surrounding messages by timestamp
    let target_time = &target.created_at;

    // Build query for messages before the anchor
    let mut before_conditions = vec![format!("created_at < $target_time")];
    let mut after_conditions = vec![format!("created_at > $target_time")];

    if same_container_only {
        // Constrain to same room + topic + thread
        if !target.room.is_empty() {
            before_conditions.push("room = $room".to_string());
            after_conditions.push("room = $room".to_string());
        }
        if !target.topic.is_empty() {
            before_conditions.push("topic = $topic".to_string());
            after_conditions.push("topic = $topic".to_string());
        }
        if !target.thread.is_empty() {
            before_conditions.push("thread = $thread".to_string());
            after_conditions.push("thread = $thread".to_string());
        }
        // If no container fields set, fall back to channel
        if target.room.is_empty() && target.topic.is_empty() && target.thread.is_empty() {
            before_conditions.push("channel = $channel".to_string());
            after_conditions.push("channel = $channel".to_string());
        }
    }

    let before_where = format!("WHERE {}", before_conditions.join(" AND "));
    let after_where = format!("WHERE {}", after_conditions.join(" AND "));

    let fields = crate::db::RAW_MSG_SELECT_FIELDS;
    let before_sql = format!(
        "SELECT {fields} FROM raw_message {before_where} ORDER BY created_at DESC LIMIT {before}"
    );
    let after_sql = format!(
        "SELECT {fields} FROM raw_message {after_where} ORDER BY created_at ASC LIMIT {after}"
    );

    // Execute before query
    let mut q = nomen.db().query(&before_sql);
    q = q.bind(("target_time", target_time.clone()));
    if same_container_only {
        if !target.room.is_empty() {
            q = q.bind(("room", target.room.clone()));
        }
        if !target.topic.is_empty() {
            q = q.bind(("topic", target.topic.clone()));
        }
        if !target.thread.is_empty() {
            q = q.bind(("thread", target.thread.clone()));
        }
        if target.room.is_empty() && target.topic.is_empty() && target.thread.is_empty() {
            q = q.bind(("channel", target.channel.clone()));
        }
    }
    let before_result = q.await.map_err(|e| ApiError::from_anyhow(e.into()))?;
    let mut checked = before_result.check().map_err(|e| ApiError::from_anyhow(e.into()))?;
    let mut before_msgs: Vec<ingest::RawMessageRecord> =
        checked.take(0).map_err(|e| ApiError::from_anyhow(e.into()))?;
    // Reverse so they're in chronological order
    before_msgs.reverse();

    // Execute after query
    let mut q = nomen.db().query(&after_sql);
    q = q.bind(("target_time", target_time.clone()));
    if same_container_only {
        if !target.room.is_empty() {
            q = q.bind(("room", target.room.clone()));
        }
        if !target.topic.is_empty() {
            q = q.bind(("topic", target.topic.clone()));
        }
        if !target.thread.is_empty() {
            q = q.bind(("thread", target.thread.clone()));
        }
        if target.room.is_empty() && target.topic.is_empty() && target.thread.is_empty() {
            q = q.bind(("channel", target.channel.clone()));
        }
    }
    let after_result = q.await.map_err(|e| ApiError::from_anyhow(e.into()))?;
    let mut checked = after_result.check().map_err(|e| ApiError::from_anyhow(e.into()))?;
    let after_msgs: Vec<ingest::RawMessageRecord> =
        checked.take(0).map_err(|e| ApiError::from_anyhow(e.into()))?;

    // Assemble: before + target + after
    let target_index = before_msgs.len();
    let mut all_msgs = before_msgs;
    all_msgs.push(target);
    all_msgs.extend(after_msgs);

    let context_messages: Vec<Value> = all_msgs.iter().map(raw_message_to_json).collect();

    Ok(json!({
        "count": context_messages.len(),
        "messages": context_messages,
        "target_index": target_index,
    }))
}

/// Generate a ~100-char snippet around the first case-insensitive match of `query` in `content`,
/// with the matched substring wrapped in `**bold**` markers.
/// Returns `None` if the query is not found in the content.
fn highlight_snippet(content: &str, query: &str) -> Option<String> {
    let content_lower = content.to_lowercase();
    let query_lower = query.to_lowercase();

    // Find the first occurrence (case-insensitive)
    let match_start = content_lower.find(&query_lower)?;
    let match_end = match_start + query.len();

    // Calculate a window of ~100 chars centered on the match
    let half_window = 50usize;
    let window_start = match_start.saturating_sub(half_window);
    // Snap to nearest char boundary
    let window_start = content
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= window_start)
        .last()
        .unwrap_or(0);

    let window_end = (match_end + half_window).min(content.len());
    let window_end = content
        .char_indices()
        .map(|(i, _)| i)
        .find(|&i| i >= window_end)
        .unwrap_or(content.len());

    let prefix = if window_start > 0 { "..." } else { "" };
    let suffix = if window_end < content.len() { "..." } else { "" };

    // Build snippet with bold markers around the original-case matched text
    let before = &content[window_start..match_start];
    let matched = &content[match_start..match_end];
    let after = &content[match_end..window_end];

    Some(format!("{prefix}{before}**{matched}**{after}{suffix}"))
}

/// Full-text BM25 search over raw messages.
pub async fn search(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if query.is_empty() {
        return Err(ApiError::invalid_params("query is required"));
    }

    let source = params.get("source").and_then(|v| v.as_str());
    let room = params.get("room").and_then(|v| v.as_str());
    let topic = params.get("topic").and_then(|v| v.as_str());
    let sender = params.get("sender").and_then(|v| v.as_str());
    let since = params.get("since").and_then(|v| v.as_str());
    let until = params.get("until").and_then(|v| v.as_str());
    let include_consolidated = params
        .get("include_consolidated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

    let results = nomen
        .search_messages(
            query,
            source,
            room,
            topic,
            sender,
            since,
            until,
            include_consolidated,
            limit,
        )
        .await
        .map_err(ApiError::from_anyhow)?;

    let msg_values: Vec<Value> = results
        .iter()
        .map(|m| {
            let snippet = highlight_snippet(&m.content, query);
            json!({
                "id": m.id,
                "source": m.source,
                "source_id": m.source_id,
                "sender": m.sender,
                "channel": m.channel,
                "content": m.content,
                "metadata": m.metadata,
                "created_at": m.created_at,
                "consolidated": m.consolidated,
                "nostr_event_id": m.nostr_event_id,
                "provider_id": m.provider_id,
                "sender_id": m.sender_id,
                "room": m.room,
                "topic": m.topic,
                "thread": m.thread,
                "scope": m.scope,
                "source_created_at": m.source_created_at,
                "publish_status": m.publish_status,
                "score": m.score,
                "snippet": snippet,
            })
        })
        .collect();

    Ok(json!({
        "count": msg_values.len(),
        "messages": msg_values,
    }))
}

pub async fn send_message(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let recipient = params
        .get("recipient")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let channel = params
        .get("channel")
        .and_then(|v| v.as_str())
        .map(String::from);
    let metadata = params.get("metadata").cloned();

    if recipient.is_empty() || content.is_empty() {
        return Err(ApiError::invalid_params(
            "recipient and content are required",
        ));
    }

    let target = send::parse_recipient(recipient).map_err(ApiError::from_anyhow)?;
    let opts = send::SendOptions {
        target,
        content: content.to_string(),
        channel,
        metadata,
    };

    let result = nomen.send(opts).await.map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "event_id": result.event_id,
        "accepted": result.accepted.len(),
        "rejected": result.rejected.len(),
        "summary": result.summary(),
    }))
}
