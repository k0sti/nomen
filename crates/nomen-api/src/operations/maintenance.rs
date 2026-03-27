//! Maintenance domain operations: consolidate, cluster, sync, embed, prune.

use serde_json::{json, Value};

use crate::NomenBackend;
use nomen_core::api::errors::ApiError;
use nomen_core::ops::{ClusterParams, ConsolidateParams};
use nomen_llm::consolidate::BatchExtraction;

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

fn parse_timestamp(params: &Value, key: &str) -> Result<Option<i64>, ApiError> {
    let Some(value) = params.get(key) else {
        return Ok(None);
    };
    if let Some(ts) = value.as_i64() {
        return Ok(Some(ts));
    }
    if let Some(s) = value.as_str() {
        return chrono::DateTime::parse_from_rfc3339(s)
            .map(|dt| Some(dt.timestamp()))
            .map_err(|_| {
                ApiError::invalid_params(&format!(
                    "{key} must be a unix timestamp or RFC3339 string"
                ))
            });
    }
    Err(ApiError::invalid_params(&format!(
        "{key} must be a unix timestamp or RFC3339 string"
    )))
}

fn consolidate_params_from_value(params: &Value) -> Result<ConsolidateParams, ApiError> {
    let batch_size = params
        .get("batch_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
    let min_messages = params
        .get("min_messages")
        .and_then(|v| v.as_u64())
        .unwrap_or(3) as usize;

    let platform = extract_string_array(params, "#proxy").or_else(|| {
        params
            .get("source")
            .and_then(|v| v.as_str())
            .map(|s| vec![s.to_string()])
    });
    let community_id = extract_string_array(params, "#community");
    let chat_id = extract_string_array(params, "#chat");
    let thread_id = extract_string_array(params, "#thread");
    let since = parse_timestamp(params, "since")?;
    let older_than = params
        .get("older_than")
        .and_then(|v| v.as_str())
        .map(String::from);

    Ok(ConsolidateParams {
        batch_size,
        min_messages,
        platform,
        community_id,
        chat_id,
        thread_id,
        since,
        older_than,
    })
}

pub async fn consolidate(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let opts = consolidate_params_from_value(params)?;

    let report = nomen
        .consolidate(opts)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "messages_processed": report.messages_processed,
        "memories_created": report.memories_created,
        "events_published": report.events_published,
        "containers": report.channels,
    }))
}

pub async fn consolidate_prepare(
    nomen: &dyn NomenBackend,
    params: &Value,
) -> Result<Value, ApiError> {
    let ttl_minutes = params
        .get("ttl_minutes")
        .and_then(|v| v.as_u64())
        .unwrap_or(60) as u32;

    let opts = consolidate_params_from_value(params)?;

    let result = nomen
        .consolidate_prepare(opts, ttl_minutes)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(serde_json::to_value(&result).map_err(|e| ApiError::internal(&e.to_string()))?)
}

pub async fn consolidate_commit(
    nomen: &dyn NomenBackend,
    params: &Value,
) -> Result<Value, ApiError> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::invalid_params("session_id is required"))?;

    let extractions: Vec<BatchExtraction> = params
        .get("extractions")
        .ok_or_else(|| ApiError::invalid_params("extractions is required"))?
        .as_array()
        .ok_or_else(|| ApiError::invalid_params("extractions must be an array"))?
        .iter()
        .map(|v| serde_json::from_value(v.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::invalid_params(&format!("invalid extraction format: {e}")))?;

    let result = nomen
        .consolidate_commit(session_id, &extractions)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(serde_json::to_value(&result).map_err(|e| ApiError::internal(&e.to_string()))?)
}

pub async fn cluster(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let prefix = params
        .get("prefix")
        .and_then(|v| v.as_str())
        .map(String::from);
    let min_members = params
        .get("min_members")
        .and_then(|v| v.as_u64())
        .unwrap_or(3) as usize;
    let namespace_depth = params
        .get("namespace_depth")
        .and_then(|v| v.as_u64())
        .unwrap_or(2) as usize;
    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let opts = ClusterParams {
        min_members,
        namespace_depth,
        dry_run,
        prefix_filter: prefix,
    };

    let report = nomen
        .cluster_fusion(opts)
        .await
        .map_err(ApiError::from_anyhow)?;

    let cluster_details: Vec<Value> = report
        .cluster_details
        .iter()
        .map(|c| {
            json!({
                "prefix": c.prefix,
                "member_count": c.member_count,
                "member_topics": c.member_topics,
            })
        })
        .collect();

    Ok(json!({
        "memories_scanned": report.memories_scanned,
        "clusters_found": report.clusters_found,
        "clusters_synthesized": report.clusters_synthesized,
        "edges_created": report.edges_created,
        "dry_run": report.dry_run,
        "clusters": cluster_details,
    }))
}

pub async fn sync(nomen: &dyn NomenBackend, _params: &Value) -> Result<Value, ApiError> {
    let report = nomen.sync().await.map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "stored": report.stored,
        "skipped": report.skipped,
        "errors": report.errors,
    }))
}

pub async fn embed(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;

    let report = nomen.embed(limit).await.map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "embedded": report.embedded,
        "total": report.total,
    }))
}

pub async fn prune(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let days = params.get("days").and_then(|v| v.as_u64()).unwrap_or(90);
    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let report = nomen
        .prune(days, dry_run)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "memories_pruned": report.memories_pruned,
        "dry_run": report.dry_run,
    }))
}
