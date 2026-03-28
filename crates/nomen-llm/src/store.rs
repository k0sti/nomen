//! Direct memory store helpers — extracted from Nomen::store_direct.

use anyhow::Result;
use nomen_core::embed::Embedder;
use nomen_core::memory;
use nomen_core::NewMemory;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

/// Store a new memory directly (without relay event).
pub async fn store_direct(
    db: &Surreal<Db>,
    embedder: &dyn Embedder,
    mem: NewMemory,
) -> Result<String> {
    store_direct_with_author(db, embedder, mem, "").await
}

/// Store a new memory with explicit author pubkey for d-tag construction.
pub async fn store_direct_with_author(
    db: &Surreal<Db>,
    embedder: &dyn Embedder,
    mem: NewMemory,
    author_pubkey_hex: &str,
) -> Result<String> {
    let d_tag = memory::build_dtag_from_tier(&mem.tier, author_pubkey_hex, &mem.topic);
    let source = mem.source.as_deref().unwrap_or("api");
    let model = mem.model.as_deref().unwrap_or("nomen/api");

    let visibility = memory::base_tier(&mem.tier).to_string();
    let parsed = memory::ParsedMemory {
        tier: mem.tier,
        visibility,
        topic: mem.topic,
        model: model.to_string(),
        content: mem.content.clone(),
        created_at: nostr_sdk::Timestamp::now(),
        d_tag: d_tag.clone(),
        source: source.to_string(),
        importance: mem.importance,
    };

    nomen_db::store_memory_direct(db, &parsed, source).await?;

    // Set type if provided
    if let Some(ref t) = mem.memory_type {
        let _ = nomen_db::set_memory_type(db, &d_tag, t).await;
    }

    // Set importance if provided
    if let Some(imp) = mem.importance {
        let _ = nomen_db::set_importance(db, &d_tag, imp).await;
    }

    // Generate embedding if embedder is configured
    if embedder.dimensions() > 0 {
        if let Ok(embeddings) = embedder.embed(&[mem.content]).await {
            if let Some(embedding) = embeddings.into_iter().next() {
                let _ = nomen_db::store_embedding(db, &d_tag, embedding).await;
            }
        }
    }

    Ok(d_tag)
}
