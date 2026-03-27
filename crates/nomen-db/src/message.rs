use anyhow::Result;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;

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
