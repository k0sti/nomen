pub use nomen_core::ingest::*;
pub use nomen_db::RawMessageRecord;

use anyhow::Result;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use tracing::debug;

/// Ingest a raw message into SurrealDB.
pub async fn ingest_message(db: &Surreal<Db>, msg: &RawMessage) -> Result<String> {
    debug!(source = %msg.source, sender = %msg.sender, "Ingesting message");
    let id = crate::db::store_raw_message(db, msg).await?;
    Ok(id)
}

/// Query raw messages with filters.
pub async fn get_messages(db: &Surreal<Db>, opts: &MessageQuery) -> Result<Vec<RawMessageRecord>> {
    crate::db::query_raw_messages(db, opts).await
}
