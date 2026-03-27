//! Group domain operations: list, members, create, add_member, remove_member.

use serde_json::{json, Value};

use crate::NomenBackend;
use nomen_core::api::errors::ApiError;

pub async fn list(
    nomen: &dyn NomenBackend,
    _default_channel: &str,
    _params: &Value,
) -> Result<Value, ApiError> {
    let groups = nomen.group_list().await.map_err(ApiError::from_anyhow)?;

    let group_values: Vec<Value> = groups
        .iter()
        .map(|g| {
            json!({
                "id": g.id,
                "name": g.name,
                "member_count": g.members.len(),
                "nostr_group": g.nostr_group,
                "relay": g.relay,
            })
        })
        .collect();

    Ok(json!({
        "count": group_values.len(),
        "groups": group_values,
    }))
}

pub async fn members(
    nomen: &dyn NomenBackend,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");

    if id.is_empty() {
        return Err(ApiError::invalid_params("id is required"));
    }

    let members = nomen
        .group_members(id)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "id": id,
        "count": members.len(),
        "members": members,
    }))
}

pub async fn create(
    nomen: &dyn NomenBackend,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");

    if id.is_empty() || name.is_empty() {
        return Err(ApiError::invalid_params("id and name are required"));
    }

    let members_arr: Vec<String> = params
        .get("members")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let nostr_group = params.get("nostr_group").and_then(|v| v.as_str());
    let relay = params.get("relay").and_then(|v| v.as_str());

    nomen
        .group_create(id, name, &members_arr, nostr_group, relay)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "id": id,
        "name": name,
        "created": true,
    }))
}

pub async fn add_member(
    nomen: &dyn NomenBackend,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let npub = params.get("npub").and_then(|v| v.as_str()).unwrap_or("");

    if id.is_empty() || npub.is_empty() {
        return Err(ApiError::invalid_params("id and npub are required"));
    }

    nomen
        .group_add_member(id, npub)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "id": id,
        "npub": npub,
        "added": true,
    }))
}

pub async fn remove_member(
    nomen: &dyn NomenBackend,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let npub = params.get("npub").and_then(|v| v.as_str()).unwrap_or("");

    if id.is_empty() || npub.is_empty() {
        return Err(ApiError::invalid_params("id and npub are required"));
    }

    nomen
        .group_remove_member(id, npub)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "id": id,
        "npub": npub,
        "removed": true,
    }))
}
