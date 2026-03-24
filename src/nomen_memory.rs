//! impl Nomen — memory store, get, delete, list, search operations.

use anyhow::Result;

use crate::db;
use crate::embed::Embedder;
use crate::memory;
use crate::search::{SearchOptions, SearchResult};
use crate::{ListOptions, ListReport, ListStats, NewMemory, Nomen};

impl Nomen {
    /// Hybrid (vector + full-text) search over stored memories.
    pub async fn search(&self, opts: SearchOptions) -> Result<Vec<SearchResult>> {
        crate::search::search(&self.db, self.embedder.as_ref(), &opts).await
    }

    /// Store a new memory into SurrealDB and optionally publish to relay.
    ///
    /// Includes supersedes logic: if a memory with the same topic already exists
    /// in the local DB, the new event will carry a `supersedes` tag.
    /// For personal/internal tier, content is NIP-44 encrypted before relay publish.
    /// Content is always plain text (not JSON).
    pub async fn store(&self, mem: NewMemory) -> Result<String> {
        let author_pubkey_hex = self
            .signer
            .as_ref()
            .map(|s| s.public_key().to_hex())
            .unwrap_or_default();

        let base_tier = memory::base_tier(&mem.tier);
        let d_tag = memory::build_dtag_from_tier(&mem.tier, &author_pubkey_hex, &mem.topic);
        let (_vis, scope) = memory::extract_visibility_scope(&d_tag);

        let source = mem.source.as_deref().unwrap_or("api");
        let model = mem.model.as_deref().unwrap_or("nomen/api");
        let content_str = mem.content.clone();

        // Supersedes logic: check for existing memory with same topic
        let previous_nostr_id =
            match db::get_memory_by_topic(&self.db, &mem.topic).await? {
                Some(existing) => existing.nostr_id,
                None => {
                    match db::get_memory_by_dtag(&self.db, &d_tag).await? {
                        Some(existing) => existing.nostr_id,
                        None => None,
                    }
                }
            };

        let parsed = memory::ParsedMemory {
            tier: mem.tier.clone(),
            visibility: base_tier.to_string(),
            topic: mem.topic.clone(),
            model: model.to_string(),
            content: content_str.clone(),
            created_at: nostr_sdk::Timestamp::now(),
            d_tag: d_tag.clone(),
            source: source.to_string(),
            importance: mem.importance,
        };

        db::store_memory_direct(&self.db, &parsed, source).await?;

        // Set importance if provided
        if let Some(imp) = mem.importance {
            let _ = db::set_importance(&self.db, &d_tag, imp).await;
        }

        // Generate embedding if embedder is configured
        if self.embedder.dimensions() > 0 {
            if let Ok(embeddings) = self.embedder.embed(&[content_str.clone()]).await {
                if let Some(embedding) = embeddings.into_iter().next() {
                    let _ = db::store_embedding(&self.db, &d_tag, embedding).await;
                }
            }
        }

        // Publish to relay if available
        if let Some(ref relay) = self.relay {
            // NIP-44 encrypt for personal/private tier
            let final_content = if base_tier == "personal" || base_tier == "private" {
                relay.signer().encrypt(&content_str).unwrap_or(content_str)
            } else {
                content_str
            };

            let mut tags = vec![
                nostr_sdk::Tag::custom(nostr_sdk::TagKind::Custom("d".into()), vec![d_tag.clone()]),
                nostr_sdk::Tag::custom(
                    nostr_sdk::TagKind::Custom("visibility".into()),
                    vec![base_tier.to_string()],
                ),
                nostr_sdk::Tag::custom(
                    nostr_sdk::TagKind::Custom("scope".into()),
                    vec![scope.clone()],
                ),
                nostr_sdk::Tag::custom(
                    nostr_sdk::TagKind::Custom("model".into()),
                    vec![model.to_string()],
                ),
            ];

            // Add importance tag if set
            if let Some(imp) = mem.importance {
                tags.push(nostr_sdk::Tag::custom(
                    nostr_sdk::TagKind::Custom("importance".into()),
                    vec![imp.to_string()],
                ));
            }

            // Add supersedes tag if updating an existing memory
            if let Some(ref prev_id) = previous_nostr_id {
                if !prev_id.is_empty() {
                    tags.push(nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom("supersedes".into()),
                        vec![prev_id.clone()],
                    ));
                }
            }

            // Add topic tags for relay-side filtering
            for part in mem.topic.split('/') {
                if !part.is_empty() {
                    tags.push(nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom("t".into()),
                        vec![part.to_string()],
                    ));
                }
            }

            // Add h tag for group tier (NIP-29)
            if let Some(group_id) = mem.tier.strip_prefix("group:") {
                tags.push(nostr_sdk::Tag::custom(
                    nostr_sdk::TagKind::Custom("h".into()),
                    vec![group_id.to_string()],
                ));
            }

            let builder = nostr_sdk::EventBuilder::new(
                nostr_sdk::Kind::Custom(crate::kinds::MEMORY_KIND),
                final_content,
            )
            .tags(tags);

