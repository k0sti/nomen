//! Action name -> handler routing for the canonical API v2.

use serde_json::Value;

use crate::operations;
use crate::NomenBackend;
use nomen_core::api::errors::ApiError;
use nomen_core::api::types::ApiResponse;

/// Dispatch a canonical API v2 action.
///
/// This is the single entry point for both CVM and MCP transports.
pub async fn dispatch(nomen: &dyn NomenBackend, action: &str, params: &Value) -> ApiResponse {
    let result = dispatch_inner(nomen, action, params).await;
    match result {
        Ok(value) => ApiResponse::success(value),
        Err(err) => ApiResponse::error(err),
    }
}

async fn dispatch_inner(
    nomen: &dyn NomenBackend,
    action: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    match action {
        // Memory domain
        "memory.search" => operations::memory::search(nomen, params).await,
        "memory.put" => operations::memory::put(nomen, params).await,
        "memory.get" => operations::memory::get(nomen, params).await,
        "memory.list" => operations::memory::list(nomen, params).await,
        "memory.delete" => operations::memory::delete(nomen, params).await,

        // Message domain
        "message.ingest" => operations::message::ingest(nomen, params).await,
        "message.context" => operations::message::context(nomen, params).await,
        "message.search" => operations::message::search(nomen, params).await,
        "message.send" => operations::message::send_message(nomen, params).await,
        "message.store" => operations::message::store(nomen, params).await,
        "message.query" => operations::message::query(nomen, params).await,
        "message.store_media" => operations::message::store_media(nomen, params).await,
        "message.import" => operations::message::import(nomen, params).await,
        "message.fetch_media" => operations::message::fetch_media(nomen, params).await,

        // Entity domain
        "entity.list" => operations::entity::list(nomen, params).await,
        "entity.relationships" => operations::entity::relationships(nomen, params).await,

        // Maintenance domain
        "memory.consolidate" => operations::maintenance::consolidate(nomen, params).await,
        "memory.consolidate_prepare" => {
            operations::maintenance::consolidate_prepare(nomen, params).await
        }
        "memory.consolidate_commit" => {
            operations::maintenance::consolidate_commit(nomen, params).await
        }
        "memory.cluster" => operations::maintenance::cluster(nomen, params).await,
        "memory.sync" => operations::maintenance::sync(nomen, params).await,
        "memory.embed" => operations::maintenance::embed(nomen, params).await,
        "memory.prune" => operations::maintenance::prune(nomen, params).await,

        // Identity domain
        "identity.auth" => operations::identity::auth(nomen, params).await,

        // Group domain
        "group.list" => operations::group::list(nomen, params).await,
        "group.members" => operations::group::members(nomen, params).await,
        "group.create" => operations::group::create(nomen, params).await,
        "group.add_member" => operations::group::add_member(nomen, params).await,
        "group.remove_member" => operations::group::remove_member(nomen, params).await,

        _ => Err(ApiError::unknown_action(action)),
    }
}

/// Map an MCP underscore-format tool name to a v2 action name.
///
/// E.g., `memory_search` -> `memory.search`.
pub fn mcp_tool_to_action(tool_name: &str) -> Option<String> {
    // v2 tools use underscore in MCP: memory_search -> memory.search
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
            | "message.context"
            | "message.search"
            | "message.send"
            | "message.store"
            | "message.query"
            | "message.store_media"
            | "message.import"
            | "message.fetch_media"
            | "memory.consolidate"
            | "memory.consolidate_prepare"
            | "memory.consolidate_commit"
            | "memory.cluster"
            | "memory.sync"
            | "memory.embed"
            | "memory.prune"
            | "identity.auth"
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
