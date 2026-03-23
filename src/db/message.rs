use anyhow::Result;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
use surrealdb::types::{RecordId, SurrealValue};
use surrealdb::Surreal;

use crate::ingest::{MessageQuery, RawMessage, RawMessageRecord};

/// Store a raw message into SurrealDB.
pub async fn store_raw_message(db: &Surreal<Db>, msg: &RawMessage) -> Result<String> {
    let now = chrono::Utc::now().to_rfc3339();
    let created = msg.created_at.as_deref().unwrap_or(&now);
    let source_id = msg.source_id.clone().unwrap_or_default();
    let channel = msg.channel.clone().unwrap_or_default();
    // Use serde-based record creation to avoid bind serialization issues
    #[derive(Serialize, SurrealValue)]
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
        source_id,
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
            .bind((
                "id",
                RecordId::new("raw_message", id.as_str()),
            ))
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
    get_unconsolidated_messages_filtered(db, limit, None, None).await
}

/// Get unconsolidated messages with optional filters.
///
/// - `before`: Only messages created before this RFC3339 timestamp (for --older-than).
/// - `tier_filter`: Only messages matching this tier/channel pattern.
pub async fn get_unconsolidated_messages_filtered(
    db: &Surreal<Db>,
    limit: usize,
    before: Option<&str>,
    _tier_filter: Option<&str>,
) -> Result<Vec<RawMessageRecord>> {
    let mut conditions = vec!["consolidated = false".to_string()];
    if before.is_some() {
        conditions.push("created_at < $before".to_string());
    }

    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated FROM raw_message WHERE {where_clause} ORDER BY created_at ASC LIMIT {limit}"
    );

    let mut q = db.query(&sql);
    if let Some(before_val) = before {
        q = q.bind(("before", before_val.to_string()));
    }

    let results: Vec<RawMessageRecord> = q.await?.check()?.take(0)?;
    Ok(results)
}

