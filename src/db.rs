use anyhow::{Context, Result};
use nostr_sdk::prelude::*;
use serde::{Deserialize, Deserializer, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, SurrealKv};
use tracing::debug;

use crate::memory::ParsedMemory;

/// Deserialize SurrealDB NONE/null as None for Option<String>.
pub fn deserialize_option_string<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
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
        fn visit_none<E: de::Error>(self) -> std::result::Result<Option<String>, E> { Ok(None) }
        fn visit_unit<E: de::Error>(self) -> std::result::Result<Option<String>, E> { Ok(None) }
        fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> std::result::Result<Option<String>, D2::Error> {
            Ok(Some(String::deserialize(d)?))
        }
        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Option<String>, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Option<String>, E> {
            Ok(Some(v))
        }
        fn visit_enum<A: de::EnumAccess<'de>>(self, data: A) -> std::result::Result<Option<String>, A::Error> {
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
        fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<String, A::Error> {
            let mut tb = String::new();
            let mut id = String::new();
            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "tb" => tb = map.next_value()?,
                    "id" => {
                        id = map.next_value::<serde_json::Value>()?.to_string().trim_matches('"').to_string();
                    },
                    _ => { let _ = map.next_value::<serde_json::Value>()?; }
                }
            }
            Ok(format!("{tb}:{id}"))
        }
        fn visit_enum<A: de::EnumAccess<'de>>(self, data: A) -> std::result::Result<String, A::Error> {
            // SurrealDB Thing can be serialized as an enum (internal representation)
            let (variant, _): (String, _) = data.variant()?;
            Ok(variant)
        }
    }
    deserializer.deserialize_any(ThingOrString)
}

/// SurrealDB memory record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub content: String,
    pub summary: Option<String>,
    pub embedding: Option<Vec<f32>>,
    pub tier: String,
    pub scope: String,
    pub topic: String,
    pub confidence: Option<f64>,
    pub source: String,
    pub model: Option<String>,
    pub version: i64,
    pub nostr_id: Option<String>,
    pub d_tag: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub ephemeral: bool,
}

/// Version check result
#[derive(Debug, Deserialize)]
struct VersionCheck {
    version: i64,
}

/// Search result from SurrealDB (text-only search)
#[derive(Debug, Deserialize)]
pub struct TextSearchResult {
    pub content: String,
    pub summary: Option<String>,
    pub tier: String,
    pub topic: String,
    pub confidence: Option<f64>,
    pub created_at: String,
    #[allow(dead_code)]
    pub score: Option<f64>,
}

/// Hybrid search result from SurrealDB
#[derive(Debug, Deserialize)]
pub struct HybridSearchRow {
    pub content: String,
    pub summary: Option<String>,
    pub tier: String,
    pub scope: String,
    pub topic: String,
    pub confidence: Option<f64>,
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
#[derive(Debug, Deserialize)]
pub struct MissingEmbeddingRow {
    pub d_tag: Option<String>,
    pub content: String,
    pub summary: Option<String>,
}

/// Formatted search result for display
pub struct SearchDisplayResult {
    pub tier: String,
    pub topic: String,
    pub confidence: String,
    pub summary: String,
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
DEFINE FIELD IF NOT EXISTS content    ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS summary    ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS embedding  ON memory TYPE option<array<float>>;
DEFINE FIELD IF NOT EXISTS tier       ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS scope      ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS topic      ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS confidence ON memory TYPE option<float>;
DEFINE FIELD IF NOT EXISTS source     ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS model      ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS version    ON memory TYPE int DEFAULT 1;
DEFINE FIELD IF NOT EXISTS nostr_id   ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS d_tag      ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS ephemeral  ON memory TYPE bool DEFAULT false;
DEFINE FIELD IF NOT EXISTS consolidated_from ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS consolidated_at   ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS last_accessed     ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS access_count      ON memory TYPE int DEFAULT 0;
DEFINE FIELD IF NOT EXISTS importance        ON memory TYPE option<int>;
-- Note: created_at/updated_at remain TYPE string (not datetime) because SurrealDB
-- datetime serialization requires special handling in Rust serde. RFC3339 strings
-- still support lexicographic ordering which is sufficient for our queries.

DEFINE ANALYZER IF NOT EXISTS memory_analyzer TOKENIZERS class FILTERS ascii, lowercase, snowball(english);
DEFINE INDEX IF NOT EXISTS memory_fulltext ON memory FIELDS content SEARCH ANALYZER memory_analyzer BM25;
DEFINE INDEX IF NOT EXISTS memory_d_tag  ON memory FIELDS d_tag UNIQUE;
DEFINE INDEX IF NOT EXISTS memory_tier   ON memory FIELDS tier;
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

DEFINE INDEX IF NOT EXISTS raw_msg_source ON raw_message FIELDS source, source_id UNIQUE;
DEFINE INDEX IF NOT EXISTS raw_msg_sender ON raw_message FIELDS sender;

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
        .await
        .context("Failed to open SurrealDB")?;

