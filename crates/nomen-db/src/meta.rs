use anyhow::Result;
use serde::Deserialize;
use surrealdb::engine::local::Db;
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;

/// Get a meta value by key.
pub async fn get_meta(db: &Surreal<Db>, key: &str) -> Result<Option<String>> {
    #[derive(Deserialize, SurrealValue)]
    struct MetaRow {
        val: String,
    }
    let result: Option<MetaRow> = db
        .query("SELECT val FROM kv_meta WHERE key = $key LIMIT 1")
        .bind(("key", key.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(result.map(|r| r.val))
}

/// Set a meta value (upsert by key).
pub async fn set_meta(db: &Surreal<Db>, key: &str, val: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    db.query("DELETE FROM kv_meta WHERE key = $key; CREATE kv_meta CONTENT { key: $key, val: $val, updated_at: $now }")
        .bind(("key", key.to_string()))
        .bind(("val", val.to_string()))
        .bind(("now", now))
        .await?
        .check()?;
    Ok(())
}
