//! Action name → handler routing for the canonical API v2.

use serde_json::{json, Value};

use super::errors::ApiError;
use super::operations;
use super::types::ApiResponse;
use crate::auth::CallerContext;
use crate::Nomen;

/// Dispatch a canonical API v2 action.
///
/// This is the single entry point for both CVM and MCP transports.
/// The `caller` parameter gates access: write actions require owner,
/// read actions filter by the caller's visibility permissions.
pub async fn dispatch(
    nomen: &Nomen,
    default_channel: &str,
    action: &str,
    params: &Value,
    caller: &CallerContext,
) -> ApiResponse {
    let result = dispatch_inner(nomen, default_channel, action, params, caller).await;
    match result {
        Ok(value) => ApiResponse::success(value),
        Err(err) => ApiResponse::error(err),
    }
}

async fn dispatch_inner(
    nomen: &Nomen,
    default_channel: &str,
    action: &str,
    params: &Value,
    caller: &CallerContext,
) -> Result<Value, ApiError> {
    // Check action-level permissions
    crate::auth::check_action_permission(action, caller)
        .map_err(ApiError::unauthorized)?;

    let mut result = match action {
        // Memory domain
        "memory.search" => operations::memory::search(nomen, default_channel, params).await,
        "memory.put" => operations::memory::put(nomen, default_channel, params).await,
        "memory.get" => operations::memory::get(nomen, default_channel, params).await,
        "memory.get_batch" => operations::memory::get_batch(nomen, default_channel, params).await,
        "memory.list" => operations::memory::list(nomen, default_channel, params).await,
        "memory.delete" => operations::memory::delete(nomen, default_channel, params).await,
        "memory.pin" => operations::memory::pin(nomen, default_channel, params).await,
        "memory.unpin" => operations::memory::unpin(nomen, default_channel, params).await,

        // Message domain
        "message.ingest" => operations::message::ingest(nomen, default_channel, params).await,
        "message.list" => operations::message::list(nomen, default_channel, params).await,
        "message.context" => operations::message::context(nomen, default_channel, params).await,
        "message.search" => operations::message::search(nomen, default_channel, params).await,
        "message.send" => operations::message::send_message(nomen, default_channel, params).await,

        // Entity domain
        "entity.list" => operations::entity::list(nomen, default_channel, params).await,
        "entity.relationships" => {
            operations::entity::relationships(nomen, default_channel, params).await
        }

        // Maintenance domain
        "memory.consolidate" => {
            operations::maintenance::consolidate(nomen, default_channel, params).await
        }
        "memory.consolidate_prepare" => {
            operations::maintenance::consolidate_prepare(nomen, default_channel, params).await
        }
        "memory.consolidate_commit" => {
            operations::maintenance::consolidate_commit(nomen, default_channel, params).await
        }
        "memory.cluster" => operations::maintenance::cluster(nomen, default_channel, params).await,
        "memory.sync" => operations::maintenance::sync(nomen, default_channel, params).await,
        "memory.embed" => operations::maintenance::embed(nomen, default_channel, params).await,
        "memory.publish" => operations::maintenance::publish(nomen, default_channel, params).await,
        "memory.migrate_dtags" => operations::maintenance::migrate_dtags(nomen, default_channel, params).await,
        "memory.prune" => operations::maintenance::prune(nomen, default_channel, params).await,

        // Group domain
        "group.list" => operations::group::list(nomen, default_channel, params).await,
        "group.members" => operations::group::members(nomen, default_channel, params).await,
        "group.create" => operations::group::create(nomen, default_channel, params).await,
        "group.add_member" => operations::group::add_member(nomen, default_channel, params).await,
        "group.remove_member" => {
            operations::group::remove_member(nomen, default_channel, params).await
        }

        _ => Err(ApiError::unknown_action(action)),
    }?;

    // Enforce visibility-tier access control on read results for non-owner callers.
    if !caller.is_owner() {
        filter_by_visibility(action, &mut result, caller);
    }

    Ok(result)
}

