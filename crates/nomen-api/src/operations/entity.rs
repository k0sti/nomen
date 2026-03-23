//! Entity domain operations: list, relationships.

use serde_json::{json, Value};

use nomen_core::api::errors::ApiError;
use nomen_core::entities::EntityKind;
use crate::NomenBackend;

pub async fn list(
    nomen: &dyn NomenBackend,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let kind_filter = params.get("kind").and_then(|v| v.as_str());

    if let Some(k) = kind_filter {
        if EntityKind::from_str(k).is_none() {
            return Err(ApiError::invalid_params(
                "Unknown entity kind. Valid: person, project, concept, place, organization, technology",
            ));
        }
    }

    let entity_list = nomen
        .entities(kind_filter)
        .await
        .map_err(ApiError::from_anyhow)?;

    // Optional name query filter
    let query = params.get("query").and_then(|v| v.as_str());
    let filtered: Vec<_> = if let Some(q) = query {
        let q_lower = q.to_lowercase();
        entity_list
            .iter()
            .filter(|e| e.name.to_lowercase().contains(&q_lower))
            .collect()
    } else {
        entity_list.iter().collect()
    };

    let entities: Vec<Value> = filtered
        .iter()
        .map(|e| {
            json!({
                "name": e.name,
                "kind": e.kind,
                "created_at": e.created_at,
            })
        })
        .collect();

    Ok(json!({
        "count": entities.len(),
        "entities": entities,
    }))
}

pub async fn relationships(
    nomen: &dyn NomenBackend,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let entity_name = params.get("name").and_then(|v| v.as_str());

    let rels = nomen
        .entity_relationships(entity_name)
        .await
        .map_err(ApiError::from_anyhow)?;

    let rel_values: Vec<Value> = rels
        .iter()
        .map(|r| {
            json!({
                "from": r.from_name,
                "relation": r.relation,
                "to": r.to_name,
                "detail": r.detail,
            })
        })
        .collect();

    Ok(json!({
        "count": rel_values.len(),
        "relationships": rel_values,
    }))
}
