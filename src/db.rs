use anyhow::{Context, Result};
use nostr_sdk::prelude::*;
use serde::{Deserialize, Deserializer, Serialize};
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::types::{RecordId, SurrealValue};
use surrealdb::Surreal;
use tracing::debug;

use crate::memory::ParsedMemory;

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

/// SurrealDB memory record
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct MemoryRecord {
    pub search_text: String,
    /// The actual detail text, without topic/summary prepended.
    #[serde(default)]
    pub detail: Option<String>,
    pub embedding: Option<Vec<f32>>,
    pub visibility: String,
    pub scope: String,
    pub topic: String,
    pub source: String,
    pub model: Option<String>,
    pub version: i64,
    pub nostr_id: Option<String>,
    pub d_tag: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub ephemeral: bool,
    pub consolidated_from: Option<String>,
    pub consolidated_at: Option<String>,
    pub last_accessed: Option<String>,
    #[serde(default)]
    pub access_count: i64,
    pub importance: Option<i64>,
    /// Whether this memory is pinned (always injected into agent sessions).
    #[serde(default)]
    pub pinned: bool,
    /// Whether this memory has an embedding vector. Computed from queries, not stored directly.
    #[serde(default)]
    pub embedded: bool,
}

/// Version check result
#[derive(Debug, Deserialize, SurrealValue)]
struct VersionCheck {
    version: i64,
}

/// Search result from SurrealDB (text-only search)
#[derive(Debug, Deserialize, SurrealValue)]
pub struct TextSearchResult {
    pub search_text: String,
    pub visibility: String,
    pub topic: String,
    pub created_at: String,
    #[allow(dead_code)]
    pub score: Option<f64>,
}

/// Hybrid search result from SurrealDB
#[derive(Debug, Deserialize, SurrealValue)]
pub struct HybridSearchRow {
    pub search_text: String,
    #[serde(default)]
    pub detail: Option<String>,
    pub visibility: String,
    pub scope: String,
    pub topic: String,
    pub importance: Option<i32>,
    pub source: String,
    pub model: Option<String>,
    pub version: Option<i64>,
    pub d_tag: Option<String>,
    pub created_at: String,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub last_accessed: Option<String>,
    pub vec_score: Option<f64>,
    pub text_score: Option<f64>,
    pub combined: Option<f64>,
    pub embedding: Option<Vec<f32>>,
}

/// Row for memories missing embeddings
#[derive(Debug, Deserialize, SurrealValue)]
pub struct MissingEmbeddingRow {
    pub d_tag: Option<String>,
    pub search_text: String,
}

/// Formatted search result for display
pub struct SearchDisplayResult {
    pub visibility: String,
    pub topic: String,
    pub created_at: Timestamp,
}

fn db_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".nomen")
        .join("db")
}

/// Base schema (without HNSW index — that's applied dynamically based on config).
///
/// Also exported as `SCHEMA` for integration tests.
pub const SCHEMA: &str = SCHEMA_BASE;
const SCHEMA_BASE: &str = r#"
DEFINE TABLE IF NOT EXISTS memory SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS search_text ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS detail     ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS embedding  ON memory TYPE option<array<float>>;
DEFINE FIELD IF NOT EXISTS visibility ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS scope      ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS topic      ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS source     ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS model      ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS version    ON memory TYPE int DEFAULT 1;
DEFINE FIELD IF NOT EXISTS nostr_id   ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS d_tag      ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS ephemeral  ON memory TYPE bool DEFAULT false;
DEFINE FIELD IF NOT EXISTS pinned    ON memory TYPE bool DEFAULT false;
DEFINE FIELD IF NOT EXISTS embedded  ON memory TYPE bool DEFAULT false;
DEFINE FIELD IF NOT EXISTS consolidated_from ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS consolidated_at   ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS last_accessed     ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS access_count      ON memory TYPE int DEFAULT 0;
DEFINE FIELD IF NOT EXISTS importance        ON memory TYPE option<int>;
DEFINE FIELD IF NOT EXISTS source_time_start ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS source_time_end   ON memory TYPE option<string>;
-- Note: created_at/updated_at remain TYPE string (not datetime) because SurrealDB
-- datetime serialization requires special handling in Rust serde. RFC3339 strings
-- still support lexicographic ordering which is sufficient for our queries.

DEFINE ANALYZER IF NOT EXISTS memory_analyzer TOKENIZERS class FILTERS ascii, lowercase, snowball(english);
DEFINE INDEX IF NOT EXISTS memory_fulltext ON memory FIELDS search_text FULLTEXT ANALYZER memory_analyzer BM25;
DEFINE INDEX IF NOT EXISTS memory_d_tag  ON memory FIELDS d_tag UNIQUE;
DEFINE INDEX IF NOT EXISTS memory_visibility ON memory FIELDS visibility;
DEFINE INDEX IF NOT EXISTS memory_scope  ON memory FIELDS scope;
DEFINE INDEX IF NOT EXISTS memory_topic  ON memory FIELDS topic;

DEFINE TABLE IF NOT EXISTS nomen_group SCHEMALESS;
DEFINE FIELD IF NOT EXISTS name       ON nomen_group TYPE string;
DEFINE FIELD IF NOT EXISTS parent     ON nomen_group TYPE option<string>;
DEFINE FIELD IF NOT EXISTS members    ON nomen_group TYPE option<array>;
DEFINE FIELD IF NOT EXISTS relay      ON nomen_group TYPE option<string>;
DEFINE FIELD IF NOT EXISTS nostr_group ON nomen_group TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON nomen_group TYPE string;
DEFINE INDEX IF NOT EXISTS group_id   ON nomen_group FIELDS id UNIQUE;

DEFINE TABLE IF NOT EXISTS raw_message SCHEMALESS;
DEFINE FIELD IF NOT EXISTS source       ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS source_id    ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS sender       ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS channel      ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS content      ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS metadata     ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at   ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS consolidated ON raw_message TYPE bool DEFAULT false;
DEFINE INDEX IF NOT EXISTS raw_msg_time    ON raw_message FIELDS created_at;
DEFINE INDEX IF NOT EXISTS raw_msg_channel ON raw_message FIELDS channel;

DEFINE TABLE IF NOT EXISTS entity SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS name       ON entity TYPE string;
DEFINE FIELD IF NOT EXISTS kind       ON entity TYPE string;
DEFINE FIELD IF NOT EXISTS attributes ON entity TYPE option<object>;
DEFINE FIELD IF NOT EXISTS created_at ON entity TYPE string;
DEFINE INDEX IF NOT EXISTS entity_name ON entity FIELDS name UNIQUE;

DEFINE TABLE IF NOT EXISTS mentions SCHEMALESS;
DEFINE TABLE IF NOT EXISTS consolidated_from SCHEMALESS;
DEFINE TABLE IF NOT EXISTS references SCHEMALESS;
DEFINE TABLE IF NOT EXISTS related_to SCHEMALESS;

DEFINE FIELD IF NOT EXISTS nostr_event_id   ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS provider_id      ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS sender_id        ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS room             ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS topic            ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS thread           ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS scope            ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS source_created_at ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS publish_status   ON raw_message TYPE option<string>;

DEFINE INDEX IF NOT EXISTS raw_msg_nostr_id    ON raw_message FIELDS nostr_event_id;
DEFINE INDEX IF NOT EXISTS raw_msg_provider_id ON raw_message FIELDS provider_id, channel;
DEFINE INDEX IF NOT EXISTS raw_msg_source ON raw_message FIELDS source, source_id;
DEFINE INDEX IF NOT EXISTS raw_msg_sender ON raw_message FIELDS sender;
DEFINE INDEX IF NOT EXISTS raw_msg_room    ON raw_message FIELDS room;
DEFINE INDEX IF NOT EXISTS raw_msg_topic   ON raw_message FIELDS topic;
DEFINE INDEX IF NOT EXISTS raw_msg_thread  ON raw_message FIELDS thread;
DEFINE INDEX IF NOT EXISTS raw_msg_source_created_at ON raw_message FIELDS source_created_at;
DEFINE ANALYZER IF NOT EXISTS raw_message_analyzer
  TOKENIZERS class FILTERS ascii, lowercase, snowball(english);
DEFINE INDEX IF NOT EXISTS raw_msg_fulltext
  ON raw_message FIELDS content FULLTEXT ANALYZER raw_message_analyzer BM25;

DEFINE TABLE IF NOT EXISTS meta SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS key        ON meta TYPE string;
DEFINE FIELD IF NOT EXISTS value      ON meta TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON meta TYPE string;
DEFINE INDEX IF NOT EXISTS meta_key   ON meta FIELDS key UNIQUE;

