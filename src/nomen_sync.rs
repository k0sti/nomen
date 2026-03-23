//! impl Nomen — relay sync, embedding generation.

use anyhow::Result;

use crate::db;
use crate::memory;
use crate::{EmbedReport, Nomen, SyncReport};

impl Nomen {
    /// Sync memories from relay to local DB.
    pub async fn sync(&self) -> Result<SyncReport> {
        let relay = self
            .relay
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No relay configured for sync"))?;
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No signer configured for sync"))?;

        let pubkeys = vec![signer.public_key()];
        let events = relay.fetch_memories(&pubkeys).await?;

        let mut stored = 0usize;
        let mut skipped = 0usize;
        let mut errors = 0usize;

        for event in events.into_iter() {
            if event.kind == nostr_sdk::Kind::Custom(crate::kinds::LESSON_KIND)
                || event.kind == nostr_sdk::Kind::Custom(crate::kinds::LEGACY_LESSON_KIND)
            {
                continue;
            }
            let d_tag = memory::get_tag_value(&event.tags, "d").unwrap_or_default();
            if d_tag.starts_with("snowclaw:config:") {
                continue;
            }

            let parsed = memory::parse_event(&event, signer.as_ref());
            match db::store_memory(&self.db, &parsed, &event.id.to_hex()).await {
                Ok(true) => stored += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    tracing::warn!("Failed to store memory {}: {e}", parsed.topic);
                    errors += 1;
                }
            }
        }

        self.emit_event("sync.complete", serde_json::json!({
            "stored": stored,
            "skipped": skipped,
            "errors": errors,
        }));

        Ok(SyncReport {
            stored,
            skipped,
            errors,
        })
    }

    /// Generate embeddings for memories that lack them.
    pub async fn embed(&self, limit: usize) -> Result<EmbedReport> {
        if self.embedder.dimensions() == 0 {
            anyhow::bail!("No embedding provider configured");
        }

        let missing = db::get_memories_without_embeddings(&self.db, limit).await?;
        let total = missing.len();

        if missing.is_empty() {
            return Ok(EmbedReport {
                embedded: 0,
                total: 0,
            });
        }

        let texts: Vec<String> = missing
            .iter()
            .map(|m| m.content.clone())
            .collect();

        let embeddings = self.embedder.embed(&texts).await?;
        let mut embedded = 0usize;

        for (row, embedding) in missing.iter().zip(embeddings.into_iter()) {
            if let Some(ref d_tag) = row.d_tag {
                db::store_embedding(&self.db, d_tag, embedding).await?;
                embedded += 1;
            }
        }

        Ok(EmbedReport { embedded, total })
    }
}
