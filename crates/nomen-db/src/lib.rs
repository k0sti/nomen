//! nomen-db — SurrealDB storage layer for the Nomen memory system.
//!
//! Contains all database operations: CRUD, search, graph, embeddings, schema.
//! Depends on nomen-core for pure types (ParsedMemory, CollectedEvent, etc.)
//! but does NOT depend on nostr-sdk directly.

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;
use tracing::debug;

pub mod embed;
pub mod entity;
pub mod graph;
pub mod groups;
pub mod memory;
pub mod message;
pub mod meta;
mod schema;
pub mod search;
pub mod search_engine;

// ── Re-exports ──────────────────────────────────────────────────────
// All public items re-exported so callers can use `db::function_name()`.

pub use schema::SCHEMA;

pub use memory::{
    count_memories_by_type, delete_collected_before, delete_memories_by_dtags,
    delete_memory_by_dtag, delete_memory_by_nostr_id, delete_memory_by_topic,
    find_prunable_memories, get_memory_by_dtag, get_memory_by_topic, list_memories, prune_memories,
    set_importance, store_memory, store_memory_direct,
    update_access_tracking, update_access_tracking_batch, PrunableMemory, PruneReport,
};

pub mod collected;

pub use message::{
    cleanup_expired_consolidation_sessions, create_consolidation_session,
    get_consolidation_session, update_consolidation_session_status, ConsolidationSessionRecord,
};

pub use collected::{
    count_collected_events, count_unconsolidated_collected, get_collected_event,
    get_unconsolidated_collected, mark_collected_consolidated, query_collected_events,
    search_collected_events, store_collected_event, CollectedMessageRecord, CollectedSearchResult,
};

pub use search::{
    get_memories_without_embeddings, hybrid_search, search_memories, HybridSearchRow,
    MissingEmbeddingRow, SearchDisplayResult, TextSearchResult,
};

pub use entity::{
    create_mention_edge, create_typed_edge, list_entities, list_entity_relationships, store_entity,
};

pub use embed::store_embedding;

pub use graph::{
    create_consolidated_edge, create_references_edge, get_graph_neighbors_simple, GraphNeighbor,
};

pub use meta::{get_meta, set_meta};

pub use search_engine::search;

pub use groups::{
    add_member, create_group, get_members, list_groups, remove_member, GroupStoreExt,
};

// ── Shared deserializer helpers ─────────────────────────────────────

/// Deserialize SurrealDB NONE/null as None for Option<String>.
pub fn deserialize_option_string<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de;
    struct OptionStringVisitor;
    impl<'de> de::Visitor<'de> for OptionStringVisitor {
        type Value = Option<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string, null, or NONE")
        }
        fn visit_none<E: de::Error>(self) -> std::result::Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> std::result::Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_some<D2: Deserializer<'de>>(
            self,
            d: D2,
        ) -> std::result::Result<Option<String>, D2::Error> {
            Ok(Some(String::deserialize(d)?))
        }
        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Option<String>, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Option<String>, E> {
            Ok(Some(v))
        }
        fn visit_enum<A: de::EnumAccess<'de>>(
            self,
            data: A,
        ) -> std::result::Result<Option<String>, A::Error> {
            // SurrealDB NONE comes as an enum variant
            let _ = data.variant::<String>()?;
            Ok(None)
        }
    }
    deserializer.deserialize_any(OptionStringVisitor)
}

/// Deserialize SurrealDB Thing (record ID) as a plain String.
pub fn deserialize_thing_as_string<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de;
    struct ThingOrString;
    impl<'de> de::Visitor<'de> for ThingOrString {
        type Value = String;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or SurrealDB Thing")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<String, E> {
            Ok(v)
        }
        fn visit_map<A: de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<String, A::Error> {
            let mut tb = String::new();
            let mut id = String::new();
            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "tb" => tb = map.next_value()?,
                    "id" => {
                        id = map
                            .next_value::<serde_json::Value>()?
                            .to_string()
                            .trim_matches('"')
                            .to_string();
                    }
                    _ => {
                        let _ = map.next_value::<serde_json::Value>()?;
                    }
                }
            }
            Ok(format!("{tb}:{id}"))
        }
        fn visit_enum<A: de::EnumAccess<'de>>(
            self,
            data: A,
        ) -> std::result::Result<String, A::Error> {
            // SurrealDB Thing can be serialized as an enum (internal representation)
            let (variant, _): (String, _) = data.variant()?;
            Ok(variant)
        }
    }
    deserializer.deserialize_any(ThingOrString)
}

// ── Shared types ────────────────────────────────────────────────────

/// SurrealDB memory record
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct MemoryRecord {
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub tier: String,
    pub scope: String,
    pub topic: String,
    /// Legacy field — kept as Option for migration reads, never written.
    #[serde(default)]
    pub source: String,
    pub model: Option<String>,
    pub version: i64,
    pub nostr_id: Option<String>,
    pub d_tag: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl nomen_core::access::AccessCheckable for MemoryRecord {
    fn tier(&self) -> &str {
        &self.tier
    }
    fn scope(&self) -> &str {
        &self.scope
    }
    fn source(&self) -> &str {
        &self.source
    }
}

/// A raw message as stored in SurrealDB (with DB-assigned id and consolidated flag).
/// Legacy raw-message compatibility record.
///
/// Canonical normalized messaging data now lives in collected-message records
/// using `platform/community/chat/thread/message`. This struct remains only as a
/// compatibility bridge for older consolidation and migration paths.

/// An entity record from SurrealDB.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct EntityRecord {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub kind: String,
    pub attributes: Option<serde_json::Value>,
    pub created_at: String,
}

/// A relationship record from SurrealDB.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct RelationshipRecord {
    pub from_name: String,
    pub to_name: String,
    pub relation: String,
    #[serde(default)]
    pub detail: String,
    pub created_at: String,
}

// ── Database initialization ─────────────────────────────────────────

pub fn db_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".nomen")
        .join("db")
}

/// Initialize (or open) the SurrealDB database and apply schema.
/// Uses default embedding dimensions (1536).
pub async fn init_db() -> Result<Surreal<Db>> {
    init_db_with_dimensions(1536).await
}

/// Initialize (or open) the SurrealDB database with configurable HNSW dimensions.
pub async fn init_db_with_dimensions(dimensions: usize) -> Result<Surreal<Db>> {
    let path = db_path();
    std::fs::create_dir_all(&path)
        .with_context(|| format!("Failed to create DB directory: {}", path.display()))?;

    debug!("Opening SurrealDB at {}", path.display());
    let db = Surreal::new::<SurrealKv>(path)
        .versioned()
        .await
        .context("Failed to open SurrealDB")?;

    db.use_ns("nomen").use_db("nomen").await?;

    db.query(schema::SCHEMA_BASE)
        .await
        .context("Failed to apply base schema")?;

    // Apply HNSW index with configurable dimensions
    let hnsw_sql = format!(
        "DEFINE INDEX IF NOT EXISTS memory_embedding ON memory FIELDS embedding HNSW DIMENSION {dimensions} DIST COSINE EFC 150 M 12"
    );
    db.query(&hnsw_sql)
        .await
        .context("Failed to apply HNSW index")?;
    debug!(dimensions, "Schema applied");

    Ok(db)
}
