use anyhow::Result;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;

use nomen_core::session::ResolvedSession;

use crate::SessionRecord;

// ── Session CRUD ─────────────────────────────────────────────────────

/// Create or update a session record.
pub async fn create_session(
    db: &Surreal<Db>,
    session: &ResolvedSession,
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
