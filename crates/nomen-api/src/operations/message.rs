//! Message domain operations: ingest, context, search, send, store, query.

use serde_json::{json, Value};

use crate::NomenBackend;
use nomen_core::api::errors::ApiError;
use nomen_core::collected::{CollectedEvent, CollectedEventFilter};
use nomen_core::kinds::COLLECTED_MESSAGE_KIND;
use nomen_core::send::{parse_recipient, SendOptions};

/// Ingest a message by converting it to a kind 30100 collected event.
pub async fn ingest(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
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
    // Canonical ingest fields: platform/community_id/chat_id/thread_id.
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| Some(source.clone()));
    let community_id = params
        .get("community_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let chat_id = params
        .get("chat_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let thread_id = params
        .get("thread_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let source_id = params
        .get("source_id")
        .and_then(|v| v.as_str())
        .map(String::from);

    let now = chrono::Utc::now().timestamp();
    let d_tag = if let Some(ref sid) = source_id {
        if let Some(ref chat) = chat_id {
            format!(
                "{}:{chat}:{sid}",
                platform.clone().unwrap_or_else(|| source.clone())
            )
        } else {
            format!("{source}:{sid}")
        }
    } else {
        format!("{source}:{now}:{sender}")
    };

    let plat = platform.clone().unwrap_or_else(|| source.clone());
    let mut tags = vec![
        vec!["d".to_string(), d_tag.clone()],
        vec!["platform".to_string(), plat.clone()],
        vec!["sender".to_string(), sender.clone()],
        // NIP-48 proxy tag for Nostr relay compatibility (optional)
        vec!["proxy".to_string(), d_tag.clone(), plat],
    ];
    if let Some(ref community) = community_id {
        tags.push(vec!["community".to_string(), community.clone()]);
    }
    if let Some(ref chat) = chat_id {
        tags.push(vec!["chat".to_string(), chat.clone()]);
    }
    if let Some(ref thread) = thread_id {
        tags.push(vec!["thread".to_string(), thread.clone()]);
    }

    let event = CollectedEvent {
        kind: COLLECTED_MESSAGE_KIND,
        created_at: now,
        pubkey: String::new(),
        tags,
        content,
        id: None,
        sig: None,
    };

    let result = nomen
        .store_collected_event(event)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "d_tag": result.d_tag,
        "stored": result.stored,
        "replaced": result.replaced,
        "source": source,
        "platform": platform,
        "community_id": community_id,
        "chat_id": chat_id,
        "thread_id": thread_id,
    }))
}

/// Query collected events with tag-based filtering.
pub async fn query(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let filter = CollectedEventFilter {
        platform: extract_string_array(params, "#platform"),
        community_id: extract_string_array(params, "#community"),
        chat_id: extract_string_array(params, "#chat"),
        sender_id: extract_string_array(params, "#sender"),
        thread_id: extract_string_array(params, "#thread"),
        since: params.get("since").and_then(|v| v.as_i64()),
        until: params.get("until").and_then(|v| v.as_i64()),
        limit: params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize),
    };

    let records = nomen
        .query_collected_events(filter)
        .await
        .map_err(ApiError::from_anyhow)?;

    let events: Vec<Value> = records
        .iter()
        .filter_map(|r| serde_json::from_str(&r.event_json).ok())
        .collect();

    Ok(json!({
        "count": events.len(),
        "events": events,
    }))
}

/// Retrieve message context using tag-based filters on collected_message.
pub async fn context(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let has_chat = params.get("#chat").is_some();
    if !has_chat {
        return Err(ApiError::invalid_params(
            "#chat filter is required for message.context",
        ));
    }

    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
    let before_ts = params.get("before").and_then(|v| v.as_i64());

    let filter = CollectedEventFilter {
        platform: extract_string_array(params, "#platform"),
        community_id: extract_string_array(params, "#community"),
        chat_id: extract_string_array(params, "#chat"),
        sender_id: extract_string_array(params, "#sender"),
        thread_id: extract_string_array(params, "#thread"),
        since: params.get("since").and_then(|v| v.as_i64()),
        until: before_ts,
        limit: Some(limit),
    };

    let records = nomen
        .query_collected_events(filter)
        .await
        .map_err(ApiError::from_anyhow)?;

    let messages: Vec<Value> = records
        .iter()
        .map(|r| {
            json!({
                "sender": r.sender_id,
                "platform": r.platform,
                "community": r.community_id,
                "chat": r.chat_id,
                "thread": r.thread_id,
                "message_id": r.message_id,
                "content": r.content,
                "created_at": r.created_at,
            })
        })
        .collect();

    Ok(json!({
        "count": messages.len(),
        "messages": messages,
    }))
}

