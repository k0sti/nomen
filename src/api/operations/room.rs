//! Room domain operations: resolve, bind, unbind.

use serde_json::{json, Value};

use crate::api::errors::ApiError;
use crate::Nomen;

/// Resolve room context for a provider ID.
///
/// Returns matching memories for all d-tags bound to this provider ID.
pub async fn resolve(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let provider_id = params
        .get("provider_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if provider_id.is_empty() {
        return Err(ApiError::invalid_params("provider_id is required"));
    }

    // Look up d-tags bound to this provider ID
    let d_tags = nomen
        .resolve_provider(provider_id)
        .await
        .map_err(ApiError::from_anyhow)?;

    if d_tags.is_empty() {
        return Ok(json!({
            "provider_id": provider_id,
            "count": 0,
            "results": [],
            "by_d_tag": {},
        }));
    }

    // Fetch all bound memories
    let records = nomen
        .get_batch(&d_tags)
        .await
        .map_err(ApiError::from_anyhow)?;

    let results: Vec<Value> = records
        .iter()
        .map(|m| {
            json!({
                "topic": m.topic,
                "summary": m.summary,
                "detail": m.content,
                "visibility": m.tier,
                "scope": m.scope,
                "confidence": m.confidence,
                "version": m.version,
                "created_at": m.created_at,
                "d_tag": m.d_tag,
            })
        })
        .collect();

    let mut by_dtag = serde_json::Map::new();
    for m in &records {
        if let Some(ref dt) = m.d_tag {
            by_dtag.insert(
                dt.clone(),
                json!({
                    "topic": m.topic,
                    "summary": m.summary,
                    "detail": m.content,
                    "visibility": m.tier,
                    "scope": m.scope,
                    "confidence": m.confidence,
                    "version": m.version,
                    "created_at": m.created_at,
                    "d_tag": m.d_tag,
                }),
            );
        }
    }

    Ok(json!({
        "provider_id": provider_id,
        "count": results.len(),
        "results": results,
        "by_d_tag": Value::Object(by_dtag),
    }))
}

/// Bind a provider ID to a room memory d-tag.
pub async fn bind(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let provider_id = params
        .get("provider_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let d_tag = params
        .get("d_tag")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if provider_id.is_empty() || d_tag.is_empty() {
        return Err(ApiError::invalid_params(
            "provider_id and d_tag are required",
        ));
    }

    nomen
        .bind_provider(provider_id, d_tag)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "bound": true,
        "provider_id": provider_id,
        "d_tag": d_tag,
    }))
}

/// Unbind a provider ID from a room memory d-tag.
pub async fn unbind(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let provider_id = params
        .get("provider_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let d_tag = params
        .get("d_tag")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if provider_id.is_empty() || d_tag.is_empty() {
        return Err(ApiError::invalid_params(
            "provider_id and d_tag are required",
        ));
    }

    let removed = nomen
        .unbind_provider(provider_id, d_tag)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "unbound": removed,
        "provider_id": provider_id,
        "d_tag": d_tag,
    }))
}
