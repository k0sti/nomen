//! Entity domain operations: list, relationships.
//!
//! Entities are memories with `type=entity:*`. Relationships are `references` edges.

use serde_json::{json, Value};

use crate::NomenBackend;
use nomen_core::api::errors::ApiError;
use nomen_core::entities::EntityKind;

pub async fn list(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let kind_filter = params.get("kind").and_then(|v| v.as_str());

    if let Some(k) = kind_filter {
        if EntityKind::from_str(k).is_none() {
            return Err(ApiError::invalid_params(
                "Unknown entity kind. Valid: person, project, concept, place, organization, technology",
            ));
        }
    }

    let type_filter = kind_filter.map(|k| format!("entity:{k}"));
    let memories = nomen
        .entity_memories(type_filter.as_deref())
        .await
        .map_err(ApiError::from_anyhow)?;

    // Optional name query filter
    let query = params.get("query").and_then(|v| v.as_str());
    let filtered: Vec<_> = if let Some(q) = query {
        let q_lower = q.to_lowercase();
        memories
            .iter()
            .filter(|m| m.topic.to_lowercase().contains(&q_lower) || m.content.to_lowercase().contains(&q_lower))
            .collect()
    } else {
        memories.iter().collect()
    };

    let entities: Vec<Value> = filtered
        .iter()
        .map(|m| {
            let kind = m.memory_type.as_deref()
                .unwrap_or("entity:concept")
                .strip_prefix("entity:")
                .unwrap_or("concept");
            json!({
                "name": m.content,
                "topic": m.topic,
                "kind": kind,
                "d_tag": m.d_tag,
                "created_at": m.created_at,
            })
        })
        .collect();

    Ok(json!({
        "count": entities.len(),
        "entities": entities,
    }))
}

pub async fn relationships(nomen: &dyn NomenBackend, params: &Value) -> Result<Value, ApiError> {
    let d_tag = params.get("d_tag").and_then(|v| v.as_str());

    let rels = nomen
        .entity_relationships(d_tag)
        .await
        .map_err(ApiError::from_anyhow)?;

    Ok(json!({
        "count": rels.len(),
        "relationships": rels,
    }))
}