/// BM25 fulltext search over collected messages.
pub async fn search(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let query_str = params.get("query").and_then(|v| v.as_str()).unwrap_or("");

    if query_str.is_empty() {
        return Err(ApiError::invalid_params("query is required"));
    }

    let filter = CollectedEventFilter {
        platform: extract_string_array(params, "#platform"),
        community_id: extract_string_array(params, "#community"),
        chat_id: extract_string_array(params, "#chat"),
        sender_id: extract_string_array(params, "#sender"),
        thread_id: extract_string_array(params, "#thread"),
        since: params.get("since").and_then(|v| v.as_i64()),
        until: params.get("until").and_then(|v| v.as_i64()),
        limit: params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize),
    };

    let results = nomen
        .search_collected_events(query_str, filter)
        .await
        .map_err(ApiError::from_anyhow)?;

    let events: Vec<Value> = results
        .iter()
        .map(|r| {
            let mut event: Value = serde_json::from_str(&r.event_json).unwrap_or(json!({}));
            if let Some(obj) = event.as_object_mut() {
                obj.insert("score".to_string(), json!(r.score));
            }
            event
        })
        .collect();

    Ok(json!({
        "count": events.len(),
        "events": events,
    }))
}

pub async fn send_message(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
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

    let target = parse_recipient(recipient).map_err(ApiError::from_anyhow)?;
    let opts = SendOptions {
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

/// Store a kind 30100 collected event.
pub async fn store(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let event_value = params
        .get("event")
        .ok_or_else(|| ApiError::invalid_params("event is required"))?;

    let event = CollectedEvent::from_json(event_value).map_err(|e| ApiError::invalid_params(e))?;

    event.validate().map_err(|e| ApiError::invalid_params(e))?;

    let result = nomen
        .store_collected_event(event)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "d_tag": result.d_tag,
        "stored": result.stored,
        "replaced": result.replaced,
    }))
}

/// Upload media to the configured media store.
pub async fn store_media(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let mime_type = params
        .get("mime_type")
        .and_then(|v| v.as_str())
        .unwrap_or("application/octet-stream");

    // Accept base64 data or file path
    let data = if let Some(b64) = params.get("data_base64").and_then(|v| v.as_str()) {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| ApiError::invalid_params(format!("invalid base64: {e}")))?
    } else if let Some(path) = params.get("data").and_then(|v| v.as_str()) {
        std::fs::read(path)
            .map_err(|e| ApiError::invalid_params(format!("cannot read file: {e}")))?
    } else {
        return Err(ApiError::invalid_params(
            "data (file path) or data_base64 is required",
        ));
    };

    let result = nomen
        .store_media(&data, mime_type)
        .await
        .map_err(ApiError::from_anyhow)?;

    match result {
        Some(media_ref) => Ok(json!({
            "sha256": media_ref.sha256,
            "path": media_ref.path,
            "size": media_ref.size,
            "mime_type": media_ref.mime_type,
        })),
        None => Err(ApiError::invalid_params("no media store configured")),
    }
}

/// Import historical messages from a platform.
///
/// Currently a stub — requires platform-specific import clients.
pub async fn import(_nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if platform.is_empty() {
        return Err(ApiError::invalid_params("platform is required"));
    }

    Err(ApiError::invalid_params(format!(
        "import from '{platform}' not yet implemented"
    )))
}

/// Fetch media for stored events that have original URLs but no local copies.
///
/// Currently a stub — requires HTTP download + event update logic.
pub async fn fetch_media(_nomen: &dyn NomenBackend, _params: &Value) -> Result<Value, ApiError> {
    Err(ApiError::invalid_params(
        "message.fetch_media not yet implemented",
    ))
}

/// Extract a string array from a JSON value field.
/// Accepts both a single string and an array of strings.
fn extract_string_array(params: &Value, key: &str) -> Option<Vec<String>> {
    let val = params.get(key)?;
    if let Some(arr) = val.as_array() {
        let strings: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if strings.is_empty() {
            None
        } else {
            Some(strings)
        }
    } else if let Some(s) = val.as_str() {
        Some(vec![s.to_string()])
    } else {
        None
    }
}
