//! Message domain operations: ingest, list, context, send.

use serde_json::{json, Value};

use crate::api::errors::ApiError;
use crate::ingest;
use crate::send;
use crate::Nomen;

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
        limit: Some(params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize),
        consolidated_only: false,
    };

    let messages = nomen
        .get_messages(opts)
        .await
        .map_err(ApiError::from_anyhow)?;

    let msg_values: Vec<Value> = messages
        .iter()
        .map(|m| {
            json!({
                "source": m.source,
                "sender": m.sender,
                "channel": m.channel,
                "content": m.content,
                "consolidated": m.consolidated,
                "created_at": m.created_at,
            })
        })
        .collect();

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
    let source_id = params
        .get("source_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if source_id.is_empty() {
        return Err(ApiError::invalid_params("source_id is required"));
    }

    let before = params.get("before").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let after = params.get("after").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    // Get the target message first to find its channel and timestamp
    let target_opts = ingest::MessageQuery {
        source: None,
        channel: None,
        sender: None,
        since: None,
        limit: None,
        consolidated_only: false,
    };

    let all_messages = nomen
        .get_messages(target_opts)
        .await
        .map_err(ApiError::from_anyhow)?;

    // Find the target message by source_id
    let target_idx = all_messages.iter().position(|m| m.source_id == source_id);

    let target_idx = match target_idx {
        Some(idx) => idx,
        None => {
            return Err(ApiError::not_found(format!(
                "Message not found: {source_id}"
            )))
        }
    };

    let start = target_idx.saturating_sub(before);
    let end = (target_idx + after + 1).min(all_messages.len());

    let context_messages: Vec<Value> = all_messages[start..end]
        .iter()
        .map(|m| {
            json!({
                "source": m.source,
                "sender": m.sender,
                "channel": m.channel,
                "content": m.content,
                "source_id": m.source_id,
                "consolidated": m.consolidated,
                "created_at": m.created_at,
            })
        })
        .collect();

    Ok(json!({
        "count": context_messages.len(),
        "messages": context_messages,
        "target_index": target_idx - start,
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
