//! Memory domain operations: search, put, get, list, delete.

use serde_json::{json, Value};

use crate::api::errors::ApiError;
use crate::api::types::{resolve_visibility_scope, RetrievalParams, Visibility};
use crate::search::SearchOptions;
use crate::Nomen;

pub async fn search(
    nomen: &Nomen,
    default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if query.is_empty() {
        return Err(ApiError::invalid_params("query is required"));
    }

    let (vis, scope) = resolve_visibility_scope(params, nomen, default_channel)?;
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    let retrieval: RetrievalParams = params
        .get("retrieval")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(|| {
            // Also check flat params for backward compat
            RetrievalParams {
                vector_weight: params.get("vector_weight").and_then(|v| v.as_f64()).unwrap_or(0.7) as f32,
                text_weight: params.get("text_weight").and_then(|v| v.as_f64()).unwrap_or(0.3) as f32,
                aggregate: params.get("aggregate").and_then(|v| v.as_bool()).unwrap_or(false),
                graph_expand: params.get("graph_expand").and_then(|v| v.as_bool()).unwrap_or(false),
                max_hops: params.get("max_hops").and_then(|v| v.as_u64()).unwrap_or(1) as usize,
            }
        });

    let tier = vis.as_ref().map(|v| match v {
        Visibility::Group => {
            if let Some(ref s) = scope {
                format!("group:{s}")
            } else {
                "group".to_string()
            }
        }
        other => other.as_str().to_string(),
    });

    let allowed_scopes = scope.map(|s| vec![s]);

    let opts = SearchOptions {
        query,
        tier,
        allowed_scopes,
        limit,
        vector_weight: retrieval.vector_weight,
        text_weight: retrieval.text_weight,
        aggregate: retrieval.aggregate,
        graph_expand: retrieval.graph_expand,
        max_hops: retrieval.max_hops,
        ..Default::default()
    };

    let results = nomen.search(opts).await.map_err(ApiError::from_anyhow)?;

    let result_values: Vec<Value> = results
        .iter()
        .map(|r| {
            json!({
                "topic": r.topic,
                "summary": r.summary,
                "detail": r.detail,
                "visibility": r.tier,
                "scope": "",
                "confidence": r.confidence,
                "match_type": format!("{:?}", r.match_type).to_lowercase(),
                "graph_edge": r.graph_edge,
                "contradicts": r.contradicts,
                "created_at": r.created_at.to_human_datetime(),
            })
        })
        .collect();

    Ok(json!({
        "count": result_values.len(),
        "results": result_values,
    }))
}

pub async fn put(
    nomen: &Nomen,
    default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let topic = params
        .get("topic")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let summary = params
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if topic.is_empty() || summary.is_empty() {
        return Err(ApiError::invalid_params("topic and summary are required"));
    }

    let detail = params
        .get("detail")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let confidence = params
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.8);

    let (vis, scope) = resolve_visibility_scope(params, nomen, default_channel)?;
    let visibility = vis.unwrap_or(Visibility::Public);
    let scope_str = scope.unwrap_or_default();
    let tier = visibility.to_tier(&scope_str);

    let mem = crate::NewMemory {
        topic: topic.clone(),
        summary,
        detail,
        tier,
        confidence,
        source: Some("api/v2".to_string()),
        model: Some("api/v2".to_string()),
    };

    let d_tag = nomen.store(mem).await.map_err(ApiError::from_anyhow)?;

    // Check what version we ended up with
    let version = if let Ok(Some(record)) = nomen.get_by_raw_topic(&topic).await {
        record.version
    } else {
        1
    };

    Ok(json!({
        "d_tag": d_tag,
        "topic": topic,
        "version": version,
    }))
}