/// Post-filter read results to enforce visibility-tier access control.
/// Owners see everything; members see public+group; anonymous sees public only.
fn filter_by_visibility(action: &str, result: &mut Value, caller: &CallerContext) {
    let allowed = caller.allowed_visibilities();

    match action {
        "memory.list" => {
            if let Some(memories) = result.get_mut("memories").and_then(|v| v.as_array_mut()) {
                memories.retain(|m| {
                    m.get("visibility")
                        .and_then(|v| v.as_str())
                        .map(|vis| allowed.contains(&vis))
                        .unwrap_or(false)
                });
                result["count"] = json!(memories.len());
            }
            // Filter stats.by_tier to only show allowed tiers
            if let Some(stats) = result.get_mut("stats") {
                if let Some(by_tier) = stats.get_mut("by_tier").and_then(|v| v.as_array_mut()) {
                    by_tier.retain(|entry| {
                        entry
                            .get("tier")
                            .and_then(|v| v.as_str())
                            .map(|tier| allowed.contains(&tier))
                            .unwrap_or(false)
                    });
                }
            }
        }
        "memory.search" => {
            if let Some(results) = result.get_mut("results").and_then(|v| v.as_array_mut()) {
                results.retain(|r| {
                    r.get("visibility")
                        .and_then(|v| v.as_str())
                        .map(|vis| allowed.contains(&vis))
                        .unwrap_or(false)
                });
                result["count"] = json!(results.len());
            }
        }
        "memory.get" => {
            // Single result: if visibility not allowed, return null
            if let Some(vis) = result.get("visibility").and_then(|v| v.as_str()) {
                if !allowed.contains(&vis) {
                    *result = Value::Null;
                }
            }
        }
        "memory.get_batch" => {
            if let Some(results) = result.get_mut("results").and_then(|v| v.as_array_mut()) {
                results.retain(|r| {
                    r.get("visibility")
                        .and_then(|v| v.as_str())
                        .map(|vis| allowed.contains(&vis))
                        .unwrap_or(false)
                });
                result["count"] = json!(results.len());
            }
            if let Some(by_dtag) = result.get_mut("by_d_tag").and_then(|v| v.as_object_mut()) {
                by_dtag.retain(|_, v| {
                    v.get("visibility")
                        .and_then(|v| v.as_str())
                        .map(|vis| allowed.contains(&vis))
                        .unwrap_or(false)
                });
            }
        }
        _ => {}
    }
}

/// Map an MCP underscore-format tool name to a v2 action name.
///
/// E.g., `memory_search` → `memory.search`.
pub fn mcp_tool_to_action(tool_name: &str) -> Option<String> {
    // v2 tools use underscore in MCP: memory_search → memory.search
    let parts: Vec<&str> = tool_name.splitn(2, '_').collect();
    if parts.len() == 2 {
        let candidate = format!("{}.{}", parts[0], parts[1]);
        // Validate it's a known action
        match candidate.as_str() {
            "memory.search"
            | "memory.put"
            | "memory.get"
            | "memory.list"
            | "memory.delete"
            | "message.ingest"
            | "message.list"
            | "message.context"
            | "message.search"
            | "message.send"
            | "memory.pin"
            | "memory.unpin"
            | "memory.get_batch"
            | "memory.consolidate"
            | "memory.consolidate_prepare"
            | "memory.consolidate_commit"
            | "memory.cluster"
            | "memory.sync"
            | "memory.embed"
            | "memory.publish"
            | "memory.migrate_dtags"
            | "memory.prune"
            | "entity.list"
            | "entity.relationships"
            | "group.list"
            | "group.members"
            | "group.create"
            | "group.add_member"
            | "group.remove_member" => Some(candidate),
            _ => None,
        }
    } else {
        None
    }
}