DEFINE TABLE IF NOT EXISTS session SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS session_id    ON session TYPE string;
DEFINE FIELD IF NOT EXISTS tier          ON session TYPE string;
DEFINE FIELD IF NOT EXISTS scope         ON session TYPE string;
DEFINE FIELD IF NOT EXISTS channel       ON session TYPE string;
DEFINE FIELD IF NOT EXISTS group_id      ON session TYPE string;
DEFINE FIELD IF NOT EXISTS participants  ON session TYPE array;
DEFINE FIELD IF NOT EXISTS participants.* ON session TYPE string;
DEFINE FIELD IF NOT EXISTS created_at    ON session TYPE string;
DEFINE FIELD IF NOT EXISTS last_active   ON session TYPE string;
DEFINE INDEX IF NOT EXISTS session_sid   ON session FIELDS session_id UNIQUE;

-- Consolidation sessions (two-phase agent mode)
DEFINE TABLE IF NOT EXISTS consolidation_session SCHEMALESS;
DEFINE INDEX IF NOT EXISTS cons_session_sid ON consolidation_session FIELDS session_id UNIQUE;

"#;

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

    db.query(SCHEMA_BASE)
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

fn build_record(parsed: &ParsedMemory, nostr_id: &str) -> MemoryRecord {
    let now = chrono::Utc::now().to_rfc3339();
    let created = {
        let secs = parsed.created_at.as_u64() as i64;
        chrono::DateTime::from_timestamp(secs, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| now.clone())
    };

    let scope = extract_scope(&parsed.d_tag);
    let version: i64 = parsed.version.parse().unwrap_or(1);

    // Store plain text in search_text for FTS indexing (not JSON).
    // Include topic so topic words are searchable too.
    let searchable_content = format!("{} {}", parsed.topic, parsed.detail);

    MemoryRecord {
        search_text: searchable_content,
        detail: Some(parsed.detail.clone()),
        embedding: None,
        visibility: crate::memory::base_tier(&parsed.visibility).to_string(),
        scope,
        topic: parsed.topic.clone(),
        source: parsed.source.clone(),
        model: Some(parsed.model.clone()),
        version,
        nostr_id: Some(nostr_id.to_string()),
        d_tag: Some(parsed.d_tag.clone()),
        created_at: created,
        updated_at: now,
        ephemeral: false,
        consolidated_from: None,
        consolidated_at: None,
        last_accessed: None,
        access_count: 0,
        importance: None,
        pinned: false,
        embedded: false,
    }
}

/// Store a parsed memory into SurrealDB. Returns Ok(true) if inserted/updated, Ok(false) if skipped.
pub async fn store_memory(db: &Surreal<Db>, parsed: &ParsedMemory, event: &Event) -> Result<bool> {
    let record = build_record(parsed, &event.id.to_hex());

    // Check existing version
    let d_tag_owned = record.d_tag.clone().unwrap_or_default();
    let result = db
        .query("SELECT version FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", d_tag_owned.clone()))
        .await?;

    let existing: Option<VersionCheck> = result.check()?.take(0)?;

    if let Some(existing) = existing {
        if existing.version >= record.version {
            return Ok(false);
        }
    }

    // Upsert: delete then create
    db.query(
        "DELETE FROM memory WHERE d_tag = $d_tag; \
         CREATE memory CONTENT $record",
    )
    .bind(("d_tag", d_tag_owned))
    .bind(("record", record))
    .await?
    .check()?;

    Ok(true)
}

