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
    pub source: String,
    pub model: Option<String>,
    pub version: Option<i64>,
    pub d_tag: Option<String>,
    pub created_at: String,
    pub vec_score: Option<f64>,
    pub text_score: Option<f64>,
    pub combined: Option<f64>,
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

const SCHEMA: &str = r#"
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

DEFINE ANALYZER IF NOT EXISTS memory_analyzer TOKENIZERS class FILTERS ascii, lowercase, snowball(english);
DEFINE INDEX IF NOT EXISTS memory_fulltext ON memory FIELDS content SEARCH ANALYZER memory_analyzer BM25;
DEFINE INDEX IF NOT EXISTS memory_d_tag  ON memory FIELDS d_tag UNIQUE;
DEFINE INDEX IF NOT EXISTS memory_tier   ON memory FIELDS tier;
DEFINE INDEX IF NOT EXISTS memory_scope  ON memory FIELDS scope;
DEFINE INDEX IF NOT EXISTS memory_topic  ON memory FIELDS topic;
DEFINE INDEX IF NOT EXISTS memory_embedding ON memory FIELDS embedding HNSW DIMENSION 1536 DIST COSINE EFC 150 M 12;

DEFINE TABLE IF NOT EXISTS nomen_group SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id         ON nomen_group TYPE string;
DEFINE FIELD IF NOT EXISTS name       ON nomen_group TYPE string;
DEFINE FIELD IF NOT EXISTS parent     ON nomen_group TYPE option<string>;
DEFINE FIELD IF NOT EXISTS members    ON nomen_group TYPE array;
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
"#;

/// Initialize (or open) the SurrealDB database and apply schema.
pub async fn init_db() -> Result<Surreal<Db>> {
    let path = db_path();
    std::fs::create_dir_all(&path)
        .with_context(|| format!("Failed to create DB directory: {}", path.display()))?;

    debug!("Opening SurrealDB at {}", path.display());
    let db = Surreal::new::<SurrealKv>(path)
        .await
        .context("Failed to open SurrealDB")?;

    db.use_ns("nomen").use_db("nomen").await?;

    db.query(SCHEMA).await.context("Failed to apply schema")?;
    debug!("Schema applied");

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

/// Store a memory directly (not from a relay event).
pub async fn store_memory_direct(
    db: &Surreal<Db>,
    parsed: &ParsedMemory,
    event_id: &str,
) -> Result<()> {
    let record = build_record(parsed, event_id);
    let d_tag_owned = record.d_tag.clone().unwrap_or_default();

    db.query("DELETE FROM memory WHERE d_tag = $d_tag; CREATE memory CONTENT $record")
        .bind(("d_tag", d_tag_owned))
        .bind(("record", record))
        .await?
        .check()?;

    Ok(())
}

/// Full-text search for memories.
pub async fn search_memories(
    db: &Surreal<Db>,
    query: &str,
    tier: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchDisplayResult>> {
    let query_owned = query.to_string();
    let tier_owned = tier.map(|t| t.to_string());

    let results: Vec<TextSearchResult> = if let Some(ref tier_val) = tier_owned {
        let sql = format!(
            "SELECT *, search::score(1) AS score FROM memory \
             WHERE content @1@ $query AND tier = $tier \
             ORDER BY score DESC LIMIT {limit}"
        );
        db.query(&sql)
            .bind(("query", query_owned))
            .bind(("tier", tier_val.clone()))
            .await?
            .check()?
            .take(0)?
    } else {
        let sql = format!(
            "SELECT *, search::score(1) AS score FROM memory \
             WHERE content @1@ $query \
             ORDER BY score DESC LIMIT {limit}"
        );
        db.query(&sql)
            .bind(("query", query_owned))
            .await?
            .check()?
            .take(0)?
    };

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
        conditions.push("scope IN $scopes".to_string());
    }
    if min_confidence.is_some() {
        conditions.push("(confidence IS NONE OR confidence >= $min_conf)".to_string());
    }

    let where_clause = conditions.join(" AND ");

    let sql = format!(
        "SELECT *, \
           vector::similarity::cosine(embedding, $vec) AS vec_score, \
           search::score(1) AS text_score, \
           (vector::similarity::cosine(embedding, $vec) * $vw + search::score(1) * $tw) AS combined \
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
fn extract_scope(d_tag: &str) -> String {
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
    let sql = format!(
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated FROM raw_message WHERE consolidated = false ORDER BY created_at ASC LIMIT {limit}"
    );
    let results: Vec<RawMessageRecord> = db.query(&sql).await?.check()?.take(0)?;
    Ok(results)
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
