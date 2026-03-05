use anyhow::Result;
use nostr_sdk::Timestamp;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tracing::debug;

use crate::db::{self, SearchDisplayResult};
use crate::embed::Embedder;

/// How a search result was matched.
#[derive(Debug, Clone, Copy)]
pub enum MatchType {
    /// Matched by vector similarity only
    Vector,
    /// Matched by BM25 full-text only
    Text,
    /// Combined vector + text score
    Hybrid,
}

/// Options for hybrid search.
pub struct SearchOptions {
    pub query: String,
    pub tier: Option<String>,
    /// Allowed scopes (placeholder for group hierarchy expansion).
    /// Pass None to skip scope filtering.
    pub allowed_scopes: Option<Vec<String>>,
    pub limit: usize,
    pub vector_weight: f32,
    pub text_weight: f32,
    pub min_confidence: Option<f64>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            query: String::new(),
            tier: None,
            allowed_scopes: None,
            limit: 10,
            vector_weight: 0.7,
            text_weight: 0.3,
            min_confidence: None,
        }
    }
}

/// A search result with scoring info.
pub struct SearchResult {
    pub tier: String,
    pub topic: String,
    pub confidence: String,
    pub summary: String,
    pub created_at: Timestamp,
    pub score: f64,
    pub match_type: MatchType,
}

/// Run a search: hybrid if embedder is available, text-only fallback otherwise.
pub async fn search(
    db: &Surreal<Db>,
    embedder: &dyn Embedder,
    opts: &SearchOptions,
) -> Result<Vec<SearchResult>> {
    // If embedder has no dimensions (NoopEmbedder), fall back to text-only
    if embedder.dimensions() == 0 {
        debug!("No embedder configured, falling back to text-only search");
        return text_only_search(db, opts).await;
    }

    // Generate query embedding
    let query_embedding = match embedder.embed_one(&opts.query).await {
        Ok(emb) => emb,
        Err(e) => {
            tracing::warn!("Failed to generate query embedding, falling back to text-only: {e}");
            return text_only_search(db, opts).await;
        }
    };

    debug!("Running hybrid search (vector_weight={}, text_weight={})", opts.vector_weight, opts.text_weight);

    let rows = db::hybrid_search(
        db,
        &opts.query,
        &query_embedding,
        opts.tier.as_deref(),
        opts.allowed_scopes.as_deref(),
        opts.min_confidence,
        opts.vector_weight,
        opts.text_weight,
        opts.limit,
    )
    .await?;

    let results = rows
        .into_iter()
        .map(|r| {
            let ts = chrono::DateTime::parse_from_rfc3339(&r.created_at)
                .map(|dt| Timestamp::from(dt.timestamp() as u64))
                .unwrap_or(Timestamp::from(0));

            let vec_score = r.vec_score.unwrap_or(0.0);
            let text_score = r.text_score.unwrap_or(0.0);
            let combined = r.combined.unwrap_or(0.0);

            let match_type = if vec_score > 0.0 && text_score > 0.0 {
                MatchType::Hybrid
            } else if vec_score > 0.0 {
                MatchType::Vector
            } else {
                MatchType::Text
            };

            SearchResult {
                tier: r.tier,
                topic: r.topic,
                confidence: r
                    .confidence
                    .map(|c| format!("{c:.2}"))
                    .unwrap_or("?".to_string()),
                summary: r.summary.unwrap_or(r.content),
                created_at: ts,
                score: combined,
                match_type,
            }
        })
        .collect();

    Ok(results)
}

/// Text-only search fallback.
async fn text_only_search(
    db: &Surreal<Db>,
    opts: &SearchOptions,
) -> Result<Vec<SearchResult>> {
    let display_results: Vec<SearchDisplayResult> =
        db::search_memories(db, &opts.query, opts.tier.as_deref(), opts.limit).await?;

    let results = display_results
        .into_iter()
        .map(|r| SearchResult {
            tier: r.tier,
            topic: r.topic,
            confidence: r.confidence,
            summary: r.summary,
            created_at: r.created_at,
            score: 0.0,
            match_type: MatchType::Text,
        })
        .collect();

    Ok(results)
}
