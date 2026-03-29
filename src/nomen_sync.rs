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
            let d_tag = memory::get_tag_value(&event.tags, "d").unwrap_or_default();
            let parsed = memory::parse_event(&event, signer.as_ref());
            match db::store_memory(&self.db, &parsed, &event.id.to_hex()).await {
                Ok(true) => {
                    stored += 1;
                    // Rebuild graph edges from relation tags
                    self.rebuild_edges_from_tags(&d_tag, &event.tags).await;
                }
                Ok(false) => skipped += 1,
                Err(e) => {
                    tracing::warn!("Failed to store memory {}: {e}", parsed.topic);
                    errors += 1;
                }
            }
        }

        // Sync collected messages (kind 30100)
        match relay.fetch_messages(&pubkeys).await {
            Ok(collected_events) => {
                for event in collected_events.into_iter() {
                    let d_tag_val = memory::get_tag_value(&event.tags, "d").unwrap_or_default();
                    if d_tag_val.is_empty() {
                        continue;
                    }

                    let tags: Vec<Vec<String>> = event
                        .tags
                        .iter()
                        .map(|tag| tag.as_slice().iter().map(|s| s.to_string()).collect())
                        .collect();

                    let collected = nomen_core::collected::CollectedEvent {
                        kind: nomen_core::kinds::COLLECTED_MESSAGE_KIND,
                        created_at: event.created_at.as_u64() as i64,
                        pubkey: event.pubkey.to_hex(),
                        tags,
                        content: event.content.to_string(),
                        id: Some(event.id.to_hex()),
                        sig: Some(event.sig.to_string()),
                    };

                    match nomen_db::store_collected_event(&self.db, &collected).await {
                        Ok(result) => {
                            if result.stored && !result.replaced {
                                stored += 1;
                            } else {
                                skipped += 1;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to store collected event {d_tag_val}: {e}");
                            errors += 1;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to fetch collected messages from relay: {e}");
            }
        }

        // Sync group definitions (kind 30000)
        match relay.fetch_groups(&pubkeys).await {
            Ok(group_events) => {
                for event in group_events {
                    let raw_d = memory::get_tag_value(&event.tags, "d").unwrap_or_default();
                    let group_id = raw_d.strip_prefix("nomen:group:").unwrap_or(&raw_d).to_string();
                    if group_id.is_empty() {
                        continue;
                    }
                    let name = memory::get_tag_value(&event.tags, "name")
                        .unwrap_or_else(|| group_id.clone());
                    let members: Vec<String> = event
                        .tags
                        .iter()
                        .filter_map(|tag| {
                            let s = tag.as_slice();
                            if s.first().map(|v| v.as_str()) == Some("member") {
                                s.get(1).map(|v| v.to_string())
                            } else {
                                None
                            }
                        })
                        .collect();
                    let relay_url = memory::get_tag_value(&event.tags, "relay");
                    let nostr_group = memory::get_tag_value(&event.tags, "nostr_group");

                    // Upsert: delete then create
                    let _ = db::delete_group(&self.db, &group_id).await;
                    match db::create_group(
                        &self.db,
                        &group_id,
                        &name,
                        &members,
                        nostr_group.as_deref(),
                        relay_url.as_deref(),
                    )
                    .await
                    {
                        Ok(()) => stored += 1,
                        Err(e) => {
                            tracing::warn!("Failed to sync group {group_id}: {e}");
                            errors += 1;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to fetch group events from relay: {e}");
            }
        }

        self.emit_event(
            "sync.complete",
            serde_json::json!({
                "stored": stored,
                "skipped": skipped,
                "errors": errors,
            }),
        );

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

        let texts: Vec<String> = missing.iter().map(|m| m.content.clone()).collect();

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

    /// Known relation names for graph edge reconstruction from Nostr tags.
    const KNOWN_RELATIONS: &'static [&'static str] = &[
        "mentions", "works_on", "collaborates_with", "manages", "owns",
        "member_of", "depends_on", "uses", "created", "located_in",
        "hired_by", "decided", "supports", "contradicts", "supersedes",
        "summarizes",
    ];

    /// Rebuild references edges from relation tags on a synced event.
    async fn rebuild_edges_from_tags(&self, d_tag: &str, tags: &nostr_sdk::Tags) {
        // Delete existing outgoing edges for idempotent rebuild
        let _ = db::delete_references_for(&self.db, d_tag).await;

        for tag in tags.iter() {
            let parts: Vec<&str> = tag.as_slice().iter().map(|s| s.as_str()).collect();
            if parts.len() < 2 {
                continue;
            }
            let relation = parts[0];
            let target_d_tag = parts[1];

            // source tags → consolidated_from edges (handled elsewhere)
            if relation == "source" {
                continue;
            }

            // Skip system tags
            if !Self::KNOWN_RELATIONS.contains(&relation) {
                continue;
            }

            let weight: Option<f64> = parts.get(2)
                .and_then(|w| if w.is_empty() { None } else { w.parse().ok() });
            let detail: Option<&str> = parts.get(3)
                .and_then(|d| if d.is_empty() { None } else { Some(*d) });

            if let Err(e) = db::create_references_edge(
                &self.db, d_tag, target_d_tag, relation, weight, detail,
            ).await {
                tracing::debug!("Failed to create edge {relation} → {target_d_tag}: {e}");
            }
        }
    }
}