    db.use_ns("nomen").use_db("nomen").await?;

    db.query(SCHEMA_BASE).await.context("Failed to apply base schema")?;

    // Apply HNSW index with configurable dimensions
    let hnsw_sql = format!(
        "DEFINE INDEX IF NOT EXISTS memory_embedding ON memory FIELDS embedding HNSW DIMENSION {dimensions} DIST COSINE EFC 150 M 12"
    );
    db.query(&hnsw_sql).await.context("Failed to apply HNSW index")?;
    debug!(dimensions, "Schema applied with HNSW dimensions");

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
    let confidence: Option<f64> = parsed.confidence.parse().ok();

    MemoryRecord {
        content: parsed.content_raw.clone(),
        summary: Some(parsed.summary.clone()),
        embedding: None,
        tier: crate::memory::base_tier(&parsed.tier).to_string(),
        scope,
        topic: parsed.topic.clone(),
        confidence,
        source: parsed.source.clone(),
        model: Some(parsed.model.clone()),
        version,
        nostr_id: Some(nostr_id.to_string()),
        d_tag: Some(parsed.d_tag.clone()),
        created_at: created,
        updated_at: now,
        ephemeral: false,
    }
}

/// Store a parsed memory into SurrealDB. Returns Ok(true) if inserted/updated, Ok(false) if skipped.
pub async fn store_memory(
    db: &Surreal<Db>,
    parsed: &ParsedMemory,
    event: &Event,
) -> Result<bool> {
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
         CREATE memory CONTENT $record"
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

    db.query("DELETE FROM memory WHERE d_tag = $d_tag; CREATE memory CONTENT $record")
        .bind(("d_tag", d_tag_owned.clone()))
        .bind(("record", record))
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

    let mut conditions = vec!["content @1@ $query".to_string()];
    if tier.is_some() {
        conditions.push("tier = $tier".to_string());
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
                tier: r.tier,
                topic: r.topic,
                confidence: r
                    .confidence
                    .map(|c| format!("{c:.2}"))
                    .unwrap_or("?".to_string()),
                summary: r.summary.unwrap_or(r.content),
                created_at: ts,
            }
        })
        .collect();

    Ok(display_results)
}

