use anyhow::{Context, Result};
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, SurrealKv};
use tracing::debug;

use crate::memory::ParsedMemory;

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
