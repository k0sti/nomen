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

    let opts = crate::ConsolidateOptions {
        batch_size,
        min_messages,
        ..Default::default()
    };

    let report = nomen
        .consolidate(opts)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "messages_processed": report.messages_processed,
        "memories_created": report.memories_created,
        "events_published": report.events_published,
        "channels": report.channels,
    }))
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
