use std::collections::HashSet;

use anyhow::Result;
use nostr_sdk::Timestamp;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use tracing::debug;

use crate::db;
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
    /// Discovered via graph edge traversal
    Graph,
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
    /// If true, group results with >0.85 embedding similarity and merge them.
    pub aggregate: bool,
    /// Enable graph expansion: traverse edges from results to surface related memories.
    pub graph_expand: bool,
    /// Max hops for graph traversal (default: 1).
    pub max_hops: usize,
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
            aggregate: false,
            graph_expand: false,
            max_hops: 1,
        }
    }
}

/// A search result with scoring info.
pub struct SearchResult {
    pub tier: String,
    pub topic: String,
    pub confidence: String,
    pub summary: String,
    pub detail: String,
    pub created_at: Timestamp,
    pub score: f64,
    pub match_type: MatchType,
    /// The d_tag for access tracking.
    pub d_tag: Option<String>,
    /// Embedding vector (for aggregation similarity checks).
    pub embedding: Option<Vec<f32>>,
    /// The graph edge type that connected this result (only for Graph match type).
    pub graph_edge: Option<String>,
    /// True if this result contradicts one of the direct search hits.
    pub contradicts: bool,
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

    debug!(
        "Running hybrid search (vector_weight={}, text_weight={})",
        opts.vector_weight, opts.text_weight
    );

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

            // Apply confidence decay
            let decay = confidence_decay_factor(r.last_accessed.as_deref(), &r.created_at);
            let raw_confidence = r.confidence.unwrap_or(0.5);
            let effective_confidence = raw_confidence * decay;

            // Compute recency factor (1.0 for now, decays to 0.0 over MAX_AGE_DAYS)
            let recency = confidence_decay_factor(Some(&r.created_at), &r.created_at);

            // Importance normalized to 0.0–1.0 (importance 1-10 → 0.1–1.0)
            let importance_norm = r.importance.unwrap_or(5) as f64 / 10.0;

            // Composite score per spec: semantic×0.4 + text×0.3 + recency×0.15 + importance×0.15
            let decayed_score =
                vec_score * 0.4 + text_score * 0.3 + recency * 0.15 + importance_norm * 0.15;

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
                summary: r.summary.unwrap_or_else(|| r.content.clone()),
                detail: r.detail.unwrap_or_else(|| r.content.clone()),
                created_at: ts,
                score: decayed_score,
                match_type,
                d_tag: r.d_tag,
                embedding: r.embedding,
                graph_edge: None,
                contradicts: false,
            }
        })
        .collect();

    // Re-sort by decayed score
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Graph expansion: traverse edges from results to discover related memories
    if opts.graph_expand {
        results = graph_expand(db, results, opts.max_hops).await?;
    }

    // Aggregate similar results if requested
    if opts.aggregate {
        results = aggregate_results(results);
    }

    // Update access tracking for all results
    let d_tags: Vec<String> = results.iter().filter_map(|r| r.d_tag.clone()).collect();
    if !d_tags.is_empty() {
        db::update_access_tracking_batch(db, &d_tags).await.ok();
    }

    Ok(results)
}

/// Edge type weights for graph-expanded results.
/// Higher weight = more relevant connection type.
fn edge_type_weight(edge_type: &str, relation: Option<&str>) -> f64 {
    // If the references edge has a specific relation, use that for scoring
    if edge_type == "references" {
        if let Some(rel) = relation {
            return match rel {
                "contradicts" => 0.8,
                "supersedes" => 0.5,
                _ => 0.6,
            };
        }
        return 0.6;
    }
    match edge_type {
        "contradicts" => 0.8,
        "mentions" => 0.7,
        "references" => 0.6,
        "consolidated_from" => 0.3,
        _ => 0.3,
    }
}