/// Store a memory directly (not from a relay event). Returns the d_tag.
pub async fn store_memory_direct(
    db: &Surreal<Db>,
    parsed: &ParsedMemory,
    event_id: &str,
) -> Result<String> {
    let record = build_record(parsed, event_id);
    let d_tag_owned = record.d_tag.clone().unwrap_or_default();

    db.query("DELETE FROM memory WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag_owned.clone()))
        .await?
        .check()?;

    db.query(
        "CREATE memory SET \
         search_text = $search_text, detail = $detail, visibility = $visibility, scope = $scope, \
         topic = $topic, source = $source, model = $model, \
         version = $version, nostr_id = $nostr_id, d_tag = $d_tag, \
         created_at = $created_at, updated_at = $updated_at, ephemeral = $ephemeral"
    )
    .bind(("search_text", record.search_text))
    .bind(("detail", record.detail))
    .bind(("visibility", record.visibility))
    .bind(("scope", record.scope))
    .bind(("topic", record.topic))
    .bind(("source", record.source))
    .bind(("model", record.model))
    .bind(("version", record.version))
    .bind(("nostr_id", record.nostr_id))
    .bind(("d_tag", record.d_tag))
    .bind(("created_at", record.created_at))
    .bind(("updated_at", record.updated_at))
    .bind(("ephemeral", record.ephemeral))
    .await?
    .check()?;

    Ok(d_tag_owned)
}

/// Full-text search for memories.
pub async fn search_memories(
    db: &Surreal<Db>,
    query: &str,
    tier: Option<&str>,
    allowed_scopes: Option<&[String]>,
    limit: usize,
) -> Result<Vec<SearchDisplayResult>> {
    let query_owned = query.to_string();

    let mut conditions = vec!["search_text @1@ $query".to_string()];
    if tier.is_some() {
        conditions.push("visibility = $tier".to_string());
    }
    if allowed_scopes.is_some() {
        conditions.push("(scope = \"\" OR array::any($scopes, |$s| scope = $s OR string::starts_with(scope, string::concat($s, \".\"))))".to_string());
    }
    let where_clause = conditions.join(" AND ");

    let sql = format!(
        "SELECT *, search::score(1) AS score FROM memory \
         WHERE {where_clause} \
         ORDER BY score DESC LIMIT {limit}"
    );

    let mut q = db.query(&sql).bind(("query", query_owned));
    if let Some(tier_val) = tier {
        q = q.bind(("tier", tier_val.to_string()));
    }
    if let Some(scopes) = allowed_scopes {
        q = q.bind(("scopes", scopes.to_vec()));
    }

    let results: Vec<TextSearchResult> = q.await?.check()?.take(0)?;

    let display_results = results
        .into_iter()
        .map(|r| {
            let ts = chrono::DateTime::parse_from_rfc3339(&r.created_at)
                .map(|dt| Timestamp::from(dt.timestamp() as u64))
                .unwrap_or(Timestamp::from(0));

            SearchDisplayResult {
                visibility: r.visibility,
                topic: r.topic,
                created_at: ts,
            }
        })
        .collect();

    Ok(display_results)
}

/// Update an existing memory's embedding by d-tag.
pub async fn store_embedding(db: &Surreal<Db>, d_tag: &str, embedding: Vec<f32>) -> Result<()> {
    db.query("UPDATE memory SET embedding = $embedding, updated_at = $now WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .bind(("embedding", embedding))
        .bind(("now", chrono::Utc::now().to_rfc3339()))
        .await?
        .check()?;
    Ok(())
}

/// List all memories (without embeddings, for display).
pub async fn list_memories(
    db: &Surreal<Db>,
    tier: Option<&str>,
    limit: usize,
    pinned: Option<bool>,
) -> Result<Vec<MemoryRecord>> {
    // Exclude embedding from SELECT to avoid SurrealDB HNSW index deserialization issues
    let fields = "search_text, detail, visibility, scope, topic, source, model, version, nostr_id, d_tag, created_at, updated_at, ephemeral, consolidated_from, consolidated_at, last_accessed, access_count, importance, pinned, embedding IS NOT NONE AS embedded";
    let mut conditions = Vec::new();
    if tier.is_some() {
        conditions.push("visibility = $tier".to_string());
    }
    if let Some(p) = pinned {
        conditions.push(format!("pinned = {p}"));
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql = format!("SELECT {fields} FROM memory {where_clause} ORDER BY created_at DESC LIMIT {limit}");

    let mut q = db.query(&sql);
    if let Some(t) = tier {
        q = q.bind(("tier", t.to_string()));
    }

    let mut results: Vec<MemoryRecord> = q.await?.check()?.take(0)?;
    // Strip embeddings from response to keep payload small
    for r in &mut results {
        r.embedding = None;
    }
    Ok(results)
}

/// Get memories that are missing embeddings.
pub async fn get_memories_without_embeddings(
    db: &Surreal<Db>,
    limit: usize,
) -> Result<Vec<MissingEmbeddingRow>> {
    let sql =
        format!("SELECT d_tag, search_text FROM memory WHERE embedding IS NONE LIMIT {limit}");
    let results: Vec<MissingEmbeddingRow> = db.query(&sql).await?.check()?.take(0)?;
    Ok(results)
}

/// Hybrid search combining vector similarity + BM25 full-text.
pub async fn hybrid_search(
    db: &Surreal<Db>,
    query_text: &str,
    query_embedding: &[f32],
    tier: Option<&str>,
    allowed_scopes: Option<&[String]>,
    _min_confidence: Option<f64>,
    vector_weight: f32,
    text_weight: f32,
    limit: usize,
) -> Result<Vec<HybridSearchRow>> {
    let mut conditions = vec!["search_text @1@ $query".to_string()];

    if tier.is_some() {
        conditions.push("visibility = $tier".to_string());
    }
    if allowed_scopes.is_some() {
        conditions.push("(scope = \"\" OR array::any($scopes, |$s| scope = $s OR string::starts_with(scope, string::concat($s, \".\"))))".to_string());
    }

    let where_clause = conditions.join(" AND ");

    let sql = format!(
        "SELECT *, \
           IF embedding != NONE THEN vector::similarity::cosine(embedding, $vec) ELSE 0 END AS vec_score, \
           search::score(1) AS text_score, \
           (IF embedding != NONE THEN vector::similarity::cosine(embedding, $vec) ELSE 0 END * $vw + search::score(1) * $tw) AS combined \
         FROM memory \
         WHERE {where_clause} \
         ORDER BY combined DESC \
         LIMIT {limit}"
    );

    let mut q = db
        .query(&sql)
        .bind(("query", query_text.to_string()))
        .bind(("vec", query_embedding.to_vec()))
        .bind(("vw", vector_weight))
        .bind(("tw", text_weight));

    if let Some(tier_val) = tier {
        q = q.bind(("tier", tier_val.to_string()));
    }
    if let Some(scopes) = allowed_scopes {
        q = q.bind(("scopes", scopes.to_vec()));
    }

    let results: Vec<HybridSearchRow> = q.await?.check()?.take(0)?;
    Ok(results)
}

/// Get a single memory by d-tag (topic).
pub async fn get_memory_by_dtag(db: &Surreal<Db>, d_tag: &str) -> Result<Option<MemoryRecord>> {
    let mut results: Vec<MemoryRecord> = db
        .query("SELECT search_text, detail, visibility, scope, topic, source, model, version, nostr_id, d_tag, created_at, updated_at, ephemeral, consolidated_from, consolidated_at, last_accessed, access_count, importance, pinned, embedding IS NOT NONE AS embedded FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(results.pop())
}

/// Get multiple memories by d-tags in a single query.
pub async fn get_memories_by_dtags(db: &Surreal<Db>, d_tags: &[String]) -> Result<Vec<MemoryRecord>> {
    if d_tags.is_empty() {
        return Ok(vec![]);
    }
    let results: Vec<MemoryRecord> = db
        .query("SELECT search_text, detail, visibility, scope, topic, source, model, version, nostr_id, d_tag, created_at, updated_at, ephemeral, consolidated_from, consolidated_at, last_accessed, access_count, importance, pinned, embedding IS NOT NONE AS embedded FROM memory WHERE d_tag IN $d_tags")
        .bind(("d_tags", d_tags.to_vec()))
        .await?
        .check()?
        .take(0)?;
    Ok(results)
}

/// Get a single memory by topic field (raw topic, not d-tag).
pub async fn get_memory_by_topic(db: &Surreal<Db>, topic: &str) -> Result<Option<MemoryRecord>> {
    let mut results: Vec<MemoryRecord> = db
        .query("SELECT search_text, detail, visibility, scope, topic, source, model, version, nostr_id, d_tag, created_at, updated_at, ephemeral, consolidated_from, consolidated_at, last_accessed, access_count, importance, pinned, embedding IS NOT NONE AS embedded FROM memory WHERE topic = $topic LIMIT 1")
        .bind(("topic", topic.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(results.pop())
}

/// Delete memory by topic field (raw topic, not d-tag).
pub async fn delete_memory_by_topic(db: &Surreal<Db>, topic: &str) -> Result<()> {
    db.query("DELETE FROM memory WHERE topic = $topic")
        .bind(("topic", topic.to_string()))
        .await?
        .check()?;
    Ok(())
}

/// Delete memory by d-tag.
pub async fn delete_memory_by_dtag(db: &Surreal<Db>, d_tag: &str) -> Result<()> {
    db.query("DELETE FROM memory WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?;
    Ok(())
}

/// Delete memory by nostr event ID.
pub async fn delete_memory_by_nostr_id(db: &Surreal<Db>, nostr_id: &str) -> Result<()> {
    db.query("DELETE FROM memory WHERE nostr_id = $nostr_id")
        .bind(("nostr_id", nostr_id.to_string()))
        .await?
        .check()?;
    Ok(())
}

/// Extract scope from d-tag.
///
/// Supports both v0.2 (`{visibility}:{context}:{topic}`) and legacy v0.1 formats.
/// Returns the context segment as scope (e.g., pubkey hex for personal, group id for group).
fn extract_scope(d_tag: &str) -> String {
    // v0.2 format: {visibility}:{context}:{topic}
    // Context/scope may itself contain colons (e.g. telegram:-1003821690204),
    // so parse as first colon + last colon, not splitn(3).
    if crate::memory::is_v2_dtag(d_tag) {
        let (_visibility, scope) = crate::memory::extract_visibility_scope(d_tag);
        return scope;
    }

    // v0.1 legacy formats
    if d_tag.starts_with("snowclaw:memory:npub:") {
        d_tag
            .strip_prefix("snowclaw:memory:npub:")
            .unwrap_or("")
            .to_string()
    } else if d_tag.starts_with("snowclaw:memory:group:") {
        d_tag
            .strip_prefix("snowclaw:memory:group:")
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    }
}

// ── Raw Message CRUD ────────────────────────────────────────────────

use crate::entities::{EntityKind, EntityRecord};
use crate::ingest::{MessageQuery, RawMessage, RawMessageRecord, RawMessageSearchResult};

/// Store a raw message into SurrealDB.
pub async fn store_raw_message(db: &Surreal<Db>, msg: &RawMessage) -> Result<String> {
    let now = chrono::Utc::now().to_rfc3339();
    let created = msg.created_at.as_deref().unwrap_or(&now);
    let source_id = msg.source_id.clone().unwrap_or_default();
    let channel = msg.channel.clone().unwrap_or_default();
    // Use serde-based record creation to avoid bind serialization issues
    #[derive(Serialize, SurrealValue)]
    struct NewRawMessage {
        source: String,
        source_id: String,
        sender: String,
        channel: String,
        content: String,
        metadata: String,
        created_at: String,
        consolidated: bool,
        nostr_event_id: String,
        provider_id: String,
        sender_id: String,
        room: String,
        topic: String,
        thread: String,
        scope: String,
        source_created_at: String,
        publish_status: String,
    }

    let metadata = msg.metadata.clone().unwrap_or_default();

    let record = NewRawMessage {
        source: msg.source.clone(),
        source_id,
        sender: msg.sender_name.clone().unwrap_or_else(|| msg.sender.clone()),
        channel,
        content: msg.content.clone(),
        metadata,
        created_at: created.to_string(),
        consolidated: false,
        nostr_event_id: String::new(),
        provider_id: msg.provider_id.clone().unwrap_or_default(),
        sender_id: msg.sender.clone(),
        room: msg.room.clone().unwrap_or_default(),
        topic: msg.topic.clone().unwrap_or_default(),
        thread: msg.thread.clone().unwrap_or_default(),
        scope: msg.scope.clone().unwrap_or_default(),
        source_created_at: msg.source_ts.clone().unwrap_or_default(),
        publish_status: String::new(),
    };

    #[derive(Deserialize, SurrealValue)]
    struct CreatedRow {
        #[serde(deserialize_with = "deserialize_thing_as_string")]
        id: String,
    }
    let rows: Vec<CreatedRow> = db
        .query("CREATE raw_message CONTENT $record RETURN meta::id(id) AS id")
        .bind(("record", record))
        .await?
        .check()?
        .take(0)?;

    let id = rows.into_iter().next().map(|r| r.id).unwrap_or_default();
    Ok(id)
}

/// Check for duplicate raw message by provider_id + channel.
pub async fn check_duplicate_raw_message(
    db: &Surreal<Db>,
    provider_id: &str,
    channel: &str,
) -> Result<bool> {
    #[derive(Deserialize, SurrealValue)]
    struct CountRow {
        count: usize,
    }
    let result: Option<CountRow> = db
        .query("SELECT count() AS count FROM raw_message WHERE provider_id = $pid AND channel = $ch GROUP ALL")
        .bind(("pid", provider_id.to_string()))
        .bind(("ch", channel.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(result.map(|r| r.count > 0).unwrap_or(false))
}

/// Check for duplicate raw message by nostr_event_id (for sync/import).
pub async fn check_duplicate_by_nostr_id(
    db: &Surreal<Db>,
    nostr_event_id: &str,
) -> Result<bool> {
    #[derive(Deserialize, SurrealValue)]
    struct CountRow {
        count: usize,
    }
    let result: Option<CountRow> = db
        .query("SELECT count() AS count FROM raw_message WHERE nostr_event_id = $nid GROUP ALL")
        .bind(("nid", nostr_event_id.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(result.map(|r| r.count > 0).unwrap_or(false))
}

/// All fields selected from raw_message for query results.
pub const RAW_MSG_SELECT_FIELDS: &str = "meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated, nostr_event_id ?? '' AS nostr_event_id, provider_id ?? '' AS provider_id, sender_id ?? '' AS sender_id, room ?? '' AS room, topic ?? '' AS topic, thread ?? '' AS thread, scope ?? '' AS scope, source_created_at ?? '' AS source_created_at, publish_status ?? '' AS publish_status, metadata ?? '' AS metadata";

/// Store a raw message imported from relay (with nostr_event_id already known).
pub async fn store_raw_message_from_relay(
    db: &Surreal<Db>,
    msg: &RawMessage,
    nostr_event_id: &str,
) -> Result<String> {
    let now = chrono::Utc::now().to_rfc3339();
    let created = msg.created_at.as_deref().unwrap_or(&now);

    #[derive(Serialize, SurrealValue)]
    struct NewRawMessage {
        source: String,
        source_id: String,
        sender: String,
        channel: String,
        content: String,
        metadata: String,
        created_at: String,
        consolidated: bool,
        nostr_event_id: String,
        provider_id: String,
        sender_id: String,
        room: String,
        topic: String,
        thread: String,
        scope: String,
        source_created_at: String,
        publish_status: String,
    }

    let record = NewRawMessage {
        source: msg.source.clone(),
        source_id: msg.source_id.clone().unwrap_or_default(),
        sender: msg.sender_name.clone().unwrap_or_else(|| msg.sender.clone()),
        channel: msg.channel.clone().unwrap_or_default(),
        content: msg.content.clone(),
        metadata: msg.metadata.clone().unwrap_or_default(),
        created_at: created.to_string(),
        consolidated: false,
        nostr_event_id: nostr_event_id.to_string(),
        provider_id: msg.provider_id.clone().unwrap_or_default(),
        sender_id: msg.sender.clone(),
        room: msg.room.clone().unwrap_or_default(),
        topic: msg.topic.clone().unwrap_or_default(),
        thread: msg.thread.clone().unwrap_or_default(),
        scope: msg.scope.clone().unwrap_or_default(),
        source_created_at: msg.source_ts.clone().unwrap_or_default(),
        publish_status: "published".to_string(),
    };

    db.query("CREATE raw_message CONTENT $record")
        .bind(("record", record))
        .await?
        .check()?;

    Ok("ok".to_string())
}

/// Query raw messages with filters.
pub async fn query_raw_messages(
    db: &Surreal<Db>,
    opts: &MessageQuery,
) -> Result<Vec<RawMessageRecord>> {
    let mut conditions = Vec::new();
    if opts.source.is_some() {
        conditions.push("source = $source".to_string());
    }
    if opts.channel.is_some() {
        conditions.push("channel = $channel".to_string());
    }
    if opts.sender.is_some() {
        conditions.push("sender = $sender".to_string());
    }
    if opts.since.is_some() {
        conditions.push("created_at >= $since".to_string());
    }
    if opts.until.is_some() {
        conditions.push("created_at <= $until".to_string());
    }
    if opts.room.is_some() {
        conditions.push("room = $room".to_string());
    }
    if opts.topic.is_some() {
        conditions.push("topic = $topic".to_string());
    }
    if opts.thread.is_some() {
        conditions.push("thread = $thread".to_string());
    }
    if !opts.include_consolidated {
        conditions.push("consolidated = false".to_string());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let limit = opts.limit.unwrap_or(100);
    let order_dir = match opts.order.as_deref() {
        Some("asc") => "ASC",
        _ => "DESC",
    };
    let sql = format!(
        "SELECT {RAW_MSG_SELECT_FIELDS} FROM raw_message {where_clause} ORDER BY created_at {order_dir} LIMIT {limit}"
    );

    let mut q = db.query(&sql);
    if let Some(ref source) = opts.source {
        q = q.bind(("source", source.clone()));
    }
    if let Some(ref channel) = opts.channel {
        q = q.bind(("channel", channel.clone()));
    }
    if let Some(ref sender) = opts.sender {
        q = q.bind(("sender", sender.clone()));
    }
    if let Some(ref since) = opts.since {
        q = q.bind(("since", since.clone()));
    }
    if let Some(ref until) = opts.until {
        q = q.bind(("until", until.clone()));
    }
    if let Some(ref room) = opts.room {
        q = q.bind(("room", room.clone()));
    }
    if let Some(ref topic) = opts.topic {
        q = q.bind(("topic", topic.clone()));
    }
    if let Some(ref thread) = opts.thread {
        q = q.bind(("thread", thread.clone()));
    }

    let results: Vec<RawMessageRecord> = q.await?.check()?.take(0)?;
    Ok(results)
}

/// Look up a single raw message by various anchor types.
///
/// Tries in order: `id` (local DB id), `nostr_event_id`, `provider_id` + `channel`, `source_id`.
pub async fn get_message_by_anchor(
    db: &Surreal<Db>,
    id: Option<&str>,
    nostr_event_id: Option<&str>,
    provider_id: Option<&str>,
    channel: Option<&str>,
    source_id: Option<&str>,
) -> Result<Option<RawMessageRecord>> {
    // Try by local DB id
    if let Some(id_val) = id {
        let sql = format!(
            "SELECT {RAW_MSG_SELECT_FIELDS} FROM raw_message:{id_val} LIMIT 1"
        );
        let results: Vec<RawMessageRecord> = db.query(&sql).await?.check()?.take(0)?;
        if let Some(r) = results.into_iter().next() {
            return Ok(Some(r));
        }
    }

    // Try by nostr_event_id
    if let Some(neid) = nostr_event_id {
        let sql = format!("SELECT {RAW_MSG_SELECT_FIELDS} FROM raw_message WHERE nostr_event_id = $neid LIMIT 1");
        let results: Vec<RawMessageRecord> = db.query(&sql).bind(("neid", neid.to_string())).await?.check()?.take(0)?;
        if let Some(r) = results.into_iter().next() {
            return Ok(Some(r));
        }
    }

    // Try by provider_id + channel
    if let Some(pid) = provider_id {
        let ch = channel.unwrap_or("");
        let sql = format!("SELECT {RAW_MSG_SELECT_FIELDS} FROM raw_message WHERE provider_id = $pid AND channel = $ch LIMIT 1");
        let results: Vec<RawMessageRecord> = db.query(&sql)
            .bind(("pid", pid.to_string()))
            .bind(("ch", ch.to_string()))
            .await?.check()?.take(0)?;
        if let Some(r) = results.into_iter().next() {
            return Ok(Some(r));
        }
    }

    // Try by legacy source_id
    if let Some(sid) = source_id {
        let sql = format!("SELECT {RAW_MSG_SELECT_FIELDS} FROM raw_message WHERE source_id = $sid LIMIT 1");
        let results: Vec<RawMessageRecord> = db.query(&sql).bind(("sid", sid.to_string())).await?.check()?.take(0)?;
        if let Some(r) = results.into_iter().next() {
            return Ok(Some(r));
        }
    }

    Ok(None)
}

/// Full-text BM25 search over raw messages.
pub async fn search_raw_messages(
    db: &Surreal<Db>,
    query: &str,
    source: Option<&str>,
    room: Option<&str>,
    topic: Option<&str>,
    sender: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
    include_consolidated: bool,
    limit: usize,
) -> Result<Vec<RawMessageSearchResult>> {
    let mut conditions = vec!["content @1@ $query".to_string()];
    if source.is_some() {
        conditions.push("source = $source".to_string());
    }
    if room.is_some() {
        conditions.push("room = $room".to_string());
    }
    if topic.is_some() {
        conditions.push("topic = $topic".to_string());
    }
    if sender.is_some() {
        conditions.push("sender = $sender".to_string());
    }
    if since.is_some() {
        conditions.push("created_at >= $since".to_string());
    }
    if until.is_some() {
        conditions.push("created_at <= $until".to_string());
    }
    if !include_consolidated {
        conditions.push("consolidated = false".to_string());
    }

    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT {RAW_MSG_SELECT_FIELDS}, search::score(1) AS score FROM raw_message WHERE {where_clause} ORDER BY score DESC, created_at DESC LIMIT {limit}"
    );

    let mut q = db.query(&sql).bind(("query", query.to_string()));
    if let Some(s) = source { q = q.bind(("source", s.to_string())); }
    if let Some(r) = room { q = q.bind(("room", r.to_string())); }
    if let Some(t) = topic { q = q.bind(("topic", t.to_string())); }
    if let Some(s) = sender { q = q.bind(("sender", s.to_string())); }
    if let Some(s) = since { q = q.bind(("since", s.to_string())); }
    if let Some(u) = until { q = q.bind(("until", u.to_string())); }

    let results: Vec<RawMessageSearchResult> = q.await?.check()?.take(0)?;
    Ok(results)
}

/// Update a raw message's nostr_event_id and publish_status after relay publish.
pub async fn update_raw_message_publish(
    db: &Surreal<Db>,
    id: &str,
    nostr_event_id: &str,
    publish_status: &str,
) -> Result<()> {
    db.query("UPDATE $id SET nostr_event_id = $neid, publish_status = $status")
        .bind(("id", RecordId::new("raw_message", id)))
        .bind(("neid", nostr_event_id.to_string()))
        .bind(("status", publish_status.to_string()))
        .await?
        .check()?;
    Ok(())
}

/// Mark raw messages as consolidated by their IDs.
pub async fn mark_messages_consolidated(db: &Surreal<Db>, ids: &[String]) -> Result<()> {
    for id in ids {
        db.query("UPDATE $id SET consolidated = true")
            .bind((
                "id",
                RecordId::new("raw_message", id.as_str()),
            ))
            .await?
            .check()?;
    }
    Ok(())
}

/// Get unconsolidated messages grouped by channel.
pub async fn get_unconsolidated_messages(
    db: &Surreal<Db>,
    limit: usize,
) -> Result<Vec<RawMessageRecord>> {
    get_unconsolidated_messages_filtered(db, limit, None, None).await
}

/// Get unconsolidated messages with optional filters.
///
/// - `before`: Only messages created before this RFC3339 timestamp (for --older-than).
/// - `tier_filter`: Only messages matching this tier/channel pattern.
pub async fn get_unconsolidated_messages_filtered(
    db: &Surreal<Db>,
    limit: usize,
    before: Option<&str>,
    _tier_filter: Option<&str>,
) -> Result<Vec<RawMessageRecord>> {
    let mut conditions = vec!["consolidated = false".to_string()];
    if before.is_some() {
        conditions.push("created_at < $before".to_string());
    }

    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated, nostr_event_id ?? '' AS nostr_event_id, provider_id ?? '' AS provider_id, sender_id ?? '' AS sender_id, room ?? '' AS room, topic ?? '' AS topic, thread ?? '' AS thread, scope ?? '' AS scope, source_created_at ?? '' AS source_created_at, publish_status ?? '' AS publish_status, metadata ?? '' AS metadata FROM raw_message WHERE {where_clause} ORDER BY created_at ASC LIMIT {limit}"
    );

    let mut q = db.query(&sql);
    if let Some(before_val) = before {
        q = q.bind(("before", before_val.to_string()));
    }

    let results: Vec<RawMessageRecord> = q.await?.check()?.take(0)?;
    Ok(results)
}

/// Set consolidation provenance tags on a memory record.
pub async fn set_consolidation_tags(
    db: &Surreal<Db>,
    d_tag: &str,
    consolidated_from: &str,
    consolidated_at: &str,
) -> Result<()> {
    db.query(
        "UPDATE memory SET consolidated_from = $from, consolidated_at = $at WHERE d_tag = $d_tag",
    )
    .bind(("d_tag", d_tag.to_string()))
    .bind(("from", consolidated_from.to_string()))
    .bind(("at", consolidated_at.to_string()))
    .await?
    .check()?;
    Ok(())
}

/// Set source time range on a memory record.
pub async fn set_source_time_range(
    db: &Surreal<Db>,
    d_tag: &str,
    start: &str,
    end: &str,
) -> Result<()> {
    db.query("UPDATE memory SET source_time_start = $start, source_time_end = $end WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .bind(("start", start.to_string()))
        .bind(("end", end.to_string()))
        .await?
        .check()?;
    Ok(())
}

/// Set the importance score on a memory record.
pub async fn set_importance(db: &Surreal<Db>, d_tag: &str, importance: i32) -> Result<()> {
    db.query("UPDATE memory SET importance = $importance WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .bind(("importance", importance))
        .await?
        .check()?;
    Ok(())
}

/// Set the pinned flag on a memory record.
pub async fn set_pinned(db: &Surreal<Db>, d_tag: &str, pinned: bool) -> Result<()> {
    db.query("UPDATE memory SET pinned = $pinned WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .bind(("pinned", pinned))
        .await?
        .check()?;
    Ok(())
}

/// Create a "references" edge between two memories with a relation type.
pub async fn create_references_edge(
    db: &Surreal<Db>,
    from_d_tag: &str,
    to_d_tag: &str,
    relation: &str,
) -> Result<()> {
    // Resolve d_tags to record IDs
    #[derive(Deserialize, SurrealValue)]
    struct IdRow {
        id: RecordId,
    }
    let from_rows: Vec<IdRow> = db
        .query("SELECT id FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", from_d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;
    let to_rows: Vec<IdRow> = db
        .query("SELECT id FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", to_d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;

    let from_id = from_rows
        .first()
        .map(|r| &r.id)
        .ok_or_else(|| anyhow::anyhow!("Memory not found: {from_d_tag}"))?;
    let to_id = to_rows
        .first()
        .map(|r| &r.id)
        .ok_or_else(|| anyhow::anyhow!("Memory not found: {to_d_tag}"))?;

    db.query("RELATE $from->references->$to SET relation = $relation, created_at = $now")
        .bind(("from", from_id.clone()))
        .bind(("to", to_id.clone()))
        .bind(("relation", relation.to_string()))
        .bind(("now", chrono::Utc::now().to_rfc3339()))
        .await?
        .check()?;
    Ok(())
}

/// A memory discovered through graph edge traversal.
#[derive(Debug, Deserialize, SurrealValue)]
pub struct GraphNeighbor {
    /// Edge type: "mentions", "references", "consolidated_from", or "contradicts"
    pub edge_type: String,
    /// The relation field on references edges (e.g. "contradicts", "supersedes")
    pub relation: Option<String>,
    pub visibility: String,
    pub topic: String,
    pub search_text: String,
    #[serde(default)]
    pub detail: Option<String>,
    pub created_at: String,
    pub d_tag: Option<String>,
    pub importance: Option<i64>,
    pub last_accessed: Option<String>,
}

/// Traverse 1-hop outgoing and incoming graph edges
/// from a memory identified by d_tag. This is a simpler, more reliable query than the full
/// graph traversal.
pub async fn get_graph_neighbors_simple(
    db: &Surreal<Db>,
    d_tag: &str,
) -> Result<Vec<GraphNeighbor>> {
    let mut all: Vec<GraphNeighbor> = Vec::new();

    // Find the memory record ID first
    #[derive(Deserialize, SurrealValue)]
    struct IdRow {
        id: RecordId,
    }
    let rows: Vec<IdRow> = db
        .query("SELECT id FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;

    let thing = match rows.first() {
        Some(r) => r.id.clone(),
        None => return Ok(all),
    };

    // 1. Outgoing references: memory->references->memory
    #[derive(Debug, Deserialize, SurrealValue)]
    struct RefEdge {
        relation: Option<String>,
        out: RecordId,
    }
    let out_edges: Vec<RefEdge> = db
        .query("SELECT relation, out FROM references WHERE in = $mid")
        .bind(("mid", thing.clone()))
        .await?
        .check()?
        .take(0)?;

    for edge in &out_edges {
        let mems: Vec<GraphNeighbor> = db
            .query("SELECT $edge_type AS edge_type, $relation AS relation, visibility, topic, search_text, created_at, d_tag, importance, last_accessed FROM $target")
            .bind(("target", edge.out.clone()))
            .bind(("edge_type", "references".to_string()))
            .bind(("relation", edge.relation.clone().unwrap_or_default()))
            .await?
            .check()?
            .take(0)?;
        all.extend(mems);
    }

    // 2. Incoming references: memory<-references<-memory
    #[derive(Debug, Deserialize, SurrealValue)]
    struct RefEdgeIn {
        relation: Option<String>,
        #[serde(rename = "in")]
        #[surreal(rename = "in")]
        in_node: RecordId,
    }
    let in_edges: Vec<RefEdgeIn> = db
        .query("SELECT relation, in FROM references WHERE out = $mid")
        .bind(("mid", thing.clone()))
        .await?
        .check()?
        .take(0)?;

    for edge in &in_edges {
        let mems: Vec<GraphNeighbor> = db
            .query("SELECT $edge_type AS edge_type, $relation AS relation, visibility, topic, search_text, created_at, d_tag, importance, last_accessed FROM $target")
            .bind(("target", edge.in_node.clone()))
            .bind(("edge_type", "references".to_string()))
            .bind(("relation", edge.relation.clone().unwrap_or_default()))
            .await?
            .check()?
            .take(0)?;
        all.extend(mems);
    }

    // 3. Shared entity mentions: find entities this memory mentions, then find other memories mentioning those entities
    #[derive(Debug, Deserialize, SurrealValue)]
    struct MentionEdge {
        out: RecordId,
    }
    let mention_edges: Vec<MentionEdge> = db
        .query("SELECT out FROM mentions WHERE in = $mid")
        .bind(("mid", thing.clone()))
        .await?
        .check()?
        .take(0)?;

    for mention in &mention_edges {
        // Find other memories that also mention this entity
        #[derive(Debug, Deserialize, SurrealValue)]
        struct MentionBack {
            #[serde(rename = "in")]
            #[surreal(rename = "in")]
            in_node: RecordId,
        }
        let back_edges: Vec<MentionBack> = db
            .query("SELECT in FROM mentions WHERE out = $ent AND in != $mid")
            .bind(("ent", mention.out.clone()))
            .bind(("mid", thing.clone()))
            .await?
            .check()?
            .take(0)?;

        for back in &back_edges {
            let mems: Vec<GraphNeighbor> = db
                .query("SELECT $edge_type AS edge_type, NONE AS relation, visibility, topic, search_text, created_at, d_tag, importance, last_accessed FROM $target")
                .bind(("target", back.in_node.clone()))
                .bind(("edge_type", "mentions".to_string()))
                .await?
                .check()?
                .take(0)?;
            all.extend(mems);
        }
    }

    // 4. Consolidated_from siblings: memories that share the same raw message sources
    #[derive(Debug, Deserialize, SurrealValue)]
    struct ConsolidatedEdge {
        out: RecordId,
    }
    let consolidated_edges: Vec<ConsolidatedEdge> = db
        .query("SELECT out FROM consolidated_from WHERE in = $mid")
        .bind(("mid", thing.clone()))
        .await?
        .check()?
        .take(0)?;

    for consol in &consolidated_edges {
        #[derive(Debug, Deserialize, SurrealValue)]
        struct ConsolBack {
            #[serde(rename = "in")]
            #[surreal(rename = "in")]
            in_node: RecordId,
        }
        let back_edges: Vec<ConsolBack> = db
            .query("SELECT in FROM consolidated_from WHERE out = $raw AND in != $mid")
            .bind(("raw", consol.out.clone()))
            .bind(("mid", thing.clone()))
            .await?
            .check()?
            .take(0)?;

        for back in &back_edges {
            let mems: Vec<GraphNeighbor> = db
                .query("SELECT $edge_type AS edge_type, NONE AS relation, visibility, topic, search_text, created_at, d_tag, importance, last_accessed FROM $target")
                .bind(("target", back.in_node.clone()))
                .bind(("edge_type", "consolidated_from".to_string()))
                .await?
                .check()?
                .take(0)?;
            all.extend(mems);
        }
    }

    Ok(all)
}

/// Get unconsolidated messages older than a cutoff, optionally filtered to ephemeral only.
pub async fn get_ephemeral_messages_before(
    db: &Surreal<Db>,
    before: &str,
    limit: usize,
) -> Result<Vec<RawMessageRecord>> {
    let sql = format!(
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated, nostr_event_id ?? '' AS nostr_event_id, provider_id ?? '' AS provider_id, sender_id ?? '' AS sender_id, room ?? '' AS room, topic ?? '' AS topic, thread ?? '' AS thread, scope ?? '' AS scope, source_created_at ?? '' AS source_created_at, publish_status ?? '' AS publish_status, metadata ?? '' AS metadata FROM raw_message WHERE created_at < $before ORDER BY created_at ASC LIMIT {limit}"
    );
    let results: Vec<RawMessageRecord> = db
        .query(&sql)
        .bind(("before", before.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(results)
}

/// Delete ephemeral raw messages older than a cutoff.
pub async fn delete_ephemeral_before(db: &Surreal<Db>, before: &str) -> Result<usize> {
    #[derive(Deserialize, SurrealValue)]
    struct CountResult {
        count: usize,
    }
    let count_result: Option<CountResult> = db
        .query("SELECT count() AS count FROM raw_message WHERE created_at < $before GROUP ALL")
        .bind(("before", before.to_string()))
        .await?
        .check()?
        .take(0)?;
    let count = count_result.map(|r| r.count).unwrap_or(0);
    if count > 0 {
        db.query("DELETE FROM raw_message WHERE created_at < $before")
            .bind(("before", before.to_string()))
            .await?
            .check()?;
    }
    Ok(count)
}

/// Count memories by type (ephemeral vs named).
pub async fn count_memories_by_type(db: &Surreal<Db>) -> Result<(usize, usize, usize)> {
    #[derive(Deserialize, SurrealValue)]
    struct CountRow {
        count: usize,
    }

    // Named memories (have a topic that doesn't start with "consolidated/" or "conv:")
    let named: Option<CountRow> = db
        .query("SELECT count() AS count FROM memory WHERE topic != NONE AND NOT (string::starts_with(topic, 'conv:') OR string::starts_with(topic, 'consolidated/')) GROUP ALL")
        .await?
        .check()?
        .take(0)?;

    // Total
    let total: Option<CountRow> = db
        .query("SELECT count() AS count FROM memory GROUP ALL")
        .await?
        .check()?
        .take(0)?;

    // Unconsolidated raw messages
    let pending: Option<CountRow> = db
        .query("SELECT count() AS count FROM raw_message WHERE consolidated = false GROUP ALL")
        .await?
        .check()?
        .take(0)?;

    let total_count = total.map(|r| r.count).unwrap_or(0);
    let named_count = named.map(|r| r.count).unwrap_or(total_count);
    let pending_count = pending.map(|r| r.count).unwrap_or(0);

    Ok((total_count, named_count, pending_count))
}

/// Per-channel message statistics for detailed stats output.
pub struct ChannelStats {
    pub channel: String,
    pub unconsolidated: usize,
    pub consolidated: usize,
    pub oldest_unconsolidated: Option<String>,
    pub newest_unconsolidated: Option<String>,
}

/// Detailed memory/message statistics.
pub struct DetailedStats {
    pub memories_by_tier: Vec<(String, usize)>,
    pub channels: Vec<ChannelStats>,
    pub last_consolidation: Option<String>,
}

/// Get detailed stats: memories by tier, messages by channel, last consolidation time.
pub async fn get_detailed_stats(db: &Surreal<Db>) -> Result<DetailedStats> {
    // ── Memories by tier ────────────────────────────────────────
    #[derive(Deserialize, SurrealValue)]
    struct TierRow {
        visibility: String,
        count: usize,
    }

    let tier_rows: Vec<TierRow> = db
        .query("SELECT visibility, count() AS count FROM memory GROUP BY visibility ORDER BY count DESC")
        .await?
        .check()?
        .take(0)?;

    let memories_by_tier: Vec<(String, usize)> = tier_rows
        .into_iter()
        .map(|r| (r.visibility, r.count))
        .collect();

    // ── Messages by channel (consolidated vs not) ───────────────
    #[derive(Deserialize, SurrealValue)]
    struct ChannelRow {
        channel: Option<String>,
        consolidated: bool,
        count: usize,
    }

    let channel_rows: Vec<ChannelRow> = db
        .query("SELECT channel, consolidated, count() AS count FROM raw_message GROUP BY channel, consolidated ORDER BY channel")
        .await?
        .check()?
        .take(0)?;

    // ── Oldest/newest unconsolidated per channel ────────────────
    #[derive(Deserialize, SurrealValue)]
    struct ChannelTimeRow {
        channel: Option<String>,
        oldest: Option<String>,
        newest: Option<String>,
    }

    let time_rows: Vec<ChannelTimeRow> = db
        .query("SELECT channel, math::min(created_at) AS oldest, math::max(created_at) AS newest FROM raw_message WHERE consolidated = false GROUP BY channel")
        .await?
        .check()?
        .take(0)?;

    // Merge channel rows into ChannelStats
    let mut channel_map: std::collections::BTreeMap<String, ChannelStats> =
        std::collections::BTreeMap::new();

    for row in channel_rows {
        let ch = row.channel.unwrap_or_default();
        let entry = channel_map.entry(ch.clone()).or_insert(ChannelStats {
            channel: ch,
            unconsolidated: 0,
            consolidated: 0,
            oldest_unconsolidated: None,
            newest_unconsolidated: None,
        });
        if row.consolidated {
            entry.consolidated = row.count;
        } else {
            entry.unconsolidated = row.count;
        }
    }

    for row in time_rows {
        let ch = row.channel.unwrap_or_default();
        if let Some(entry) = channel_map.get_mut(&ch) {
            entry.oldest_unconsolidated = row.oldest;
            entry.newest_unconsolidated = row.newest;
        }
    }

    // Sort by unconsolidated count descending
    let mut channels: Vec<ChannelStats> = channel_map.into_values().collect();
    channels.sort_by(|a, b| b.unconsolidated.cmp(&a.unconsolidated));

    // ── Last consolidation time ─────────────────────────────────
    #[derive(Deserialize, SurrealValue)]
    struct TimeRow {
        latest: Option<String>,
    }

    let last_row: Option<TimeRow> = db
        .query("SELECT math::max(consolidated_at) AS latest FROM memory WHERE consolidated_at != NONE GROUP ALL")
        .await?
        .check()?
        .take(0)?;

    let last_consolidation = last_row.and_then(|r| r.latest);

    Ok(DetailedStats {
        memories_by_tier,
        channels,
        last_consolidation,
    })
}

/// Query messages around a specific source_id: N messages before and after.
pub async fn query_messages_around(
    db: &Surreal<Db>,
    source_id: &str,
    context_count: usize,
) -> Result<Vec<RawMessageRecord>> {
    // First, find the target message's created_at
    #[derive(Deserialize, SurrealValue)]
    struct TimeRow {
        created_at: String,
    }

    let target: Option<TimeRow> = db
        .query("SELECT created_at FROM raw_message WHERE source_id = $source_id LIMIT 1")
        .bind(("source_id", source_id.to_string()))
        .await?
        .check()?
        .take(0)?;

    let pivot_time = match target {
        Some(t) => t.created_at,
        None => anyhow::bail!("Message with source_id '{source_id}' not found"),
    };

    // Fetch N messages before (inclusive of target) + N messages after
    let fields = "meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated, nostr_event_id ?? '' AS nostr_event_id, provider_id ?? '' AS provider_id, sender_id ?? '' AS sender_id, room ?? '' AS room, topic ?? '' AS topic, thread ?? '' AS thread, scope ?? '' AS scope, source_created_at ?? '' AS source_created_at, publish_status ?? '' AS publish_status, metadata ?? '' AS metadata";
    let before_sql = format!(
        "SELECT {fields} \
         FROM raw_message WHERE created_at <= $pivot ORDER BY created_at DESC LIMIT {}",
        context_count + 1
    );
    let after_sql = format!(
        "SELECT {fields} \
         FROM raw_message WHERE created_at > $pivot ORDER BY created_at ASC LIMIT {context_count}"
    );

    let combined_sql = format!("{before_sql}; {after_sql}");
    let mut result = db
        .query(&combined_sql)
        .bind(("pivot", pivot_time))
        .await?
        .check()?;

    let mut before: Vec<RawMessageRecord> = result.take(0)?;
    let after: Vec<RawMessageRecord> = result.take(1)?;

    // Before is in DESC order, reverse it
    before.reverse();
    before.extend(after);
    Ok(before)
}

/// Count consolidated raw messages older than the given cutoff date (RFC3339).
pub async fn count_old_messages(db: &Surreal<Db>, before: &str) -> Result<usize> {
    #[derive(Deserialize, SurrealValue)]
    struct CountResult {
        count: usize,
    }
    let result: Option<CountResult> = db
        .query("SELECT count() AS count FROM raw_message WHERE consolidated = true AND created_at < $before GROUP ALL")
        .bind(("before", before.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(result.map(|r| r.count).unwrap_or(0))
}

/// Prune (delete) consolidated raw messages older than the given cutoff date (RFC3339).
pub async fn prune_old_messages(db: &Surreal<Db>, before: &str) -> Result<usize> {
    let count = count_old_messages(db, before).await?;
    if count > 0 {
        db.query("DELETE FROM raw_message WHERE consolidated = true AND created_at < $before")
            .bind(("before", before.to_string()))
            .await?
            .check()?;
    }
    Ok(count)
}

// ── Access tracking ─────────────────────────────────────────────────

/// Update access tracking for a memory identified by d_tag.
pub async fn update_access_tracking(db: &Surreal<Db>, d_tag: &str) -> Result<()> {
    db.query("UPDATE memory SET last_accessed = $now, access_count += 1 WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .bind(("now", chrono::Utc::now().to_rfc3339()))
        .await?
        .check()?;
    Ok(())
}

/// Batch update access tracking for multiple d_tags.
pub async fn update_access_tracking_batch(db: &Surreal<Db>, d_tags: &[String]) -> Result<()> {
    for d_tag in d_tags {
        if let Err(e) = update_access_tracking(db, d_tag).await {
            tracing::warn!(d_tag = %d_tag, "Failed to update access tracking: {e}");
        }
    }
    Ok(())
}

// ── Memory pruning ──────────────────────────────────────────────────

/// Record for pruning candidates.
#[derive(Debug, Deserialize, serde::Serialize, SurrealValue)]
pub struct PrunableMemory {
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub d_tag: Option<String>,
    pub topic: String,
    pub access_count: Option<i64>,
    pub created_at: String,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub last_accessed: Option<String>,
}

/// Find memories eligible for pruning based on age and access patterns.
///
/// Pruning rules:
/// - access_count = 0 AND age > max_days
pub async fn find_prunable_memories(
    db: &Surreal<Db>,
    max_days: u64,
) -> Result<Vec<PrunableMemory>> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(max_days as i64);
    let cutoff_str = cutoff.to_rfc3339();

    let sql = "SELECT d_tag, topic, access_count, created_at, last_accessed \
               FROM memory WHERE \
               (access_count = 0 AND created_at < $cutoff)";

    let results: Vec<PrunableMemory> = db
        .query(sql)
        .bind(("cutoff", cutoff_str))
        .await?
        .check()?
        .take(0)?;
    Ok(results)
}

/// Delete memories by their d_tags.
pub async fn delete_memories_by_dtags(db: &Surreal<Db>, d_tags: &[String]) -> Result<usize> {
    let mut deleted = 0;
    for d_tag in d_tags {
        db.query("DELETE FROM memory WHERE d_tag = $d_tag")
            .bind(("d_tag", d_tag.clone()))
            .await?
            .check()?;
        deleted += 1;
    }
    Ok(deleted)
}

/// Result of a prune operation.
#[derive(Debug, serde::Serialize)]
pub struct PruneReport {
    pub memories_pruned: usize,
    pub raw_messages_pruned: usize,
    pub dry_run: bool,
    pub pruned: Vec<PrunableMemory>,
}

/// Shared prune logic callable from CLI and HTTP.
pub async fn prune_memories(db: &Surreal<Db>, days: u64, dry_run: bool) -> Result<PruneReport> {
    let prunable = find_prunable_memories(db, days).await?;

    let memories_pruned = if !dry_run && !prunable.is_empty() {
        let d_tags: Vec<String> = prunable.iter().filter_map(|m| m.d_tag.clone()).collect();
        delete_memories_by_dtags(db, &d_tags).await?
    } else {
        prunable.len()
    };

    // Also prune old consolidated raw messages
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    let cutoff_str = cutoff.to_rfc3339();
    let raw_messages_pruned = if dry_run {
        count_old_messages(db, &cutoff_str).await?
    } else {
        prune_old_messages(db, &cutoff_str).await?
    };

    Ok(PruneReport {
        memories_pruned,
        raw_messages_pruned,
        dry_run,
        pruned: prunable,
    })
}

// ── Publish queries ─────────────────────────────────────────────────

/// Get raw messages with pending/failed publish status (not yet on relay).
pub async fn get_unpublished_messages(
    db: &Surreal<Db>,
    limit: usize,
) -> Result<Vec<RawMessageRecord>> {
    let fields = RAW_MSG_SELECT_FIELDS;
    let sql = format!(
        "SELECT {fields} FROM raw_message \
         WHERE publish_status = '' OR publish_status = 'pending' OR publish_status = 'failed' OR publish_status IS NONE \
         ORDER BY created_at ASC LIMIT {limit}"
    );
    let results: Vec<RawMessageRecord> = db.query(&sql).await?.check()?.take(0)?;
    Ok(results)
}

/// Get memories that have not been published to relay (nostr_id is not a valid 64-char hex event ID).
pub async fn get_unpublished_memories(
    db: &Surreal<Db>,
    limit: usize,
) -> Result<Vec<MemoryRecord>> {
    let fields = "search_text, detail, visibility, scope, topic, source, model, version, nostr_id, d_tag, created_at, updated_at, ephemeral, consolidated_from, consolidated_at, last_accessed, access_count, importance, pinned, embedding IS NOT NONE AS embedded";
    let sql = format!(
        "SELECT {fields} FROM memory \
         WHERE nostr_id IS NONE OR nostr_id = '' OR string::len(nostr_id) != 64 \
         ORDER BY created_at ASC LIMIT {limit}"
    );
    let mut results: Vec<MemoryRecord> = db.query(&sql).await?.check()?.take(0)?;
    for r in &mut results {
        r.embedding = None;
    }
    Ok(results)
}

/// Update a memory's nostr_id after successful relay publish.
pub async fn update_memory_nostr_id(
    db: &Surreal<Db>,
    d_tag: &str,
    nostr_id: &str,
) -> Result<()> {
    db.query("UPDATE memory SET nostr_id = $nid WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .bind(("nid", nostr_id.to_string()))
        .await?
        .check()?;
    Ok(())
}

// ── Entity CRUD ─────────────────────────────────────────────────────

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
/// Creates a `related_to` edge from entity→entity with relation type and optional detail.
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

/// Create a "consolidated_from" edge from a consolidated memory to a raw message.
pub async fn create_consolidated_edge(
    db: &Surreal<Db>,
    memory_id: &str,
    raw_message_id: &str,
) -> Result<()> {
    db.query("RELATE $from->consolidated_from->$to")
        .bind(("from", RecordId::new("memory", memory_id)))
        .bind(("to", RecordId::new("raw_message", raw_message_id)))
        .await?
        .check()?;
    Ok(())
}

// ── Meta key-value store ─────────────────────────────────────────────

/// Get a meta value by key.
pub async fn get_meta(db: &Surreal<Db>, key: &str) -> Result<Option<String>> {
    #[derive(Deserialize, SurrealValue)]
    struct MetaRow {
        val: String,
    }
    let result: Option<MetaRow> = db
        .query("SELECT val FROM kv_meta WHERE key = $key LIMIT 1")
        .bind(("key", key.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(result.map(|r| r.val))
}

/// Set a meta value (upsert by key).
pub async fn set_meta(db: &Surreal<Db>, key: &str, val: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    db.query("DELETE FROM kv_meta WHERE key = $key; CREATE kv_meta CONTENT { key: $key, val: $val, updated_at: $now }")
        .bind(("key", key.to_string()))
        .bind(("val", val.to_string()))
        .bind(("now", now))
        .await?
        .check()?;
    Ok(())
}

/// Count unconsolidated raw messages.
pub async fn count_unconsolidated_messages(db: &Surreal<Db>) -> Result<usize> {
    #[derive(Deserialize, SurrealValue)]
    struct CountRow {
        count: usize,
    }
    let result: Option<CountRow> = db
        .query("SELECT count() AS count FROM raw_message WHERE consolidated = false GROUP ALL")
        .await?
        .check()?
        .take(0)?;
    Ok(result.map(|r| r.count).unwrap_or(0))
}

// ── Session CRUD ─────────────────────────────────────────────────────

use crate::session::SessionRecord;

/// Create or update a session record.
pub async fn create_session(
    db: &Surreal<Db>,
    session: &crate::session::ResolvedSession,
) -> Result<String> {
    let now = chrono::Utc::now().to_rfc3339();

    #[derive(Serialize, SurrealValue)]
    struct NewSession {
        session_id: String,
        tier: String,
        scope: String,
        channel: String,
        group_id: String,
        participants: Vec<String>,
        created_at: String,
        last_active: String,
    }

    let record = NewSession {
        session_id: session.session_id.clone(),
        tier: session.tier.clone(),
        scope: session.scope.clone(),
        channel: session.channel.clone(),
        group_id: session.group_id.clone(),
        participants: session.participants.clone(),
        created_at: now.clone(),
        last_active: now,
    };

    // Upsert: delete existing then create
    db.query("DELETE FROM session WHERE session_id = $sid; CREATE session CONTENT $record")
        .bind(("sid", session.session_id.clone()))
        .bind(("record", record))
        .await?
        .check()?;

    Ok(session.session_id.clone())
}

/// Get a session by session_id.
pub async fn get_session(db: &Surreal<Db>, session_id: &str) -> Result<Option<SessionRecord>> {
    let result: Option<SessionRecord> = db
        .query("SELECT meta::id(id) AS id, session_id, tier, scope, channel, group_id, participants, created_at, last_active FROM session WHERE session_id = $sid LIMIT 1")
        .bind(("sid", session_id.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(result)
}

/// Update the last_active timestamp of a session.
pub async fn update_session_last_active(db: &Surreal<Db>, session_id: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    db.query("UPDATE session SET last_active = $now WHERE session_id = $sid")
        .bind(("sid", session_id.to_string()))
        .bind(("now", now))
        .await?
        .check()?;
    Ok(())
}

/// List all sessions, ordered by last_active descending.
pub async fn list_sessions(db: &Surreal<Db>) -> Result<Vec<SessionRecord>> {
    let results: Vec<SessionRecord> = db
        .query("SELECT meta::id(id) AS id, session_id, tier, scope, channel, group_id, participants, created_at, last_active FROM session ORDER BY last_active DESC")
        .await?
        .check()?
        .take(0)?;
    Ok(results)
}

// ── Consolidation Sessions ────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone, SurrealValue)]
pub struct ConsolidationSessionRecord {
    pub id: Option<String>,
    pub session_id: String,
    pub status: String,
    pub created_at: String,
    pub expires_at: String,
    pub batches: Option<serde_json::Value>,
    pub batch_count: i64,
    pub message_count: i64,
}

pub async fn create_consolidation_session(
    db: &Surreal<Db>,
    session_id: &str,
    batches: &serde_json::Value,
    batch_count: usize,
    message_count: usize,
    ttl_minutes: u32,
) -> Result<()> {
    let now = chrono::Utc::now();
    let expires = now + chrono::Duration::minutes(ttl_minutes as i64);
    db.query(
        "CREATE consolidation_session SET \
         session_id = $sid, status = 'pending', \
         created_at = $now, expires_at = $exp, \
         batches = $batches, batch_count = $bc, message_count = $mc",
    )
    .bind(("sid", session_id.to_string()))
    .bind(("now", now.to_rfc3339()))
    .bind(("exp", expires.to_rfc3339()))
    .bind(("batches", batches.clone()))
    .bind(("bc", batch_count as i64))
    .bind(("mc", message_count as i64))
    .await?
    .check()?;
    Ok(())
}

pub async fn get_consolidation_session(
    db: &Surreal<Db>,
    session_id: &str,
) -> Result<Option<ConsolidationSessionRecord>> {
    let mut results: Vec<ConsolidationSessionRecord> = db
        .query(
            "SELECT meta::id(id) AS id, session_id, status, created_at, expires_at, \
             batches, batch_count, message_count \
             FROM consolidation_session WHERE session_id = $sid LIMIT 1",
        )
        .bind(("sid", session_id.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(results.pop())
}

pub async fn update_consolidation_session_status(
    db: &Surreal<Db>,
    session_id: &str,
    status: &str,
) -> Result<()> {
    db.query("UPDATE consolidation_session SET status = $status WHERE session_id = $sid")
        .bind(("sid", session_id.to_string()))
        .bind(("status", status.to_string()))
        .await?
        .check()?;
    Ok(())
}

pub async fn cleanup_expired_consolidation_sessions(db: &Surreal<Db>) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let results: Vec<ConsolidationSessionRecord> = db
        .query(
            "SELECT meta::id(id) AS id, session_id, status, created_at, expires_at, \
             batches, batch_count, message_count \
             FROM consolidation_session WHERE status = 'pending' AND expires_at < $now",
        )
        .bind(("now", now.clone()))
        .await?
        .check()?
        .take(0)?;
    let count = results.len();
    if count > 0 {
        db.query("UPDATE consolidation_session SET status = 'expired' WHERE status = 'pending' AND expires_at < $now")
            .bind(("now", now.clone()))
            .await?
            .check()?;
    }
    Ok(count)
}

/// Update a memory's d-tag (and derived scope/topic) in-place.
/// Used by d-tag migration (colon → slash format).
pub async fn update_memory_dtag(
    db: &Surreal<Db>,
    old_dtag: &str,
    new_dtag: &str,
    new_scope: &str,
    new_topic: &str,
) -> Result<bool> {
    let result: Option<MemoryRecord> = db
        .query("SELECT * FROM memory WHERE d_tag = $old LIMIT 1")
        .bind(("old", old_dtag.to_string()))
        .await?
        .check()?
        .take(0)?;

    if result.is_none() {
        return Ok(false);
    }

    let now = chrono::Utc::now().to_rfc3339();
    db.query(
        "UPDATE memory SET d_tag = $new_dtag, scope = $scope, topic = $topic, updated_at = $now \
         WHERE d_tag = $old_dtag",
    )
    .bind(("new_dtag", new_dtag.to_string()))
    .bind(("scope", new_scope.to_string()))
    .bind(("topic", new_topic.to_string()))
    .bind(("now", now))
    .bind(("old_dtag", old_dtag.to_string()))
    .await?
    .check()?;

    Ok(true)
}

