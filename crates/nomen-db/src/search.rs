use anyhow::Result;
use serde::Deserialize;
use surrealdb::engine::local::Db;
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;

use crate::deserialize_option_string;

/// Search result from SurrealDB (text-only search)
#[derive(Debug, Deserialize, SurrealValue)]
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
#[derive(Debug, Deserialize, SurrealValue)]
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
#[derive(Debug, Deserialize, SurrealValue)]
pub struct MissingEmbeddingRow {
    pub d_tag: Option<String>,
    pub content: String,
}

/// Formatted search result for display.
///
/// `created_at` is a Unix timestamp (seconds since epoch) as u64.
pub struct SearchDisplayResult {
    pub tier: String,
    pub topic: String,
    pub content: String,
    pub created_at: u64,
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
                .map(|dt| dt.timestamp() as u64)
                .unwrap_or(0);

            SearchDisplayResult {
                tier: r.tier,
                topic: r.topic,
                content: r.content,
                created_at: ts,
            }
        })
        .collect();

    Ok(display_results)
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

/// Get memories that are missing embeddings.
pub async fn get_memories_without_embeddings(
    db: &Surreal<Db>,
    limit: usize,
) -> Result<Vec<MissingEmbeddingRow>> {
    let sql =
        format!("SELECT d_tag, content FROM memory WHERE embedding IS NONE LIMIT {limit}");
    let results: Vec<MissingEmbeddingRow> = db.query(&sql).await?.check()?.take(0)?;
    Ok(results)
}