/// Update an existing memory's embedding by d-tag.
pub async fn store_embedding(
    db: &Surreal<Db>,
    d_tag: &str,
    embedding: Vec<f32>,
) -> Result<()> {
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
) -> Result<Vec<MemoryRecord>> {
    let (sql, bind_tier);
    if let Some(t) = tier {
        sql = format!(
            "SELECT * FROM memory WHERE tier = $tier ORDER BY created_at DESC LIMIT {limit}"
        );
        bind_tier = Some(t.to_string());
    } else {
        sql = format!(
            "SELECT * FROM memory ORDER BY created_at DESC LIMIT {limit}"
        );
        bind_tier = None;
    }

    let mut q = db.query(&sql);
    if let Some(ref t) = bind_tier {
        q = q.bind(("tier", t.clone()));
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
    let sql = format!(
        "SELECT d_tag, content, summary FROM memory WHERE embedding IS NONE LIMIT {limit}"
    );
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
    min_confidence: Option<f64>,
    vector_weight: f32,
    text_weight: f32,
    limit: usize,
) -> Result<Vec<HybridSearchRow>> {
    let mut conditions = vec!["content @1@ $query".to_string()];

    if tier.is_some() {
        conditions.push("tier = $tier".to_string());
    }
    if allowed_scopes.is_some() {
        conditions.push("(scope = \"\" OR array::any($scopes, |$s| scope = $s OR string::starts_with(scope, string::concat($s, \".\"))))".to_string());
    }
    if min_confidence.is_some() {
        conditions.push("(confidence IS NONE OR confidence >= $min_conf)".to_string());
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
    if let Some(min_conf) = min_confidence {
        q = q.bind(("min_conf", min_conf));
    }

    let results: Vec<HybridSearchRow> = q.await?.check()?.take(0)?;
    Ok(results)
}

/// Get a single memory by d-tag (topic).
pub async fn get_memory_by_dtag(db: &Surreal<Db>, d_tag: &str) -> Result<Option<MemoryRecord>> {
    let mut results: Vec<MemoryRecord> = db
        .query("SELECT * FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(results.pop())
}

/// Get a single memory by topic field (raw topic, not d-tag).
pub async fn get_memory_by_topic(db: &Surreal<Db>, topic: &str) -> Result<Option<MemoryRecord>> {
    let mut results: Vec<MemoryRecord> = db
        .query("SELECT * FROM memory WHERE topic = $topic LIMIT 1")
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
    if crate::memory::is_v2_dtag(d_tag) {
        let mut parts = d_tag.splitn(3, ':');
        let _visibility = parts.next();
        let context = parts.next().unwrap_or("");
        return context.to_string();
    }

    // v0.1 legacy formats
    if d_tag.starts_with("snowclaw:memory:npub:") {
        d_tag.strip_prefix("snowclaw:memory:npub:").unwrap_or("").to_string()
    } else if d_tag.starts_with("snowclaw:memory:group:") {
        d_tag.strip_prefix("snowclaw:memory:group:").unwrap_or("").to_string()
    } else {
        String::new()
    }
}

// ── Raw Message CRUD ────────────────────────────────────────────────

use crate::ingest::{RawMessage, RawMessageRecord, MessageQuery};
use crate::entities::{EntityRecord, EntityKind};

/// Store a raw message into SurrealDB.
pub async fn store_raw_message(db: &Surreal<Db>, msg: &RawMessage) -> Result<String> {
    let now = chrono::Utc::now().to_rfc3339();
    let created = msg.created_at.as_deref().unwrap_or(&now);
    let source_id = msg.source_id.clone().unwrap_or_default();
    let channel = msg.channel.clone().unwrap_or_default();
    // Use serde-based record creation to avoid bind serialization issues
    #[derive(Serialize)]
    struct NewRawMessage {
        source: String,
        source_id: String,
        sender: String,
        channel: String,
        content: String,
        metadata: String,
        created_at: String,
        consolidated: bool,
    }

    let metadata = msg.metadata.clone().unwrap_or_default();

    let record = NewRawMessage {
        source: msg.source.clone(),
        source_id: source_id,
        sender: msg.sender.clone(),
        channel,
        content: msg.content.clone(),
        metadata,
        created_at: created.to_string(),
        consolidated: false,
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
    if opts.consolidated_only {
        conditions.push("consolidated = true".to_string());
    } else {
        conditions.push("consolidated = false".to_string());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let limit = opts.limit.unwrap_or(100);
    let sql = format!(
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated FROM raw_message {where_clause} ORDER BY created_at ASC LIMIT {limit}"
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

    let results: Vec<RawMessageRecord> = q.await?.check()?.take(0)?;
    Ok(results)
}

/// Mark raw messages as consolidated by their IDs.
pub async fn mark_messages_consolidated(db: &Surreal<Db>, ids: &[String]) -> Result<()> {
    for id in ids {
        db.query("UPDATE $id SET consolidated = true")
            .bind(("id", surrealdb::sql::Thing::from(("raw_message", id.as_str()))))
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
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated FROM raw_message WHERE {where_clause} ORDER BY created_at ASC LIMIT {limit}"
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
    db.query("UPDATE memory SET consolidated_from = $from, consolidated_at = $at WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .bind(("from", consolidated_from.to_string()))
        .bind(("at", consolidated_at.to_string()))
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

/// Create a "references" edge between two memories with a relation type.
pub async fn create_references_edge(
    db: &Surreal<Db>,
    from_d_tag: &str,
    to_d_tag: &str,
    relation: &str,
) -> Result<()> {
    // Resolve d_tags to record IDs
    #[derive(Deserialize)]
    struct IdRow {
        #[serde(deserialize_with = "deserialize_thing_as_string")]
        id: String,
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

    let from_id = from_rows.first().map(|r| &r.id).ok_or_else(|| anyhow::anyhow!("Memory not found: {from_d_tag}"))?;
    let to_id = to_rows.first().map(|r| &r.id).ok_or_else(|| anyhow::anyhow!("Memory not found: {to_d_tag}"))?;

    // Parse table:id format
    let (from_tb, from_rid) = from_id.split_once(':').unwrap_or(("memory", from_id));
    let (to_tb, to_rid) = to_id.split_once(':').unwrap_or(("memory", to_id));

    db.query("RELATE $from->references->$to SET relation = $relation, created_at = $now")
        .bind(("from", surrealdb::sql::Thing::from((from_tb, from_rid))))
        .bind(("to", surrealdb::sql::Thing::from((to_tb, to_rid))))
        .bind(("relation", relation.to_string()))
        .bind(("now", chrono::Utc::now().to_rfc3339()))
        .await?
        .check()?;
    Ok(())
}

/// Get unconsolidated messages older than a cutoff, optionally filtered to ephemeral only.
pub async fn get_ephemeral_messages_before(
    db: &Surreal<Db>,
    before: &str,
    limit: usize,
) -> Result<Vec<RawMessageRecord>> {
    let sql = format!(
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated FROM raw_message WHERE created_at < $before ORDER BY created_at ASC LIMIT {limit}"
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
    #[derive(Deserialize)]
    struct CountResult { count: usize }
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
    #[derive(Deserialize)]
    struct CountRow { count: usize }

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

/// Query messages around a specific source_id: N messages before and after.
pub async fn query_messages_around(
    db: &Surreal<Db>,
    source_id: &str,
    context_count: usize,
) -> Result<Vec<RawMessageRecord>> {
    // First, find the target message's created_at
    #[derive(Deserialize)]
    struct TimeRow { created_at: String }

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
    let before_sql = format!(
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated \
         FROM raw_message WHERE created_at <= $pivot ORDER BY created_at DESC LIMIT {}",
        context_count + 1
    );
    let after_sql = format!(
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated \
         FROM raw_message WHERE created_at > $pivot ORDER BY created_at ASC LIMIT {context_count}"
    );

    let mut result = db
        .query(&before_sql)
        .query(&after_sql)
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
    #[derive(Deserialize)]
    struct CountResult { count: usize }
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
#[derive(Debug, Deserialize, serde::Serialize)]
pub struct PrunableMemory {
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub d_tag: Option<String>,
    pub topic: String,
    pub confidence: Option<f64>,
    pub access_count: Option<i64>,
    pub created_at: String,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub last_accessed: Option<String>,
}

/// Find memories eligible for pruning based on age and access patterns.
///
/// Pruning rules (from spec):
/// - access_count = 0 AND age > max_days
/// - confidence < 0.3 AND age > 30 days
/// - access_count = 0 AND confidence < 0.5 AND age > 30 days
pub async fn find_prunable_memories(
    db: &Surreal<Db>,
    max_days: u64,
) -> Result<Vec<PrunableMemory>> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(max_days as i64);
    let cutoff_str = cutoff.to_rfc3339();
    let cutoff_30d = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();

    let sql = "SELECT d_tag, topic, confidence, access_count, created_at, last_accessed \
               FROM memory WHERE \
               (access_count = 0 AND created_at < $cutoff) OR \
               (confidence IS NOT NONE AND confidence < 0.3 AND created_at < $cutoff_30d) OR \
               (access_count = 0 AND confidence IS NOT NONE AND confidence < 0.5 AND created_at < $cutoff_30d)";

    let results: Vec<PrunableMemory> = db
        .query(sql)
        .bind(("cutoff", cutoff_str))
        .bind(("cutoff_30d", cutoff_30d))
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

// ── Entity CRUD ─────────────────────────────────────────────────────

/// Store an entity (upsert by name).
pub async fn store_entity(
    db: &Surreal<Db>,
    name: &str,
    kind: &EntityKind,
) -> Result<String> {
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
            }"
        )
        .bind(("name", name.to_string()))
        .bind(("kind", kind_str.to_string()))
        .bind(("created_at", now))
        .await?
        .check()?
        .take(0)?;

    let id = result
        .first()
        .map(|r| r.id.clone())
        .unwrap_or_default();
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
        .bind(("from", surrealdb::sql::Thing::from(("memory", memory_id))))
        .bind(("to", surrealdb::sql::Thing::from(("entity", entity_id))))
        .bind(("relevance", relevance))
        .await?
        .check()?;
    Ok(())
}

/// Create a "consolidated_from" edge from a consolidated memory to a raw message.
pub async fn create_consolidated_edge(
    db: &Surreal<Db>,
    memory_id: &str,
    raw_message_id: &str,
) -> Result<()> {
    db.query("RELATE $from->consolidated_from->$to")
        .bind(("from", surrealdb::sql::Thing::from(("memory", memory_id))))
        .bind(("to", surrealdb::sql::Thing::from(("raw_message", raw_message_id))))
        .await?
        .check()?;
    Ok(())
}

// ── Meta key-value store ─────────────────────────────────────────────

/// Get a meta value by key.
pub async fn get_meta(db: &Surreal<Db>, key: &str) -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct MetaRow { val: String }
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
    #[derive(Deserialize)]
    struct CountRow { count: usize }
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

    #[derive(Serialize)]
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
