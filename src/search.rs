use anyhow::Result;
use nostr_sdk::Timestamp;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tracing::debug;

use crate::db::{self, SearchDisplayResult};
use crate::embed::Embedder;

/// Minimum decay factor — memories never lose more than 80% of their confidence.
const MIN_DECAY: f64 = 0.2;
/// Maximum age in days for full decay (365 days).
const MAX_AGE_DAYS: f64 = 365.0;

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
    /// The d_tag for access tracking.
    pub d_tag: Option<String>,
}

/// Calculate confidence decay factor based on days since last access.
///
/// `effective_confidence = confidence × decay_factor`
/// `decay_factor = 1.0 - (days_since_access / max_age) × (1.0 - min_decay)`
///
/// Clamped to [MIN_DECAY, 1.0].
fn confidence_decay_factor(last_accessed: Option<&str>, created_at: &str) -> f64 {
    let reference_time = last_accessed.unwrap_or(created_at);
    let days_since = chrono::DateTime::parse_from_rfc3339(reference_time)
        .map(|dt| {
            let duration = chrono::Utc::now() - dt.with_timezone(&chrono::Utc);
            duration.num_hours() as f64 / 24.0
        })
        .unwrap_or(0.0)
        .max(0.0);

    let factor = 1.0 - (days_since / MAX_AGE_DAYS) * (1.0 - MIN_DECAY);
    factor.clamp(MIN_DECAY, 1.0)
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

    let mut results: Vec<SearchResult> = rows
        .into_iter()
        .map(|r| {
            let ts = chrono::DateTime::parse_from_rfc3339(&r.created_at)
                .map(|dt| Timestamp::from(dt.timestamp() as u64))
                .unwrap_or(Timestamp::from(0));

            let vec_score = r.vec_score.unwrap_or(0.0);
            let text_score = r.text_score.unwrap_or(0.0);
            let combined = r.combined.unwrap_or(0.0);

            // Apply confidence decay (TODO #4)
            let decay = confidence_decay_factor(
                r.last_accessed.as_deref(),
                &r.created_at,
            );
            let raw_confidence = r.confidence.unwrap_or(0.5);
            let effective_confidence = raw_confidence * decay;

            // Adjust combined score by decay factor
            let decayed_score = combined * decay;

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
                confidence: format!("{effective_confidence:.2}"),
                summary: r.summary.unwrap_or(r.content),
                created_at: ts,
                score: decayed_score,
                match_type,
                d_tag: r.d_tag,
            }
        })
        .collect();

    // Re-sort by decayed score
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    // Update access tracking for all results (TODO #3)
    let d_tags: Vec<String> = results
        .iter()
        .filter_map(|r| r.d_tag.clone())
        .collect();
    if !d_tags.is_empty() {
        db::update_access_tracking_batch(db, &d_tags).await.ok();
    }

    Ok(results)
}

/// Text-only search fallback.
async fn text_only_search(
    db: &Surreal<Db>,
    opts: &SearchOptions,
) -> Result<Vec<SearchResult>> {
    let display_results: Vec<SearchDisplayResult> =
        db::search_memories(db, &opts.query, opts.tier.as_deref(), opts.allowed_scopes.as_deref(), opts.limit).await?;

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
            d_tag: None,
        })
        .collect();

    Ok(results)
}