/// Get unconsolidated messages older than a cutoff, optionally filtered to ephemeral only.
pub async fn get_ephemeral_messages_before(
    db: &Surreal<Db>,
    before: &str,
    limit: usize,
) -> Result<Vec<RawMessageRecord>> {
    let sql = format!(
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated FROM raw_message WHERE created_at < $before ORDER BY created_at ASC LIMIT {limit}"
    );
    let results: Vec<RawMessageRecord> = db
        .query(&sql)
        .bind(("before", before.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(results)
}

/// Count unconsolidated raw messages.
pub async fn count_unconsolidated_messages(db: &Surreal<Db>) -> Result<usize> {
    #[derive(Deserialize, SurrealValue)]
    struct CountRow {
        count: usize,
    }
    let result: Option<CountRow> = db
        .query("SELECT count() AS count FROM raw_message WHERE consolidated = false GROUP ALL")
        .await?
        .check()?
        .take(0)?;
    Ok(result.map(|r| r.count).unwrap_or(0))
}

/// Query messages around a specific source_id: N messages before and after.
pub async fn query_messages_around(
    db: &Surreal<Db>,
    source_id: &str,
    context_count: usize,
) -> Result<Vec<RawMessageRecord>> {
    // First, find the target message's created_at
    #[derive(Deserialize, SurrealValue)]
    struct TimeRow {
        created_at: String,
    }

    let target: Option<TimeRow> = db
        .query("SELECT created_at FROM raw_message WHERE source_id = $source_id LIMIT 1")
        .bind(("source_id", source_id.to_string()))
        .await?
        .check()?
        .take(0)?;

    let pivot_time = match target {
        Some(t) => t.created_at,
        None => anyhow::bail!("Message with source_id '{source_id}' not found"),
    };

    // Fetch N messages before (inclusive of target) + N messages after
    let before_sql = format!(
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated \
         FROM raw_message WHERE created_at <= $pivot ORDER BY created_at DESC LIMIT {}",
        context_count + 1
    );
    let after_sql = format!(
        "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, channel ?? '' AS channel, content, created_at, consolidated \
         FROM raw_message WHERE created_at > $pivot ORDER BY created_at ASC LIMIT {context_count}"
    );

    let combined_sql = format!("{before_sql}; {after_sql}");
    let mut result = db
        .query(&combined_sql)
        .bind(("pivot", pivot_time))
        .await?
        .check()?;

    let mut before: Vec<RawMessageRecord> = result.take(0)?;
    let after: Vec<RawMessageRecord> = result.take(1)?;

    // Before is in DESC order, reverse it
    before.reverse();
    before.extend(after);
    Ok(before)
}

/// Count consolidated raw messages older than the given cutoff date (RFC3339).
pub async fn count_old_messages(db: &Surreal<Db>, before: &str) -> Result<usize> {
    #[derive(Deserialize, SurrealValue)]
    struct CountResult {
        count: usize,
    }
    let result: Option<CountResult> = db
        .query("SELECT count() AS count FROM raw_message WHERE consolidated = true AND created_at < $before GROUP ALL")
        .bind(("before", before.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(result.map(|r| r.count).unwrap_or(0))
}

/// Prune (delete) consolidated raw messages older than the given cutoff date (RFC3339).
pub async fn prune_old_messages(db: &Surreal<Db>, before: &str) -> Result<usize> {
    let count = count_old_messages(db, before).await?;
    if count > 0 {
        db.query("DELETE FROM raw_message WHERE consolidated = true AND created_at < $before")
            .bind(("before", before.to_string()))
            .await?
            .check()?;
    }
    Ok(count)
}

// ── Session CRUD ─────────────────────────────────────────────────────

use crate::session::SessionRecord;

/// Create or update a session record.
pub async fn create_session(
    db: &Surreal<Db>,
    session: &crate::session::ResolvedSession,
) -> Result<String> {
    let now = chrono::Utc::now().to_rfc3339();

    #[derive(Serialize, SurrealValue)]
    struct NewSession {
        session_id: String,
        tier: String,
        scope: String,
        channel: String,
        group_id: String,
        participants: Vec<String>,
        created_at: String,
        last_active: String,
    }

    let record = NewSession {
        session_id: session.session_id.clone(),
        tier: session.tier.clone(),
        scope: session.scope.clone(),
        channel: session.channel.clone(),
        group_id: session.group_id.clone(),
        participants: session.participants.clone(),
        created_at: now.clone(),
        last_active: now,
    };

    // Upsert: delete existing then create
    db.query("DELETE FROM session WHERE session_id = $sid; CREATE session CONTENT $record")
        .bind(("sid", session.session_id.clone()))
        .bind(("record", record))
        .await?
        .check()?;

    Ok(session.session_id.clone())
}

/// Get a session by session_id.
pub async fn get_session(db: &Surreal<Db>, session_id: &str) -> Result<Option<SessionRecord>> {
    let result: Option<SessionRecord> = db
        .query("SELECT meta::id(id) AS id, session_id, tier, scope, channel, group_id, participants, created_at, last_active FROM session WHERE session_id = $sid LIMIT 1")
        .bind(("sid", session_id.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(result)
}

/// Update the last_active timestamp of a session.
pub async fn update_session_last_active(db: &Surreal<Db>, session_id: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    db.query("UPDATE session SET last_active = $now WHERE session_id = $sid")
        .bind(("sid", session_id.to_string()))
        .bind(("now", now))
        .await?
        .check()?;
    Ok(())
}

/// List all sessions, ordered by last_active descending.
pub async fn list_sessions(db: &Surreal<Db>) -> Result<Vec<SessionRecord>> {
    let results: Vec<SessionRecord> = db
        .query("SELECT meta::id(id) AS id, session_id, tier, scope, channel, group_id, participants, created_at, last_active FROM session ORDER BY last_active DESC")
        .await?
        .check()?
        .take(0)?;
    Ok(results)
}

// ── Consolidation Sessions ────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone, SurrealValue)]
pub struct ConsolidationSessionRecord {
    pub id: Option<String>,
    pub session_id: String,
    pub status: String,
    pub created_at: String,
    pub expires_at: String,
    pub batches: Option<serde_json::Value>,
    pub batch_count: i64,
    pub message_count: i64,
}

pub async fn create_consolidation_session(
    db: &Surreal<Db>,
    session_id: &str,
    batches: &serde_json::Value,
    batch_count: usize,
    message_count: usize,
    ttl_minutes: u32,
) -> Result<()> {
    let now = chrono::Utc::now();
    let expires = now + chrono::Duration::minutes(ttl_minutes as i64);
    db.query(
        "CREATE consolidation_session SET \
         session_id = $sid, status = 'pending', \
         created_at = $now, expires_at = $exp, \
         batches = $batches, batch_count = $bc, message_count = $mc",
    )
    .bind(("sid", session_id.to_string()))
    .bind(("now", now.to_rfc3339()))
    .bind(("exp", expires.to_rfc3339()))
    .bind(("batches", batches.clone()))
    .bind(("bc", batch_count as i64))
    .bind(("mc", message_count as i64))
    .await?
    .check()?;
    Ok(())
}

pub async fn get_consolidation_session(
    db: &Surreal<Db>,
    session_id: &str,
) -> Result<Option<ConsolidationSessionRecord>> {
    let mut results: Vec<ConsolidationSessionRecord> = db
        .query(
            "SELECT meta::id(id) AS id, session_id, status, created_at, expires_at, \
             batches, batch_count, message_count \
             FROM consolidation_session WHERE session_id = $sid LIMIT 1",
        )
        .bind(("sid", session_id.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(results.pop())
}

pub async fn update_consolidation_session_status(
    db: &Surreal<Db>,
    session_id: &str,
    status: &str,
) -> Result<()> {
    db.query("UPDATE consolidation_session SET status = $status WHERE session_id = $sid")
        .bind(("sid", session_id.to_string()))
        .bind(("status", status.to_string()))
        .await?
        .check()?;
    Ok(())
}

pub async fn cleanup_expired_consolidation_sessions(db: &Surreal<Db>) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let results: Vec<ConsolidationSessionRecord> = db
        .query(
            "SELECT meta::id(id) AS id, session_id, status, created_at, expires_at, \
             batches, batch_count, message_count \
             FROM consolidation_session WHERE status = 'pending' AND expires_at < $now",
        )
        .bind(("now", now.clone()))
        .await?
        .check()?
        .take(0)?;
    let count = results.len();
    if count > 0 {
        db.query("UPDATE consolidation_session SET status = 'expired' WHERE status = 'pending' AND expires_at < $now")
            .bind(("now", now.clone()))
            .await?
            .check()?;
    }
    Ok(count)
}