            if let Err(e) = relay.publish(builder).await {
                tracing::warn!("Failed to publish memory to relay: {e}");
            }
        }

        self.emit_event("memory.updated", serde_json::json!({
            "topic": mem.topic,
            "visibility": base_tier,
            "scope": scope,
            "author": author_pubkey_hex,
            "source": source,
        }));

        Ok(d_tag)
    }

    /// Store a new memory using explicit db/embedder handles.
    pub async fn store_direct(
        db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
        embedder: &dyn Embedder,
        mem: NewMemory,
    ) -> Result<String> {
        nomen_llm::store::store_direct(db, embedder, mem).await
    }

    /// Store a new memory with explicit author pubkey for d-tag construction.
    pub async fn store_direct_with_author(
        db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
        embedder: &dyn Embedder,
        mem: NewMemory,
        author_pubkey_hex: &str,
    ) -> Result<String> {
        nomen_llm::store::store_direct_with_author(db, embedder, mem, author_pubkey_hex).await
    }

    /// Delete a memory by topic or event ID.
    ///
    /// If a relay is connected, also publishes a NIP-09 deletion event (kind 5).
    pub async fn delete(&self, topic: Option<&str>, id: Option<&str>) -> Result<()> {
        let mut nostr_event_id: Option<String> = None;

        if let Some(topic) = topic {
            if let Ok(Some(record)) = db::get_memory_by_topic(&self.db, topic).await {
                nostr_event_id = record.nostr_id.clone();
            }
            if nostr_event_id.is_none() {
                if let Ok(Some(record)) = db::get_memory_by_dtag(&self.db, topic).await {
                    nostr_event_id = record.nostr_id.clone();
                }
            }
            db::delete_memory_by_dtag(&self.db, topic).await?;
            db::delete_memory_by_topic(&self.db, topic).await?;
        }
        if let Some(id) = id {
            nostr_event_id = Some(id.to_string());
            db::delete_memory_by_nostr_id(&self.db, id).await?;
        }

        // Publish NIP-09 deletion to relay if available
        if let (Some(ref relay), Some(ref event_id_hex)) = (&self.relay, &nostr_event_id) {
            if !event_id_hex.is_empty() {
                if let Ok(eid) = nostr_sdk::EventId::from_hex(event_id_hex) {
                    let delete_builder =
                        nostr_sdk::EventBuilder::new(nostr_sdk::Kind::Custom(5), "")
                            .tags(vec![nostr_sdk::Tag::event(eid)]);

                    if let Err(e) = relay.publish(delete_builder).await {
                        tracing::warn!("Failed to publish NIP-09 deletion to relay: {e}");
                    }
                }
            }
        }

        let deleted_topic = topic.or(id).unwrap_or_default();
        self.emit_event("memory.deleted", serde_json::json!({
            "topic": deleted_topic,
            "d_tag": topic.unwrap_or_default(),
            "author": self.signer.as_ref().map(|s| s.public_key().to_hex()).unwrap_or_default(),
        }));

        Ok(())
    }

    /// List memories, optionally filtered by tier.
    pub async fn list_memories(
        &self,
        tier: Option<&str>,
        limit: usize,
    ) -> Result<Vec<db::MemoryRecord>> {
        db::list_memories(&self.db, tier, limit).await
    }

    /// Count memories: returns (total, named, pending_messages).
    pub async fn count_memories(&self) -> Result<(usize, usize, usize)> {
        db::count_memories_by_type(&self.db).await
    }

    /// Get a memory by its d_tag.
    pub async fn get_by_topic(&self, d_tag: &str) -> Result<Option<db::MemoryRecord>> {
        db::get_memory_by_dtag(&self.db, d_tag).await
    }

    /// Get a memory by raw topic string (queries the `topic` field, not `d_tag`).
    pub async fn get_by_raw_topic(&self, topic: &str) -> Result<Option<db::MemoryRecord>> {
        db::get_memory_by_topic(&self.db, topic).await
    }

    /// Delete a memory by raw topic string.
    pub async fn delete_by_topic(&self, topic: &str) -> Result<()> {
        db::delete_memory_by_topic(&self.db, topic).await
    }

    /// List memories from local DB with optional filters.
    pub async fn list(&self, opts: ListOptions) -> Result<ListReport> {
        let memories = db::list_memories(&self.db, opts.tier.as_deref(), opts.limit).await?;
        let stats = if opts.include_stats {
            let (total, named, pending) = db::count_memories_by_type(&self.db).await?;
            Some(ListStats {
                total,
                named,
                pending,
            })
        } else {
            None
        };

        Ok(ListReport { memories, stats })
    }

    /// Delete consolidated messages older than a duration string (e.g. "7d", "24h").
    ///
    /// Only deletes messages that have already been consolidated.
    /// Unconsolidated messages are preserved regardless of age.
    pub async fn delete_old_messages(&self, older_than: &str) -> Result<usize> {
        let secs = crate::consolidate::parse_duration_str(older_than)?;
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(secs);
        let cutoff_ts = cutoff.timestamp();
        db::delete_collected_before(&self.db, cutoff_ts).await
    }
}
