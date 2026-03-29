use anyhow::Result;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;

use nomen_core::memory::ParsedMemory;

use crate::{deserialize_option_string, MemoryRecord};

/// Version check result
#[derive(Debug, Deserialize, SurrealValue)]
struct VersionCheck {
    version: i64,
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

    MemoryRecord {
        content: parsed.content.clone(),
        memory_type: None,
        embedding: None,
        tier: nomen_core::memory::base_tier(&parsed.tier).to_string(),
        scope,
        topic: parsed.topic.clone(),
        source: parsed.source.clone(),
        model: Some(parsed.model.clone()),
        version: 1,
        nostr_id: Some(nostr_id.to_string()),
        d_tag: Some(parsed.d_tag.clone()),
        created_at: created,
        updated_at: now,
    }
}

/// Store a parsed memory into SurrealDB. Returns Ok(true) if inserted/updated, Ok(false) if skipped.
///
/// `event_id` should be the hex-encoded Nostr event ID string.
pub async fn store_memory(db: &Surreal<Db>, parsed: &ParsedMemory, event_id: &str) -> Result<bool> {
    let record = build_record(parsed, event_id);

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
         CREATE memory CONTENT $record",
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

    db.query("DELETE FROM memory WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag_owned.clone()))
        .await?
        .check()?;

    db.query(
        "CREATE memory SET \
         content = $content, type = $type, tier = $tier, scope = $scope, \
         topic = $topic, source = $source, model = $model, \
         version = $version, nostr_id = $nostr_id, d_tag = $d_tag, \
         created_at = $created_at, updated_at = $updated_at",
    )
    .bind(("type", record.memory_type))
    .bind(("content", record.content))
    .bind(("tier", record.tier))
    .bind(("scope", record.scope))
    .bind(("topic", record.topic))
    .bind(("source", record.source))
    .bind(("model", record.model))
    .bind(("version", record.version))
    .bind(("nostr_id", record.nostr_id))
    .bind(("d_tag", record.d_tag))
    .bind(("created_at", record.created_at))
    .bind(("updated_at", record.updated_at))
    .await?
    .check()?;

    Ok(d_tag_owned)
}

/// List all memories (without embeddings, for display).
pub async fn list_memories(
    db: &Surreal<Db>,
    tier: Option<&str>,
    limit: usize,
) -> Result<Vec<MemoryRecord>> {
    let (sql, bind_tier);
    // Exclude embedding from SELECT to avoid SurrealDB HNSW index deserialization issues
    let fields = "content, type, tier, scope, topic, source, model, version, nostr_id, d_tag, created_at, updated_at, last_accessed, access_count, importance";
    if let Some(t) = tier {
        sql = format!(
            "SELECT {fields} FROM memory WHERE tier = $tier ORDER BY created_at DESC LIMIT {limit}"
        );
        bind_tier = Some(t.to_string());
    } else {
        sql = format!("SELECT {fields} FROM memory ORDER BY created_at DESC LIMIT {limit}");
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

/// Get a single memory by d-tag (topic).
pub async fn get_memory_by_dtag(db: &Surreal<Db>, d_tag: &str) -> Result<Option<MemoryRecord>> {
    let mut results: Vec<MemoryRecord> = db
        .query("SELECT content, type, tier, scope, topic, source, model, version, nostr_id, d_tag, created_at, updated_at, last_accessed, access_count, importance FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(results.pop())
}

/// Get a single memory by topic field (raw topic, not d-tag).
pub async fn get_memory_by_topic(db: &Surreal<Db>, topic: &str) -> Result<Option<MemoryRecord>> {
    let mut results: Vec<MemoryRecord> = db
        .query("SELECT content, type, tier, scope, topic, source, model, version, nostr_id, d_tag, created_at, updated_at, last_accessed, access_count, importance FROM memory WHERE topic = $topic LIMIT 1")
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

/// Set the type tag on a memory record.
pub async fn set_memory_type(db: &Surreal<Db>, d_tag: &str, memory_type: &str) -> Result<()> {
    db.query("UPDATE memory SET type = $type WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .bind(("type", memory_type.to_string()))
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

/// Count memories by type (ephemeral vs named).
pub async fn count_memories_by_type(db: &Surreal<Db>) -> Result<(usize, usize, usize)> {
    #[derive(Deserialize, SurrealValue)]
    struct CountRow {
        count: usize,
    }

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

    // Unconsolidated collected messages
    let pending: Option<CountRow> = db
        .query(
            "SELECT count() AS count FROM message WHERE consolidated = false GROUP ALL",
        )
        .await?
        .check()?
        .take(0)?;

    let total_count = total.map(|r| r.count).unwrap_or(0);
    let named_count = named.map(|r| r.count).unwrap_or(total_count);
    let pending_count = pending.map(|r| r.count).unwrap_or(0);

    Ok((total_count, named_count, pending_count))
}

/// Delete old collected messages before a cutoff timestamp (unix seconds).
///
/// Only deletes messages that have already been consolidated.
pub async fn delete_collected_before(db: &Surreal<Db>, before_ts: i64) -> Result<usize> {
    #[derive(Deserialize, SurrealValue)]
    struct CountResult {
        count: usize,
    }
    let count_result: Option<CountResult> = db
        .query("SELECT count() AS count FROM message WHERE consolidated = true AND created_at < $before GROUP ALL")
        .bind(("before", before_ts))
        .await?
        .check()?
        .take(0)?;
    let count = count_result.map(|r| r.count).unwrap_or(0);
    if count > 0 {
        db.query(
            "DELETE FROM message WHERE consolidated = true AND created_at < $before",
        )
        .bind(("before", before_ts))
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
#[derive(Debug, Deserialize, Serialize, SurrealValue)]
pub struct PrunableMemory {
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub d_tag: Option<String>,
    pub topic: String,
    pub access_count: Option<i64>,
    pub created_at: String,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub last_accessed: Option<String>,
}

/// Find memories eligible for pruning based on age and access patterns.
///
/// Pruning rules:
/// - access_count = 0 AND age > max_days
pub async fn find_prunable_memories(
    db: &Surreal<Db>,
    max_days: u64,
) -> Result<Vec<PrunableMemory>> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(max_days as i64);
    let cutoff_str = cutoff.to_rfc3339();

    let sql = "SELECT d_tag, topic, access_count, created_at, last_accessed \
               FROM memory WHERE \
               (access_count = 0 AND created_at < $cutoff)";

    let results: Vec<PrunableMemory> = db
        .query(sql)
        .bind(("cutoff", cutoff_str))
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
#[derive(Debug, Serialize)]
pub struct PruneReport {
    pub memories_pruned: usize,
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

    Ok(PruneReport {
        memories_pruned,
        dry_run,
        pruned: prunable,
    })
}

/// Extract scope from d-tag (v0.3 format).
fn extract_scope(d_tag: &str) -> String {
    let (_visibility, scope) = nomen_core::memory::extract_visibility_scope(d_tag);
    scope
}
