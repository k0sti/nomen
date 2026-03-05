//! SQLite → SurrealDB migration for Snowclaw memory databases.
//!
//! Reads memories from Snowclaw's `memories.db` SQLite file and imports
//! them into Nomen's SurrealDB store.
//!
//! Enable with the `migrate` feature:
//! ```toml
//! nomen = { path = "..", features = ["migrate"] }
//! ```

use anyhow::{Context, Result};
use rusqlite::Connection;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use tracing::{debug, info, warn};

use crate::db;
use crate::memory::ParsedMemory;

/// A row from Snowclaw's SQLite `memories` table.
#[derive(Debug)]
struct SqliteMemory {
    topic: String,
    summary: String,
    detail: String,
    tier: String,
    confidence: f64,
    model: String,
    source: String,
    created_at: String,
}

/// Report from a migration run.
#[derive(Debug, Default)]
pub struct MigrationReport {
    pub total_read: usize,
    pub imported: usize,
    pub skipped: usize,
    pub errors: usize,
}

/// Migrate memories from a Snowclaw SQLite database into Nomen SurrealDB.
///
/// The SQLite database is expected to have a `memories` table with columns:
/// `topic`, `summary`, `detail`, `tier`, `confidence`, `model`, `source`, `created_at`.
///
/// Memories are imported with d-tag `snow:memory:<topic>` and will be
/// upserted (existing memories with the same d-tag are overwritten).
pub async fn migrate_from_sqlite(
    sqlite_path: &str,
    db: &Surreal<Db>,
) -> Result<MigrationReport> {
    let conn = Connection::open(sqlite_path)
        .with_context(|| format!("Failed to open SQLite database: {sqlite_path}"))?;

    info!("Opened SQLite database: {sqlite_path}");

    let mut stmt = conn
        .prepare(
            "SELECT topic, summary, detail, tier, confidence, model, source, created_at \
             FROM memories ORDER BY created_at ASC",
        )
        .context("Failed to prepare SELECT on memories table")?;

    let rows = stmt
        .query_map([], |row| {
            Ok(SqliteMemory {
                topic: row.get(0)?,
                summary: row.get(1)?,
                detail: row.get(2)?,
                tier: row.get::<_, String>(3).unwrap_or_else(|_| "public".to_string()),
                confidence: row.get::<_, f64>(4).unwrap_or(0.8),
                model: row.get::<_, String>(5).unwrap_or_else(|_| "unknown".to_string()),
                source: row.get::<_, String>(6).unwrap_or_else(|_| "sqlite-import".to_string()),
                created_at: row.get::<_, String>(7).unwrap_or_default(),
            })
        })
        .context("Failed to query memories")?;

    let mut report = MigrationReport::default();

    for row_result in rows {
        report.total_read += 1;

        let mem = match row_result {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to read SQLite row: {e}");
                report.errors += 1;
                continue;
            }
        };

        let d_tag = format!("snow:memory:{}", mem.topic);
        let content = serde_json::json!({
            "summary": mem.summary,
            "detail": mem.detail,
        });

        let created_ts = chrono::DateTime::parse_from_rfc3339(&mem.created_at)
            .map(|dt| nostr_sdk::Timestamp::from(dt.timestamp() as u64))
            .unwrap_or_else(|_| nostr_sdk::Timestamp::now());

        let parsed = ParsedMemory {
            tier: mem.tier,
            topic: mem.topic.clone(),
            version: "1".to_string(),
            confidence: format!("{:.2}", mem.confidence),
            model: mem.model,
            summary: mem.summary.clone(),
            created_at: created_ts,
            d_tag,
            source: mem.source,
            content_raw: content.to_string(),
            detail: mem.detail,
        };

        match db::store_memory_direct(db, &parsed, "sqlite-import").await {
            Ok(_) => {
                debug!("Imported memory: {}", mem.topic);
                report.imported += 1;
            }
            Err(e) => {
                warn!("Failed to import memory {}: {e}", mem.topic);
                report.errors += 1;
            }
        }
    }

    info!(
        "Migration complete: {} read, {} imported, {} skipped, {} errors",
        report.total_read, report.imported, report.skipped, report.errors
    );

    Ok(report)
}