/// Post-processing step: traverse graph edges from direct search hits to
/// discover related memories and merge them into the result set.
///
/// For each direct hit with a d_tag, queries SurrealDB for 1-hop neighbors
/// connected via mentions, references, contradicts, or consolidated_from edges.
/// Expanded results are scored based on the originating hit's score multiplied
/// by an edge-type weight. Results are deduped by d_tag.
async fn graph_expand(
    db: &Surreal<Db>,
    mut results: Vec<SearchResult>,
    max_hops: usize,
) -> Result<Vec<SearchResult>> {
    if max_hops == 0 {
        return Ok(results);
    }

    // Collect d_tags already in results for dedup
    let mut seen_d_tags: HashSet<String> = results.iter().filter_map(|r| r.d_tag.clone()).collect();

    let mut expanded: Vec<SearchResult> = Vec::new();

    // For each direct hit, traverse its edges
    for result in &results {
        let d_tag = match result.d_tag.as_ref() {
            Some(dt) => dt.clone(),
            None => continue,
        };

        let neighbors = match db::get_graph_neighbors_simple(db, &d_tag).await {
            Ok(n) => n,
            Err(e) => {
                debug!("Graph expansion failed for {d_tag}: {e}");
                continue;
            }
        };

        for neighbor in neighbors {
            // Skip if already in results
            if let Some(ref nd_tag) = neighbor.d_tag {
                if seen_d_tags.contains(nd_tag) {
                    continue;
                }
                seen_d_tags.insert(nd_tag.clone());
            }

            let effective_edge_type = if neighbor.edge_type == "references" {
                if let Some(ref rel) = neighbor.relation {
                    if rel == "contradicts" {
                        "contradicts"
                    } else {
                        "references"
                    }
                } else {
                    "references"
                }
            } else {
                &neighbor.edge_type
            };

            let weight = edge_type_weight(&neighbor.edge_type, neighbor.relation.as_deref());
            let graph_score = result.score * weight;

            let is_contradiction = effective_edge_type == "contradicts";

            // Parse timestamp
            let ts = neighbor
                .created_at
                .parse::<u64>()
                .map(Timestamp::from)
                .unwrap_or(Timestamp::now());

            // Apply confidence decay
            let raw_confidence = neighbor.confidence.unwrap_or(0.5);
            let decay =
                confidence_decay_factor(neighbor.last_accessed.as_deref(), &neighbor.created_at);
            let effective_confidence = raw_confidence * decay;

            expanded.push(SearchResult {
                tier: neighbor.tier,
                topic: neighbor.topic,
                confidence: format!("{effective_confidence:.2}"),
                summary: neighbor.summary.unwrap_or_else(|| neighbor.content.clone()),
                detail: neighbor.detail.unwrap_or_else(|| neighbor.content.clone()),
                created_at: ts,
                score: graph_score,
                match_type: MatchType::Graph,
                d_tag: neighbor.d_tag,
                embedding: None,
                graph_edge: Some(effective_edge_type.to_string()),
                contradicts: is_contradiction,
            });
        }
    }

    // Merge expanded results into the result list
    results.extend(expanded);

    // Re-sort by score
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(results)
}

/// Text-only search fallback.
///
/// Uses the same composite scoring as hybrid search but without the vector
/// component: `text_score × 0.3 + recency × 0.15 + importance × 0.15`.
/// This ensures results have meaningful scores even when embeddings are disabled,
/// preventing downstream filters from discarding all results.
async fn text_only_search(db: &Surreal<Db>, opts: &SearchOptions) -> Result<Vec<SearchResult>> {
    let mut conditions = vec!["content @1@ $query".to_string()];
    if opts.tier.is_some() {
        conditions.push("tier = $tier".to_string());
    }
    if opts.allowed_scopes.is_some() {
        conditions.push("(scope = \"\" OR array::any($scopes, |$s| scope = $s OR string::starts_with(scope, string::concat($s, \".\"))))".to_string());
    }
    if opts.min_confidence.is_some() {
        conditions.push("(confidence IS NONE OR confidence >= $min_conf)".to_string());
    }
    let where_clause = conditions.join(" AND ");

    let sql = format!(
        "SELECT *, search::score(1) AS text_score \
         FROM memory WHERE {where_clause} \
         ORDER BY text_score DESC LIMIT {}",
        opts.limit
    );

    let mut q = db.query(&sql).bind(("query", opts.query.clone()));
    if let Some(ref tier_val) = opts.tier {
        q = q.bind(("tier", tier_val.clone()));
    }
    if let Some(ref scopes) = opts.allowed_scopes {
        q = q.bind(("scopes", scopes.clone()));
    }
    if let Some(min_conf) = opts.min_confidence {
        q = q.bind(("min_conf", min_conf));
    }

    let rows: Vec<db::HybridSearchRow> = q.await?.check()?.take(0)?;

    let mut results: Vec<SearchResult> = rows
        .into_iter()
        .map(|r| {
            let ts = chrono::DateTime::parse_from_rfc3339(&r.created_at)
                .map(|dt| Timestamp::from(dt.timestamp() as u64))
                .unwrap_or(Timestamp::from(0));

            let text_score = r.text_score.unwrap_or(0.0);

            // Apply confidence decay
            let decay = confidence_decay_factor(r.last_accessed.as_deref(), &r.created_at);
            let raw_confidence = r.confidence.unwrap_or(0.5);
            let effective_confidence = raw_confidence * decay;

            let recency = confidence_decay_factor(Some(&r.created_at), &r.created_at);
            let importance_norm = r.importance.unwrap_or(5) as f64 / 10.0;

            // Same composite as hybrid search, minus vector component:
            // text×0.3 + recency×0.15 + importance×0.15
            let decayed_score = text_score * 0.3 + recency * 0.15 + importance_norm * 0.15;

            SearchResult {
                tier: r.tier,
                topic: r.topic,
                confidence: format!("{effective_confidence:.2}"),
                summary: r.summary.unwrap_or_else(|| r.content.clone()),
                detail: r.detail.unwrap_or_else(|| r.content.clone()),
                created_at: ts,
                score: decayed_score,
                match_type: MatchType::Text,
                d_tag: r.d_tag,
                embedding: None,
                graph_edge: None,
                contradicts: false,
            }
        })
        .collect();

    // Re-sort by composite score
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Update access tracking for text-only results too
    let d_tags: Vec<String> = results.iter().filter_map(|r| r.d_tag.clone()).collect();
    if !d_tags.is_empty() {
        db::update_access_tracking_batch(db, &d_tags).await.ok();
    }

    Ok(results)
}

