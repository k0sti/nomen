//! Operation option and report types shared across crates.

/// Options for listing memories.
pub struct ListOptions {
    pub tier: Option<String>,
    pub limit: usize,
    pub include_stats: bool,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            tier: None,
            limit: 100,
            include_stats: false,
        }
    }
}

/// Memory statistics.
pub struct ListStats {
    pub total: usize,
    pub named: usize,
    pub pending: usize,
}

/// Report from a sync operation.
pub struct SyncReport {
    pub stored: usize,
    pub skipped: usize,
    pub errors: usize,
}

/// Report from an embed operation.
pub struct EmbedReport {
    pub embedded: usize,
    pub total: usize,
}

/// API-friendly consolidation options (no Box<dyn ...> fields).
pub struct ConsolidateParams {
    pub batch_size: usize,
    pub min_messages: usize,
}

impl Default for ConsolidateParams {
    fn default() -> Self {
        Self {
            batch_size: 50,
            min_messages: 3,
        }
    }
}

/// API-friendly cluster fusion options (no Box<dyn ...> fields).
pub struct ClusterParams {
    pub min_members: usize,
    pub namespace_depth: usize,
    pub dry_run: bool,
    pub prefix_filter: Option<String>,
}

impl Default for ClusterParams {
    fn default() -> Self {
        Self {
            min_members: 3,
            namespace_depth: 2,
            dry_run: false,
            prefix_filter: None,
        }
    }
}
