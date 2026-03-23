use anyhow::Result;
use surrealdb::engine::local::Db;
use surrealdb::types::RecordId;
use surrealdb::Surreal;

use crate::entities::{EntityKind, EntityRecord};

/// Store an entity (upsert by name).
pub async fn store_entity(db: &Surreal<Db>, name: &str, kind: &EntityKind) -> Result<String> {
    let now = chrono::Utc::now().to_rfc3339();
    let kind_str = kind.as_str();

    // Try to find existing entity first
    let existing: Vec<EntityRecord> = db
        .query("SELECT * FROM entity WHERE name = $name LIMIT 1")
        .bind(("name", name.to_string()))
        .await?
        .check()?
        .take(0)?;

    if let Some(entity) = existing.first() {
        return Ok(entity.id.clone());
    }

    let result: Vec<EntityRecord> = db
        .query(
            "CREATE entity CONTENT { \
                name: $name, \
                kind: $kind, \
                attributes: NONE, \
                created_at: $created_at \
            }",
        )
        .bind(("name", name.to_string()))
        .bind(("kind", kind_str.to_string()))
        .bind(("created_at", now))
        .await?
        .check()?
        .take(0)?;

    let id = result.first().map(|r| r.id.clone()).unwrap_or_default();
    Ok(id)
}

/// List all entities, optionally filtered by kind.
pub async fn list_entities(
    db: &Surreal<Db>,
    kind: Option<&EntityKind>,
) -> Result<Vec<EntityRecord>> {
    let results: Vec<EntityRecord> = if let Some(kind) = kind {
        db.query("SELECT * FROM entity WHERE kind = $kind ORDER BY name ASC")
            .bind(("kind", kind.as_str().to_string()))
            .await?
            .check()?
            .take(0)?
    } else {
        db.query("SELECT * FROM entity ORDER BY name ASC")
            .await?
            .check()?
            .take(0)?
    };
    Ok(results)
}

/// Create a "mentions" edge from a memory to an entity.
pub async fn create_mention_edge(
    db: &Surreal<Db>,
    memory_id: &str,
    entity_id: &str,
    relevance: f64,
) -> Result<()> {
    db.query("RELATE $from->mentions->$to SET relevance = $relevance")
        .bind(("from", RecordId::new("memory", memory_id)))
        .bind(("to", RecordId::new("entity", entity_id)))
        .bind(("relevance", relevance))
        .await?
        .check()?;
    Ok(())
}

/// Create a typed relationship edge between two entities.
///
/// Creates a `related_to` edge from entity->entity with relation type and optional detail.
pub async fn create_typed_edge(
    db: &Surreal<Db>,
    from_entity_id: &str,
    to_entity_id: &str,
    relation: &str,
    detail: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    db.query(
        "RELATE $from->related_to->$to SET relation = $relation, detail = $detail, created_at = $now",
    )
    .bind(("from", RecordId::new("entity", from_entity_id)))
    .bind(("to", RecordId::new("entity", to_entity_id)))
    .bind(("relation", relation.to_string()))
    .bind(("detail", detail.unwrap_or("").to_string()))
    .bind(("now", now))
    .await?
    .check()?;
    Ok(())
}

/// List all entity relationships, optionally filtered by entity name.
pub async fn list_entity_relationships(
    db: &Surreal<Db>,
    entity_name: Option<&str>,
) -> Result<Vec<crate::entities::RelationshipRecord>> {
    let results: Vec<crate::entities::RelationshipRecord> = if let Some(name) = entity_name {
        db.query(
            "SELECT in.name AS from_name, out.name AS to_name, relation, detail, created_at \
             FROM related_to \
             WHERE in.name = $name OR out.name = $name \
             ORDER BY created_at DESC",
        )
        .bind(("name", name.to_string()))
        .await?
        .check()?
        .take(0)?
    } else {
        db.query(
            "SELECT in.name AS from_name, out.name AS to_name, relation, detail, created_at \
             FROM related_to ORDER BY created_at DESC",
        )
        .await?
        .check()?
        .take(0)?
    };
    Ok(results)
}