/// Cosine similarity between two embedding vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

/// Aggregate search results: group results with >0.85 embedding similarity
/// and merge them into a single result with combined detail and highest confidence.
fn aggregate_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    if results.len() <= 1 {
        return results;
    }

    let mut merged: Vec<bool> = vec![false; results.len()];
    let mut aggregated: Vec<SearchResult> = Vec::new();

    for i in 0..results.len() {
        if merged[i] {
            continue;
        }

        let mut group_indices = vec![i];

        if let Some(ref emb_a) = results[i].embedding {
            for j in (i + 1)..results.len() {
                if merged[j] {
                    continue;
                }
                if let Some(ref emb_b) = results[j].embedding {
                    let sim = cosine_similarity(emb_a, emb_b);
                    if sim > 0.85 {
                        group_indices.push(j);
                        merged[j] = true;
                    }
                }
            }
        }

        merged[i] = true;

        if group_indices.len() == 1 {
            aggregated.push(SearchResult {
                tier: results[i].tier.clone(),
                topic: results[i].topic.clone(),
                confidence: results[i].confidence.clone(),
                summary: results[i].summary.clone(),
                detail: results[i].detail.clone(),
                created_at: results[i].created_at,
                score: results[i].score,
                match_type: results[i].match_type,
                d_tag: results[i].d_tag.clone(),
                embedding: results[i].embedding.clone(),
                graph_edge: results[i].graph_edge.clone(),
                contradicts: results[i].contradicts,
            });
        } else {
            // Use highest-scoring result as base, combine details
            let best_idx = *group_indices
                .iter()
                .max_by(|&&a, &&b| {
                    results[a]
                        .score
                        .partial_cmp(&results[b].score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap();

            let max_confidence: f64 = group_indices
                .iter()
                .filter_map(|&idx| results[idx].confidence.parse::<f64>().ok())
                .fold(0.0f64, f64::max);

            let mut combined_detail = String::new();
            let mut topics: Vec<&str> = Vec::new();
            for &idx in &group_indices {
                if !topics.contains(&results[idx].topic.as_str()) {
                    topics.push(&results[idx].topic);
                }
                if !combined_detail.is_empty() {
                    combined_detail.push_str("\n---\n");
                }
                combined_detail
                    .push_str(&format!("[{}] {}", results[idx].topic, results[idx].detail));
            }

            let topic_display = if topics.len() > 1 {
                format!(
                    "{} (+{} related)",
                    results[best_idx].topic,
                    topics.len() - 1
                )
            } else {
                results[best_idx].topic.clone()
            };

            aggregated.push(SearchResult {
                tier: results[best_idx].tier.clone(),
                topic: topic_display,
                confidence: format!("{max_confidence:.2}"),
                summary: results[best_idx].summary.clone(),
                detail: combined_detail,
                created_at: results[best_idx].created_at,
                score: results[best_idx].score,
                match_type: results[best_idx].match_type,
                d_tag: results[best_idx].d_tag.clone(),
                embedding: results[best_idx].embedding.clone(),
                graph_edge: results[best_idx].graph_edge.clone(),
                contradicts: results[best_idx].contradicts,
            });
        }
    }

    aggregated
}
