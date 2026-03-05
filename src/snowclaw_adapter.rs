//! Snowclaw Memory trait adapter for Nomen.
//!
//! Bridges Snowclaw's `Memory` trait to Nomen's SurrealDB-backed storage,
//! providing hybrid search, tier-aware recall, and group message queries.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use snow_memory::types::MemoryTier;

use crate::db;
use crate::ingest::MessageQuery;
use crate::search::SearchOptions;
use crate::Nomen;

// ── Re-exported Snowclaw-compatible types ───────────────────────────
// These mirror the definitions in snowclaw/src/memory/traits.rs so that
// Nomen can implement the interface without depending on the full
// zeroclaw crate.

/// A single memory entry (mirrors snowclaw MemoryEntry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub key: String,
    pub content: String,
    pub category: MemoryCategory,
    pub timestamp: String,
    pub session_id: Option<String>,
    pub score: Option<f64>,
}

/// Memory categories for organization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    Core,
    Daily,
    Conversation,
    Custom(String),
}

impl std::fmt::Display for MemoryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core => write!(f, "core"),
            Self::Daily => write!(f, "daily"),
            Self::Conversation => write!(f, "conversation"),
            Self::Custom(name) => write!(f, "{name}"),
        }
    }
}

/// Context for tier-aware recall filtering.
#[derive(Debug, Clone)]
pub struct RecallContext {
    pub is_main_session: bool,
    pub channel: Option<String>,
    pub group_id: Option<String>,
}

/// Core memory trait — mirrors snowclaw's Memory trait.
#[async_trait]
pub trait Memory: Send + Sync {
    fn name(&self) -> &str;

    async fn store(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
    ) -> anyhow::Result<()>;

    async fn recall(
        &self,
        query: &str,
        limit: usize,
        session_id: Option<&str>,
        context: Option<&RecallContext>,
    ) -> anyhow::Result<Vec<MemoryEntry>>;

    async fn get(&self, key: &str) -> anyhow::Result<Option<MemoryEntry>>;

    async fn list(
        &self,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>>;

    async fn forget(&self, key: &str) -> anyhow::Result<bool>;

    async fn count(&self) -> anyhow::Result<usize>;

    async fn store_with_tier(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        tier: MemoryTier,
    ) -> anyhow::Result<()>;

    async fn health_check(&self) -> bool;

    async fn recent_group_messages(
        &self,
        group_id: &str,
        limit: usize,
    ) -> Vec<MemoryEntry>;
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Map a MemoryCategory to a Nomen topic prefix.
fn category_to_topic_prefix(cat: &MemoryCategory) -> &str {
    match cat {
        MemoryCategory::Core => "core:",
        MemoryCategory::Daily => "daily:",
        MemoryCategory::Conversation => "conversation:",
        MemoryCategory::Custom(name) => {
            // Return a static prefix; the caller appends the key.
            // We leak nothing — just use the custom name directly.
            let _ = name;
            ""
        }
    }
}

/// Build a full topic from category + key.
fn build_topic(category: &MemoryCategory, key: &str) -> String {
    match category {
        MemoryCategory::Custom(name) => format!("{name}:{key}"),
        other => format!("{}{key}", category_to_topic_prefix(other)),
    }
}

/// Map MemoryTier to Nomen tier string.
fn tier_to_nomen(tier: &MemoryTier) -> String {
    match tier {
        MemoryTier::Public => "public".to_string(),
        MemoryTier::Group(id) => format!("group:{id}"),
        MemoryTier::Private(pk) => format!("private:{pk}"),
    }
}

/// Determine allowed tiers based on RecallContext.
fn context_to_tier_filter(ctx: Option<&RecallContext>) -> Option<String> {
    match ctx {
        None => None,
        Some(ctx) => {
            if ctx.is_main_session {
                // Main session sees everything — no tier filter
                None
            } else if ctx.group_id.is_some() {
                // Group context: public + matching group
                // We filter to public here; group scoping is handled via allowed_scopes
                Some("public".to_string())
            } else {
                Some("public".to_string())
            }
        }
    }
}

/// Build allowed scopes from RecallContext.
fn context_to_scopes(ctx: Option<&RecallContext>) -> Option<Vec<String>> {
    match ctx {
        None => None,
        Some(ctx) => {
            let mut scopes = vec![String::new()]; // empty scope = public
            if let Some(ref group_id) = ctx.group_id {
                scopes.push(group_id.clone());
            }
            if ctx.is_main_session {
                // Main session sees all scopes — return None for no filtering
                return None;
            }
            Some(scopes)
        }
    }
}

/// SurrealDB count result.
#[derive(Debug, Deserialize)]
struct CountResult {
    count: usize,
}

/// SurrealDB memory row for listing.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MemoryRow {
    #[serde(default, deserialize_with = "db::deserialize_thing_as_string")]
    id: String,
    content: String,
    #[serde(default)]
    summary: Option<String>,
    tier: String,
    topic: String,
    #[serde(default)]
    confidence: Option<f64>,
    created_at: String,
    #[serde(default, deserialize_with = "db::deserialize_option_string")]
    d_tag: Option<String>,
}

impl MemoryRow {
    fn into_entry(self, category: MemoryCategory) -> MemoryEntry {
        MemoryEntry {
            id: self.id,
            key: self.topic.clone(),
            content: self.summary.unwrap_or(self.content),
            category,
            timestamp: self.created_at,
            session_id: None,
            score: self.confidence,
        }
    }
}

/// Infer MemoryCategory from a topic string.
fn topic_to_category(topic: &str) -> MemoryCategory {
    if topic.starts_with("core:") {
        MemoryCategory::Core
    } else if topic.starts_with("daily:") {
        MemoryCategory::Daily
    } else if topic.starts_with("conversation:") {
        MemoryCategory::Conversation
    } else if let Some(idx) = topic.find(':') {
        MemoryCategory::Custom(topic[..idx].to_string())
    } else {
        MemoryCategory::Custom(topic.to_string())
    }
}

// ── NomenAdapter ────────────────────────────────────────────────────

/// Adapter that delegates Snowclaw Memory trait calls to a Nomen instance.
pub struct NomenAdapter {
    nomen: Nomen,
}

impl NomenAdapter {
    /// Create a new adapter wrapping a Nomen instance.
    pub fn new(nomen: Nomen) -> Self {
        Self { nomen }
    }

