//! Maintenance domain operations: consolidate, cluster, sync, embed, prune.

use serde_json::{json, Value};

use crate::api::errors::ApiError;
use crate::Nomen;

pub async fn consolidate(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let batch_size = params
        .get("batch_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
    let min_messages = params
        .get("min_messages")
        .and_then(|v| v.as_u64())
        .unwrap_or(3) as usize;
    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let older_than = params
        .get("older_than")
        .and_then(|v| v.as_str())
        .map(String::from);
    let tier = params
        .get("tier")
        .and_then(|v| v.as_str())
        .map(String::from);

    let opts = crate::ConsolidateOptions {
        batch_size,
        min_messages,
        dry_run,
        older_than,
        tier,
        ..Default::default()
    };

    let report = nomen
        .consolidate(opts)
        .await
        .map_err(ApiError::from_anyhow)?;

    let groups: Vec<Value> = report.groups.iter().map(|g| json!({
        "key": g.key,
        "message_count": g.message_count,
        "topic": g.topic,
        "scope": g.scope,
        "time_start": g.time_start,
        "time_end": g.time_end,
    })).collect();

    Ok(json!({
        "messages_processed": report.messages_processed,
        "memories_created": report.memories_created,
        "events_published": report.events_published,
        "channels": report.channels,
        "groups": groups,
        "dry_run": report.dry_run,
    }))
}

pub async fn consolidate_prepare(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let batch_size = params
        .get("batch_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
    let min_messages = params
        .get("min_messages")
        .and_then(|v| v.as_u64())
        .unwrap_or(3) as usize;
    let ttl_minutes = params
        .get("ttl_minutes")
        .and_then(|v| v.as_u64())
        .unwrap_or(60) as u32;

    let opts = crate::ConsolidateOptions {
        batch_size,
        min_messages,
        ..Default::default()
    };

    let result = nomen
        .consolidate_prepare(opts, ttl_minutes)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(serde_json::to_value(&result).map_err(|e| ApiError::internal(&e.to_string()))?)
}

pub async fn consolidate_commit(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::invalid_params("session_id is required"))?;

    let extractions: Vec<crate::consolidate::BatchExtraction> = params
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

pub async fn cluster(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
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

    let opts = crate::ClusterOptions {
        min_members,
        namespace_depth,
        dry_run,
        prefix_filter: prefix,
        ..Default::default()
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

pub async fn sync(
    nomen: &Nomen,
    _default_channel: &str,
    _params: &Value,
) -> Result<Value, ApiError> {
    let report = nomen.sync().await.map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "stored": report.stored,
        "skipped": report.skipped,
        "errors": report.errors,
    }))
}

pub async fn embed(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;

    let report = nomen.embed(limit).await.map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "embedded": report.embedded,
        "total": report.total,
    }))
}

pub async fn publish(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let memories = params
        .get("memories")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let messages = params
        .get("messages")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(100) as usize;

    // If neither flag is set, publish both
    let (do_memories, do_messages) = if !memories && !messages {
        (true, true)
    } else {
        (memories, messages)
    };

    let opts = crate::PublishOptions {
        memories: do_memories,
        messages: do_messages,
        dry_run,
        limit,
    };

    let report = nomen
        .publish(opts)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "published": report.published,
        "failed": report.failed,
        "skipped": report.skipped,
    }))
}

pub async fn prune(
    nomen: &Nomen,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
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
        "raw_messages_pruned": report.raw_messages_pruned,
        "dry_run": report.dry_run,
    }))
}
