//! Search types: options, results, match types.

use nostr_sdk::Timestamp;

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
    pub content: String,
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
