use anyhow::Result;
use nostr_sdk::prelude::*;
use serde::Deserialize;
use surrealdb::engine::local::Db;
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;
use tracing::{debug, info, warn};

use nomen_core::embed::Embedder;
use nomen_db::RawMessageRecord;
use nomen_relay::RelayManager;

use super::grouping::{derive_tier_from_messages, enforce_tier_guard, group_messages};
use super::types::{
    ConsolidationConfig, ConsolidationReport, ConsolidationStatus, ExistingMemory, GroupSummary,
};

pub(crate) const META_KEY_LAST_CONSOLIDATION: &str = "last_consolidation_run";

/// Run the consolidation pipeline.
/// 2. Group by sender/conversation-container identity + 4-hour time windows
/// 3. Send each group to LLM provider for summarization
/// 4. Store consolidated memories with provenance tags
/// 5. Mark raw messages as consolidated
/// 6. Create consolidated_from edges
/// 7. Publish NIP-09 deletion events for consumed ephemerals
pub async fn consolidate(
    db: &Surreal<Db>,
    embedder: &dyn Embedder,
    config: &ConsolidationConfig,
    relay: Option<&RelayManager>,
) -> Result<ConsolidationReport> {
    // Build cutoff time from older_than
    let cutoff = if let Some(ref duration_str) = config.older_than {
        let secs = super::parse_duration_str(duration_str)?;
        let cutoff_time = chrono::Utc::now() - chrono::Duration::seconds(secs);
        Some(cutoff_time.to_rfc3339())
    } else {
        None
    };

    // Fetch unconsolidated collected messages (kind 30100)
    let cutoff_ts = cutoff.as_deref().and_then(|c| {
        chrono::DateTime::parse_from_rfc3339(c)
            .ok()
            .map(|dt| dt.timestamp())
    });
    let collected = nomen_db::get_unconsolidated_collected(
        db,
        config.batch_size,
        cutoff_ts,
    )
    .await?;

    // Convert to RawMessageRecord for pipeline compatibility
    let messages: Vec<RawMessageRecord> = collected
        .iter()
        .map(|cm| cm.to_raw_message_record())
        .collect();

    if messages.len() < config.min_messages {
        info!(
            count = messages.len(),
            min = config.min_messages,
            "Not enough unconsolidated messages to consolidate"
        );
        return Ok(ConsolidationReport {
            dry_run: config.dry_run,
            ..Default::default()
        });
    }

    debug!(count = messages.len(), "Processing unconsolidated messages");

    // Group messages by sender/container identity + time window + scope.
    // Scope partitioning ensures messages from different groups/tiers
    // are never consolidated together (cross-group guard).
    // Current compatibility layer still derives container identity from
    // legacy raw-message `channel` fields.
    let grouped = group_messages(messages.clone());
    debug!(
        groups = grouped.len(),
        "Grouped messages into time windows (scope-partitioned)"
    );

    let mut report = ConsolidationReport {
        messages_processed: messages.len(),
        dry_run: config.dry_run,
        ..Default::default()
    };

    let now_timestamp = chrono::Utc::now().timestamp();
    let mut all_consumed_msg_ids: Vec<String> = Vec::new();

    for (key, group_msgs) in &grouped {
        if group_msgs.len() < config.min_messages {
            debug!(
                key = %key.identity,
                scope = %key.scope,
                count = group_msgs.len(),
                "Skipping group with too few messages"
            );
            continue;
        }

        let extracted = config.llm_provider.consolidate(group_msgs).await?;

        // Derive tier from source messages
        let derived_tier = derive_tier_from_messages(group_msgs);
        // Apply cross-group consolidation guard (scope partitioning in GroupKey + tier guard)
        let most_restrictive_source = if group_msgs.iter().any(|m| {
            let container = super::grouping::primary_container_id(m);
            m.source == "dm"
                || m.source == "telegram_dm"
                || (m.source == "nostr" && (container.is_empty() || container == "dm"))
        }) {
            "personal"
        } else if group_msgs
            .iter()
            .any(|m| {
                let container = super::grouping::primary_container_id(m);
                !container.is_empty() && container != "dm" && container != "general"
            })
        {
            "group"
        } else {
            "public"
        };
        let tier = enforce_tier_guard(&derived_tier, most_restrictive_source);

        for memory in &extracted {
            let group_summary = GroupSummary {
                key: format!("{}:{}", key.identity, key.window),
                message_count: group_msgs.len(),
                topic: memory.topic.clone(),
            };
            report.groups.push(group_summary);

            if config.dry_run {
                report.memories_created += 1;
                continue;
            }

            // Build v0.2 d-tag: {visibility}:{context}:{topic}
            let author_hex = config.author_pubkey.as_deref().unwrap_or("");
            let d_tag = nomen_core::memory::build_dtag_from_tier(&tier, author_hex, &memory.topic);

            // Check if a memory with this d-tag already exists (for merge)
            let existing = get_existing_memory(db, &d_tag).await;

            let (
                final_content,
                final_importance,
                contradicts,
                is_merge,
            ) = if let Ok(Some(existing_mem)) = existing {
                // Merge: re-prompt LLM with existing + new
                debug!(topic = %memory.topic, "Merging into existing memory");

                match config
                    .llm_provider
                    .merge(&existing_mem.content, group_msgs)
                    .await
                {
                    Ok(merged) if !merged.is_empty() => {
                        let m = &merged[0];
                        (
                            m.content.clone(),
                            m.importance,
                            m.contradicts_existing,
                            true,
                        )
                    }
                    Ok(_) => {
                        // Merge returned empty, use extracted as-is
                        (
                            memory.content.clone(),
                            memory.importance,
                            false,
                            true,
                        )
                    }
                    Err(e) => {
                        warn!("LLM merge failed, using extracted memory: {e}");
                        (
                            memory.content.clone(),
                            memory.importance,
                            false,
                            true,
                        )
                    }
                }
            } else {
                // No existing memory — check for near-duplicates via embedding (TODO #6)
                let mut is_dedup_merge = false;
                if embedder.dimensions() > 0 {
                    if let Ok(emb) = embedder.embed_one(&memory.content).await {
                        if let Ok(similar) = find_similar_memory(db, &emb, 0.92).await {
                            if let Some(sim_dtag) = similar {
                                debug!(
                                    topic = %memory.topic,
                                    similar_dtag = %sim_dtag,
                                    "Found near-duplicate memory, merging"
                                );
                                // Fetch the similar memory and merge
                                if let Ok(Some(sim_mem)) = get_existing_memory(db, &sim_dtag).await
                                {
                                    match config
                                        .llm_provider
                                        .merge(&sim_mem.content, group_msgs)
                                        .await
                                    {
                                        Ok(merged) if !merged.is_empty() => {
                                            let m = &merged[0];
                                            is_dedup_merge = true;
                                            // Store using the similar memory's d_tag
                                            let mem = nomen_core::NewMemory {
                                                topic: sim_dtag.clone(),
                                                content: m.content.clone(),
                                                tier: tier.clone(),
                                                importance: m.importance,
                                                source: Some("consolidation".to_string()),
                                                model: Some("nomen/consolidation".to_string()),
                                            };
                                            let stored_dtag =
                                                crate::store::store_direct(db, embedder, mem)
                                                    .await?;
                                            // Bump version
                                            bump_memory_version(db, &stored_dtag).await.ok();
                                            nomen_db::set_consolidation_tags(
                                                db,
                                                &stored_dtag,
                                                &group_msgs.len().to_string(),
                                                &now_timestamp.to_string(),
                                            )
                                            .await
                                            .ok();
                                            report.memories_updated += 1;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }

                if is_dedup_merge {
                    // Track primary conversation container for reporting.
                    let container = group_msgs
                        .first()
                        .map(|m| {
                            if !m.thread_id.is_empty() {
                                let chat = if m.chat_id.is_empty() { &m.channel } else { &m.chat_id };
                                if chat.is_empty() { m.thread_id.clone() } else { format!("{chat}/{}", m.thread_id) }
                            } else if !m.chat_id.is_empty() {
                                m.chat_id.clone()
                            } else if !m.channel.is_empty() {
                                m.channel.clone()
                            } else {
                                "general".to_string()
                            }
                        })
                        .unwrap_or_else(|| "general".to_string());
                    if !container.is_empty() && !report.channels.contains(&container) {
                        report.channels.push(container);
                    }
                    continue;
                }

                (
                    memory.content.clone(),
                    memory.importance,
                    false,
                    false,
                )
            };

            let content_for_entities = final_content.clone();

            let mem = nomen_core::NewMemory {
                topic: d_tag.clone(),
                content: final_content,
                tier: tier.clone(),
                importance: final_importance,
                source: Some("consolidation".to_string()),
                model: Some("nomen/consolidation".to_string()),
            };

            // Build extra tags for provenance
            let consolidated_from_count = group_msgs.len().to_string();
            let consolidated_at = now_timestamp.to_string();

            let d_tag = crate::store::store_direct(db, embedder, mem).await?;

            if is_merge {
                // Bump version for merged memories (TODO #2)
                bump_memory_version(db, &d_tag).await.ok();
                report.memories_updated += 1;
            } else {
                report.memories_created += 1;
            }

            // Update the memory record with consolidation tags
            nomen_db::set_consolidation_tags(
                db,
                &d_tag,
                &consolidated_from_count,
                &consolidated_at,
            )
            .await
            .ok();

            // Store importance score
            if let Some(imp) = final_importance {
                nomen_db::set_importance(db, &d_tag, imp).await.ok();
            }

            // Handle conflict detection: create contradicts edge
            if contradicts && is_merge {
                let existing_d_tag = memory.topic.clone();
                if let Err(e) =
                    nomen_db::create_references_edge(db, &d_tag, &existing_d_tag, "contradicts")
                        .await
                {
                    warn!("Failed to create contradicts edge: {e}");
                } else {
                    debug!(topic = %memory.topic, "Created contradicts edge for conflicting merge");
                }
            }

            // Create consolidated_from edges
            if let Ok(record_id) = get_memory_record_id(db, &d_tag).await {
                for msg in group_msgs {
                    if let Err(e) =
                        nomen_db::create_consolidated_edge(db, &record_id, &msg.id).await
                    {
                        warn!("Failed to create consolidated_from edge: {e}");
                    }
                }
            }

            // Entity extraction from consolidated memory (trait-based)
            {
                let entity_text = content_for_entities.clone();
                match config.entity_extractor.extract(&entity_text, &[]).await {
                    Ok((extracted_entities, extracted_relationships)) => {
                        if let Ok(record_id) = get_memory_record_id(db, &d_tag).await {
                            // Store entities and create mention edges
                            for entity in &extracted_entities {
                                match nomen_db::store_entity(db, &entity.name, &entity.kind).await
                                {
                                    Ok(entity_id) => {
                                        let eid = entity_id
                                            .split_once(':')
                                            .map(|(_, id)| id)
                                            .unwrap_or(&entity_id);
                                        let mid = record_id
                                            .split_once(':')
                                            .map(|(_, id)| id)
                                            .unwrap_or(&record_id);
                                        if let Err(e) = nomen_db::create_mention_edge(
                                            db,
                                            mid,
                                            eid,
                                            entity.relevance,
                                        )
                                        .await
                                        {
                                            warn!(
                                                "Failed to create mention edge for entity '{}': {e}",
                                                entity.name
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to store entity '{}': {e}", entity.name);
                                    }
                                }
                            }

                            // Store typed relationships between entities
                            for rel in &extracted_relationships {
                                // Ensure both entities exist first
                                let from_id = nomen_db::store_entity(
                                    db,
                                    &rel.from,
                                    &nomen_core::entities::EntityKind::Concept,
                                )
                                .await;
                                let to_id = nomen_db::store_entity(
                                    db,
                                    &rel.to,
                                    &nomen_core::entities::EntityKind::Concept,
                                )
                                .await;

                                if let (Ok(from_id), Ok(to_id)) = (from_id, to_id) {
                                    let fid = from_id
                                        .split_once(':')
                                        .map(|(_, id)| id)
                                        .unwrap_or(&from_id);
                                    let tid =
                                        to_id.split_once(':').map(|(_, id)| id).unwrap_or(&to_id);
                                    if let Err(e) = nomen_db::create_typed_edge(
                                        db,
                                        fid,
                                        tid,
                                        &rel.relation,
                                        rel.detail.as_deref(),
                                    )
                                    .await
                                    {
                                        warn!(
                                            "Failed to create typed edge {} -> {}: {e}",
                                            rel.from, rel.to
                                        );
                                    }
                                }
                            }

                            if !extracted_entities.is_empty() || !extracted_relationships.is_empty()
                            {
                                debug!(
                                    topic = %memory.topic,
                                    entities = extracted_entities.len(),
                                    relationships = extracted_relationships.len(),
                                    "Extracted entities and relationships from consolidated memory"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            topic = %memory.topic,
                            "Entity extraction failed, skipping: {e}"
                        );
                    }
                }
            }

            // Publish consolidated memory to relay as kind 31234
            if let Some(relay) = relay {
                let content_str = content_for_entities.clone();

                // Encrypt if personal/private tier
                let base = nomen_core::memory::base_tier(&tier);
                let final_content = if base == "personal" || base == "private" {
                    match relay.signer().encrypt(&content_str) {
                        Ok(encrypted) => encrypted,
                        Err(e) => {
                            warn!("Failed to encrypt private memory for relay: {e}");
                            content_str
                        }
                    }
                } else {
                    content_str
                };

                // Build tags (v0.2: visibility + scope indexed tags)
                let version_str = if is_merge { "2" } else { "1" };
                // Extract visibility and scope from the d_tag for indexed tags
                let (vis_tag, scope_tag) = nomen_core::memory::extract_visibility_scope(&d_tag);
                let mut event_tags = vec![
                    Tag::custom(TagKind::Custom("d".into()), vec![d_tag.clone()]),
                    Tag::custom(TagKind::Custom("visibility".into()), vec![vis_tag]),
                    Tag::custom(TagKind::Custom("scope".into()), vec![scope_tag]),
                    Tag::custom(
                        TagKind::Custom("model".into()),
                        vec!["nomen/consolidation".to_string()],
                    ),
                    Tag::custom(
                        TagKind::Custom("version".into()),
                        vec![version_str.to_string()],
                    ),
                    Tag::custom(
                        TagKind::Custom("consolidated_from".into()),
                        vec![consolidated_from_count.clone()],
                    ),
                    Tag::custom(
                        TagKind::Custom("consolidated_at".into()),
                        vec![consolidated_at.clone()],
                    ),
                ];

                // Add topic tags from the LLM-derived topic
                for part in memory.topic.split('/') {
                    if !part.is_empty() {
                        event_tags.push(Tag::custom(
                            TagKind::Custom("t".into()),
                            vec![part.to_string()],
                        ));
                    }
                }

                // Add h tag for group-scoped memories (NIP-29)
                if tier.starts_with("group:") {
                    if let Some(group_id) = tier.strip_prefix("group:") {
                        event_tags.push(Tag::custom(
                            TagKind::Custom("h".into()),
                            vec![group_id.to_string()],
                        ));
                    }
                }

                let builder =
                    EventBuilder::new(Kind::Custom(nomen_core::kinds::MEMORY_KIND), final_content)
                        .tags(event_tags);

                match relay.publish(builder).await {
                    Ok(result) => {
                        debug!(
                            event_id = %result.event_id,
                            d_tag = %d_tag,
                            "Published consolidated memory to relay"
                        );
                        report.events_published += 1;
                    }
                    Err(e) => {
                        warn!(d_tag = %d_tag, "Failed to publish consolidated memory: {e}");
                    }
                }
            }

            // Track channel for reporting
            let channel = group_msgs
                .first()
                .map(|m| m.channel.as_str())
                .unwrap_or("general");
            if !channel.is_empty() && !report.channels.contains(&channel.to_string()) {
                report.channels.push(channel.to_string());
            }
        }

        // Collect message IDs for this group
        for msg in group_msgs {
            all_consumed_msg_ids.push(msg.id.clone());
        }
    }

    if config.dry_run {
        return Ok(report);
    }

    // Mark collected messages as consolidated (permanent — no pruning)
    if !all_consumed_msg_ids.is_empty() {
        nomen_db::mark_collected_consolidated(db, &all_consumed_msg_ids).await?;
    }

    // Publish NIP-09 deletion events for consumed ephemerals
    if let Some(relay) = relay {
        let deleted = publish_deletion_events(relay, &messages).await?;
        report.events_deleted = deleted;
    }

    // Record consolidation run timestamp for auto-trigger
    if report.memories_created > 0 || report.memories_updated > 0 {
        record_consolidation_run(db).await.ok();
    }

    debug!(
        memories = report.memories_created,
        messages = report.messages_processed,
        deleted = report.events_deleted,
        "Consolidation complete"
    );

    Ok(report)
}

/// Publish NIP-09 kind 5 deletion events for consumed ephemeral messages.
///
/// Groups deletions by batch to avoid excessively large events.
async fn publish_deletion_events(
    relay: &RelayManager,
    messages: &[RawMessageRecord],
) -> Result<usize> {
    // Collect any messages that have nostr source_ids (these are the event IDs on relay)
    let event_ids: Vec<&str> = messages
        .iter()
        .filter(|m| !m.source_id.is_empty() && m.source == "nostr")
        .map(|m| m.source_id.as_str())
        .collect();

    if event_ids.is_empty() {
        debug!("No Nostr event IDs to delete (messages may be from non-Nostr sources)");
        return Ok(0);
    }

    // Batch deletion events (max 50 e-tags per event)
    let mut deleted = 0usize;
    for chunk in event_ids.chunks(50) {
        let mut tags = Vec::new();
        for eid_str in chunk {
            if let Ok(eid) = EventId::from_hex(eid_str) {
                tags.push(Tag::event(eid));
            }
        }

        if tags.is_empty() {
            continue;
        }

        let delete_builder = EventBuilder::new(Kind::Custom(5), "consolidated").tags(tags);

        match relay.publish(delete_builder).await {
            Ok(result) => {
                deleted += chunk.len();
                debug!(
                    event_id = %result.event_id,
                    count = chunk.len(),
                    "Published NIP-09 deletion event"
                );
            }
            Err(e) => {
                warn!("Failed to publish deletion event: {e}");
            }
        }
    }

    Ok(deleted)
}

/// Fetch an existing memory record by d_tag for merge checks.
pub(crate) async fn get_existing_memory(
    db: &Surreal<Db>,
    d_tag: &str,
) -> Result<Option<ExistingMemory>> {
    #[derive(Deserialize, SurrealValue)]
    struct Row {
        content: String,
        version: i64,
    }
    let rows: Vec<Row> = db
        .query("SELECT content, version FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(rows.into_iter().next().map(|r| ExistingMemory {
        content: r.content,
        version: r.version,
    }))
}

/// Bump the version field on a memory record.
pub(crate) async fn bump_memory_version(db: &Surreal<Db>, d_tag: &str) -> Result<()> {
    db.query("UPDATE memory SET version = version + 1 WHERE d_tag = $d_tag")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?;
    Ok(())
}

/// Find a similar existing memory by embedding cosine similarity.
/// Returns the d_tag of the most similar memory if similarity > threshold.
async fn find_similar_memory(
    db: &Surreal<Db>,
    embedding: &[f32],
    threshold: f64,
) -> Result<Option<String>> {
    #[derive(Deserialize, SurrealValue)]
    struct SimRow {
        d_tag: Option<String>,
        similarity: Option<f64>,
    }
    let rows: Vec<SimRow> = db
        .query(
            "SELECT d_tag, vector::similarity::cosine(embedding, $vec) AS similarity \
             FROM memory WHERE embedding IS NOT NONE \
             ORDER BY similarity DESC LIMIT 1",
        )
        .bind(("vec", embedding.to_vec()))
        .await?
        .check()?
        .take(0)?;

    if let Some(row) = rows.first() {
        if let (Some(ref dtag), Some(sim)) = (&row.d_tag, row.similarity) {
            if sim >= threshold {
                return Ok(Some(dtag.clone()));
            }
        }
    }
    Ok(None)
}

/// Get the SurrealDB record ID for a memory by its d_tag.
pub(crate) async fn get_memory_record_id(db: &Surreal<Db>, d_tag: &str) -> Result<String> {
    #[derive(Deserialize, SurrealValue)]
    struct IdRow {
        id: String,
    }
    let rows: Vec<IdRow> = db
        .query("SELECT string::concat(meta::tb(id), ':', meta::id(id)) AS id FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;
    rows.first()
        .map(|r| r.id.clone())
        .ok_or_else(|| anyhow::anyhow!("Memory not found for d_tag: {d_tag}"))
}

/// Check if consolidation is due based on config interval and message count.
pub async fn check_consolidation_due(
    db: &Surreal<Db>,
    config: &nomen_core::config::MemoryConsolidationConfig,
) -> Result<ConsolidationStatus> {
    let pending = nomen_db::count_unconsolidated_collected(db).await?;
    let last_run = nomen_db::get_meta(db, META_KEY_LAST_CONSOLIDATION).await?;

    // Check if count threshold exceeded
    if pending >= config.max_ephemeral_count {
        return Ok(ConsolidationStatus {
            due: true,
            reason: format!(
                "Pending message count ({pending}) exceeds threshold ({})",
                config.max_ephemeral_count
            ),
            last_run: last_run.clone(),
            hours_since_last_run: last_run.as_deref().and_then(super::types::hours_since),
            pending_messages: pending,
            interval_hours: config.interval_hours,
            max_ephemeral_count: config.max_ephemeral_count,
        });
    }

    // Check time-based interval
    let hours_elapsed = last_run.as_deref().and_then(super::types::hours_since);
    let interval_exceeded = match hours_elapsed {
        Some(h) => h >= config.interval_hours as f64,
        None => true, // Never run before
    };

    if interval_exceeded && pending > 0 {
        let reason = match hours_elapsed {
            Some(h) => format!(
                "Interval exceeded ({h:.1}h >= {}h) with {pending} pending messages",
                config.interval_hours
            ),
            None => format!("Never run before, {pending} pending messages"),
        };
        return Ok(ConsolidationStatus {
            due: true,
            reason,
            last_run: last_run.clone(),
            hours_since_last_run: hours_elapsed,
            pending_messages: pending,
            interval_hours: config.interval_hours,
            max_ephemeral_count: config.max_ephemeral_count,
        });
    }

    let reason = if pending == 0 {
        "No pending messages".to_string()
    } else {
        format!(
            "Not yet due ({:.1}h / {}h interval, {pending} / {} messages)",
            hours_elapsed.unwrap_or(0.0),
            config.interval_hours,
            config.max_ephemeral_count
        )
    };

    Ok(ConsolidationStatus {
        due: false,
        reason,
        last_run: last_run.clone(),
        hours_since_last_run: hours_elapsed,
        pending_messages: pending,
        interval_hours: config.interval_hours,
        max_ephemeral_count: config.max_ephemeral_count,
    })
}

/// Record that a consolidation run just completed.
pub async fn record_consolidation_run(db: &Surreal<Db>) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    nomen_db::set_meta(db, META_KEY_LAST_CONSOLIDATION, &now).await
}
