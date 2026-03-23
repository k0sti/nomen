use anyhow::Result;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

/// Update an existing memory's embedding by d-tag.
pub async fn store_embedding(db: &Surreal<Db>, d_tag: &str, embedding: Vec<f32>) -> Result<()> {
    db.query("UPDATE memory SET embedding = $embedding, updated_at = $now WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .bind(("embedding", embedding))
        .bind(("now", chrono::Utc::now().to_rfc3339()))
        .await?
        .check()?;
    Ok(())
}