    /// Access the inner Nomen instance.
    pub fn inner(&self) -> &Nomen {
        &self.nomen
    }
}

#[async_trait]
impl Memory for NomenAdapter {
    fn name(&self) -> &str {
        "nomen"
    }

    async fn store(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        _session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let topic = build_topic(&category, key);
        self.nomen
            .store(crate::NewMemory {
                topic,
                summary: content.to_string(),
                detail: String::new(),
                tier: "public".to_string(),
                confidence: 0.8,
            })
            .await?;
        Ok(())
    }

    async fn recall(
        &self,
        query: &str,
        limit: usize,
        _session_id: Option<&str>,
        context: Option<&RecallContext>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let tier = context_to_tier_filter(context);
        let allowed_scopes = context_to_scopes(context);

        let results = self
            .nomen
            .search(SearchOptions {
                query: query.to_string(),
                tier,
                allowed_scopes,
                limit,
                ..Default::default()
            })
            .await?;

        let entries = results
            .into_iter()
            .map(|r| {
                let category = topic_to_category(&r.topic);
                MemoryEntry {
                    id: r.topic.clone(),
                    key: r.topic,
                    content: r.summary,
                    category,
                    timestamp: {
                        let secs = r.created_at.as_u64() as i64;
                        chrono::DateTime::from_timestamp(secs, 0)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_default()
                    },
                    session_id: None,
                    score: Some(r.score),
                }
            })
            .collect();

        Ok(entries)
    }

    async fn get(&self, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        let d_tag = format!("snow:memory:{key}");
        let rows: Vec<MemoryRow> = self
            .nomen
            .db()
            .query("SELECT * FROM memory WHERE d_tag = $d_tag LIMIT 1")
            .bind(("d_tag", d_tag))
            .await?
            .check()?
            .take(0)?;

        Ok(rows.into_iter().next().map(|r| {
            let category = topic_to_category(&r.topic);
            r.into_entry(category)
        }))
    }

    async fn list(
        &self,
        category: Option<&MemoryCategory>,
        _session_id: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let rows: Vec<MemoryRow> = if let Some(cat) = category {
            let prefix = match cat {
                MemoryCategory::Core => "core:",
                MemoryCategory::Daily => "daily:",
                MemoryCategory::Conversation => "conversation:",
                MemoryCategory::Custom(name) => name.as_str(),
            };
            self.nomen
                .db()
                .query(
                    "SELECT * FROM memory WHERE string::starts_with(topic, $prefix) ORDER BY created_at DESC",
                )
                .bind(("prefix", prefix.to_string()))
                .await?
                .check()?
                .take(0)?
        } else {
            self.nomen
                .db()
                .query("SELECT * FROM memory ORDER BY created_at DESC")
                .await?
                .check()?
                .take(0)?
        };

        let entries = rows
            .into_iter()
            .map(|r| {
                let cat = category.cloned().unwrap_or_else(|| topic_to_category(&r.topic));
                r.into_entry(cat)
            })
            .collect();

        Ok(entries)
    }

    async fn forget(&self, key: &str) -> anyhow::Result<bool> {
        // Try to delete by topic
        self.nomen.delete(Some(key), None).await?;
        Ok(true)
    }

    async fn count(&self) -> anyhow::Result<usize> {
        let result: Vec<CountResult> = self
            .nomen
            .db()
            .query("SELECT count() AS count FROM memory GROUP ALL")
            .await?
            .check()?
            .take(0)?;

        Ok(result.first().map(|r| r.count).unwrap_or(0))
    }

    async fn store_with_tier(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        tier: MemoryTier,
    ) -> anyhow::Result<()> {
        let topic = build_topic(&category, key);
        self.nomen
            .store(crate::NewMemory {
                topic,
                summary: content.to_string(),
                detail: String::new(),
                tier: tier_to_nomen(&tier),
                confidence: 0.8,
            })
            .await?;
        Ok(())
    }

    async fn health_check(&self) -> bool {
        self.nomen
            .db()
            .query("RETURN true")
            .await
            .is_ok()
    }

    async fn recent_group_messages(
        &self,
        group_id: &str,
        limit: usize,
    ) -> Vec<MemoryEntry> {
        let query = MessageQuery {
            channel: Some(group_id.to_string()),
            limit: Some(limit),
            ..Default::default()
        };

        match self.nomen.get_messages(query).await {
            Ok(messages) => messages
                .into_iter()
                .map(|m| MemoryEntry {
                    id: m.id.clone(),
                    key: format!("msg:{}", m.id),
                    content: m.content,
                    category: MemoryCategory::Conversation,
                    timestamp: m.created_at,
                    session_id: None,
                    score: None,
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}