pub async fn get(
    nomen: &Nomen,
    default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let d_tag = params.get("d_tag").and_then(|v| v.as_str());
    let topic = params.get("topic").and_then(|v| v.as_str());

    if d_tag.is_none() && topic.is_none() {
        return Err(ApiError::invalid_params("topic or d_tag is required"));
    }

    // Direct d_tag lookup
    if let Some(d_tag) = d_tag {
        let record = nomen.get_by_topic(d_tag).await.map_err(ApiError::from_anyhow)?;
        return Ok(record_to_value(record));
    }

    // Topic lookup — try to build d_tag from visibility+scope+topic
    let topic = topic.unwrap();
    let (vis, scope) = resolve_visibility_scope(params, nomen, default_channel)?;

    if let (Some(vis), _) = (&vis, &scope) {
        let scope_str = scope.as_deref().unwrap_or("");
        let author_pubkey = nomen
            .signer()
            .map(|s| s.public_key().to_hex())
            .unwrap_or_default();
        let context = match vis {
            Visibility::Personal | Visibility::Internal => author_pubkey.clone(),
            Visibility::Group => scope_str.to_string(),
            Visibility::Circle => scope_str.to_string(),
            Visibility::Public => String::new(),
        };
        let d_tag = crate::memory::build_v2_dtag(vis.as_str(), &context, topic);
        let record = nomen.get_by_topic(&d_tag).await.map_err(ApiError::from_anyhow)?;
        if record.is_some() {
            return Ok(record_to_value(record));
        }
    }

    // Fallback: raw topic lookup
    let record = nomen.get_by_raw_topic(topic).await.map_err(ApiError::from_anyhow)?;
    Ok(record_to_value(record))
}

fn record_to_value(record: Option<crate::db::MemoryRecord>) -> Value {
    match record {
        Some(m) => json!({
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
        None => Value::Null,
    }
}

pub async fn list(
    nomen: &Nomen,
    default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
    let include_stats = params.get("stats").and_then(|v| v.as_bool()).unwrap_or(false);

    let (vis, _scope) = resolve_visibility_scope(params, nomen, default_channel)?;
    let tier = vis.as_ref().map(|v| v.as_str().to_string());

    let report = nomen
        .list(crate::ListOptions {
            tier,
            limit,
            include_stats,
        })
        .await
        .map_err(ApiError::from_anyhow)?;

    let memories: Vec<Value> = report
        .memories
        .iter()
        .map(|m| {
            json!({
                "topic": m.topic,
                "summary": m.summary,
                "visibility": m.tier,
                "scope": m.scope,
                "confidence": m.confidence,
                "version": m.version,
                "created_at": m.created_at,
                "d_tag": m.d_tag,
            })
        })
        .collect();

    let mut result = json!({
        "count": memories.len(),
        "memories": memories,
    });

    if let Some(ref stats) = report.stats {
        result["stats"] = json!({
            "total": stats.total,
            "named": stats.named,
            "pending": stats.pending,
        });
    }

    Ok(result)
}

pub async fn delete(
    nomen: &Nomen,
    default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let topic = params.get("topic").and_then(|v| v.as_str());
    let d_tag = params.get("d_tag").and_then(|v| v.as_str());
    let id = params.get("id").and_then(|v| v.as_str());

    if topic.is_none() && d_tag.is_none() && id.is_none() {
        return Err(ApiError::invalid_params(
            "Provide topic, d_tag, or id",
        ));
    }

    // If topic is provided with visibility/scope, build d_tag
    let effective_topic = if let Some(topic) = topic {
        let (vis, scope) = resolve_visibility_scope(params, nomen, default_channel)?;
        if let Some(vis) = vis {
            let scope_str = scope.as_deref().unwrap_or("");
            let author_pubkey = nomen
                .signer()
                .map(|s| s.public_key().to_hex())
                .unwrap_or_default();
            let context = match vis {
                Visibility::Personal | Visibility::Internal => author_pubkey,
                Visibility::Group => scope_str.to_string(),
                _ => String::new(),
            };
            Some(crate::memory::build_v2_dtag(vis.as_str(), &context, topic))
        } else {
            Some(topic.to_string())
        }
    } else {
        d_tag.map(String::from)
    };

    nomen
        .delete(effective_topic.as_deref(), id)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "deleted": true,
        "d_tag": effective_topic,
        "relay_deleted": nomen.relay().is_some(),
    }))
}
