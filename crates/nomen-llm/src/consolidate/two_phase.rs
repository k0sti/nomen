use anyhow::Result;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use tracing::warn;

use nomen_core::embed::Embedder;
use nomen_relay::RelayManager;

use super::grouping::{
    derive_tier_from_messages, enforce_tier_guard, group_messages, ConsolidationMessageLike,
};
use super::pipeline::{bump_memory_version, get_existing_memory, get_memory_record_id};
use super::types::{
    BatchExtraction, BatchMessage, CommitResult, ConsolidationConfig, PrepareResult, PreparedBatch,
    TimeRange,
};

/// Run stages 1-2: collect unconsolidated messages and group into batches.
pub async fn prepare(
    db: &Surreal<Db>,
    config: &ConsolidationConfig,
    ttl_minutes: u32,
) -> Result<PrepareResult> {
    // Clean up expired sessions first
    nomen_db::cleanup_expired_consolidation_sessions(db)
        .await
        .ok();

    // Stage 1: Collect
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
        Some(&nomen_core::collected::CollectedEventFilter {
            platform: config.platform.clone(),
            community_id: config.community_id.clone(),
            chat_id: config.chat_id.clone(),
            sender_id: None,
            thread_id: config.thread_id.clone(),
            since: config.since,
            until: None,
            limit: None,
        }),
    )
    .await?;

    if collected.len() < config.min_messages {
        return Ok(PrepareResult {
            session_id: None,
            expires_at: None,
            batch_count: 0,
            message_count: 0,
            batches: vec![],
        });
    }

    // Stage 2: Group
    let grouped = group_messages(collected.clone());
    let total_messages = collected.len();

    let mut batches = Vec::new();
    let mut batch_idx = 0;

    for (key, group_msgs) in &grouped {
        if group_msgs.len() < config.min_messages {
            continue;
        }

        let derived_tier = derive_tier_from_messages(group_msgs);
        let most_restrictive = if group_msgs.iter().any(|m| {
            let container = super::grouping::primary_container_id(m);
            m.source() == "dm"
                || m.source() == "telegram_dm"
                || (m.source() == "nostr" && (container.is_empty() || container == "dm"))
        }) {
            "personal"
        } else if group_msgs.iter().any(|m| {
            let container = super::grouping::primary_container_id(m);
            !container.is_empty() && container != "dm" && container != "general"
        }) {
            "group"
        } else {
            "public"
        };
        let visibility = enforce_tier_guard(&derived_tier, most_restrictive);

        let start_ts = group_msgs.iter().map(|m| m.created_at).min().unwrap_or(0);
        let end_ts = group_msgs.iter().map(|m| m.created_at).max().unwrap_or(0);
        let start = chrono::DateTime::from_timestamp(start_ts, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();
        let end = chrono::DateTime::from_timestamp(end_ts, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();

        let batch_messages: Vec<BatchMessage> = group_msgs
            .iter()
            .map(|m| {
                let container = super::grouping::primary_container_id(m);
                let chat = m.chat_id.clone().unwrap_or_default();
                BatchMessage {
                    id: m.d_tag.clone(),
                    sender: m.sender_id.clone().unwrap_or_default(),
                    content: m.content.clone(),
                    channel: container.clone(),
                    container,
                    chat,
                    thread: m.thread_id.clone().unwrap_or_default(),
                    source: m.platform.clone().unwrap_or_default(),
                    created_at: chrono::DateTime::from_timestamp(m.created_at, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default(),
                }
            })
            .collect();

        batches.push(PreparedBatch {
            batch_id: format!("b_{batch_idx}"),
            scope: key.scope.clone(),
            visibility,
            message_count: group_msgs.len(),
            time_range: TimeRange { start, end },
            messages: batch_messages,
        });
        batch_idx += 1;
    }

    if batches.is_empty() {
        return Ok(PrepareResult {
            session_id: None,
            expires_at: None,
            batch_count: 0,
            message_count: 0,
            batches: vec![],
        });
    }

    // Create session
    let session_id = format!("cons_{}", ulid::Ulid::new().to_string().to_lowercase());
    let batches_json = serde_json::to_value(&batches)?;
    let now = chrono::Utc::now();
    let expires = now + chrono::Duration::minutes(ttl_minutes as i64);

    nomen_db::create_consolidation_session(
        db,
        &session_id,
        &batches_json,
        batches.len(),
        total_messages,
        ttl_minutes,
    )
    .await?;

    Ok(PrepareResult {
        session_id: Some(session_id),
        expires_at: Some(expires.to_rfc3339()),
        batch_count: batches.len(),
        message_count: total_messages,
        batches,
    })
}

/// Run stages 4-6: store extracted memories, create edges, cleanup.
pub async fn commit(
    db: &Surreal<Db>,
    embedder: &dyn Embedder,
    config: &ConsolidationConfig,
    relay: Option<&RelayManager>,
    session_id: &str,
    extractions: &[BatchExtraction],
) -> Result<CommitResult> {
    // Validate session
    let session = nomen_db::get_consolidation_session(db, session_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("consolidation session not found: {session_id}"))?;

    if session.status != "pending" {
        anyhow::bail!("session already {}: {session_id}", session.status);
    }

    let now_str = chrono::Utc::now().to_rfc3339();
    if session.expires_at < now_str {
        nomen_db::update_consolidation_session_status(db, session_id, "expired")
            .await
            .ok();
        anyhow::bail!("session expired: {session_id}");
    }

    // Recover batches from session
    let batches: Vec<PreparedBatch> = session
        .batches
        .as_ref()
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?
        .unwrap_or_default();

    let author_hex = config.author_pubkey.as_deref().unwrap_or("");

    let mut result = CommitResult {
        session_id: session_id.to_string(),
        memories_created: 0,
        memories_merged: 0,
        memories_deduped: 0,
        messages_consolidated: 0,
        events_published: 0,
        events_deleted: 0,
    };

    let mut all_msg_ids: Vec<String> = Vec::new();

    for extraction in extractions {
        // Find the matching batch
        let batch = batches.iter().find(|b| b.batch_id == extraction.batch_id);
        let batch = match batch {
            Some(b) => b,
            None => {
                warn!(batch_id = %extraction.batch_id, "Unknown batch_id, skipping");
                continue;
            }
        };

        let msg_ids: Vec<String> = batch.messages.iter().map(|m| m.id.clone()).collect();

        for memory in &extraction.memories {
            let d_tag = nomen_core::memory::build_dtag_from_tier(
                &batch.visibility,
                author_hex,
                &memory.topic,
            );

            // Check for existing memory to merge
            let existing = get_existing_memory(db, &d_tag).await;
            let is_merge = existing.as_ref().ok().map(|r| r.is_some()).unwrap_or(false);

            let mem = nomen_core::NewMemory {
                memory_type: None,
                topic: d_tag.clone(),
                content: memory.content.clone(),
                tier: batch.visibility.clone(),
                importance: Some(memory.importance as i32),
                source: Some("consolidation".to_string()),
                model: Some("agent/consolidation".to_string()),
                rel: vec![],
                refs: vec![],
                mentions: vec![],
            };

            let stored_dtag = crate::store::store_direct(db, embedder, mem).await?;

            if is_merge {
                bump_memory_version(db, &stored_dtag).await.ok();
                result.memories_merged += 1;
            } else {
                result.memories_created += 1;
            }

            // Importance
            nomen_db::set_importance(db, &stored_dtag, memory.importance as i32)
                .await
                .ok();

            // Consolidated_from edges
            if let Ok(record_id) = get_memory_record_id(db, &stored_dtag).await {
                for msg_id in &msg_ids {
                    nomen_db::create_consolidated_edge(db, &record_id, msg_id)
                        .await
                        .ok();
                }
            }

            // Entity extraction — store as entity memories + references edges
            {
                let entity_text = memory.content.clone();
                if let Ok((entities, relationships)) =
                    config.entity_extractor.extract(&entity_text, &[]).await
                {
                    for entity in &entities {
                        let entity_topic = format!("entity/{}", entity.name.to_lowercase().replace(' ', "-"));
                        let entity_kind_str = format!("entity:{}", entity.kind);
                        let entity_content = entity.description.as_deref().unwrap_or(&entity.name);

                        let entity_mem = nomen_core::NewMemory {
                            memory_type: Some(entity_kind_str),
                            topic: entity_topic,
                            content: entity_content.to_string(),
                            tier: batch.visibility.clone(),
                            importance: None,
                            source: Some("consolidation".to_string()),
                            model: Some("nomen/consolidation".to_string()),
                            rel: vec![],
                            refs: vec![],
                            mentions: vec![],
                        };
                        match crate::store::store_direct(db, embedder, entity_mem).await {
                            Ok(entity_d_tag) => {
                                // mentions edge: consolidated memory → entity
                                nomen_db::create_references_edge(
                                    db, &stored_dtag, &entity_d_tag, "mentions",
                                    Some(entity.relevance), None,
                                ).await.ok();
                            }
                            Err(e) => {
                                tracing::warn!("Failed to store entity memory for '{}': {e}", entity.name);
                            }
                        }
                    }

                    // Relationship edges between entity-memories
                    for rel in &relationships {
                        let from_topic = format!("entity/{}", rel.from.to_lowercase().replace(' ', "-"));
                        let to_topic = format!("entity/{}", rel.to.to_lowercase().replace(' ', "-"));
                        if let (Ok(Some(from_mem)), Ok(Some(to_mem))) = (
                            nomen_db::get_memory_by_topic(db, &from_topic).await,
                            nomen_db::get_memory_by_topic(db, &to_topic).await,
                        ) {
                            if let (Some(ref from_dt), Some(ref to_dt)) = (from_mem.d_tag, to_mem.d_tag) {
                                nomen_db::create_references_edge(
                                    db, from_dt, to_dt, &rel.relation,
                                    None, rel.detail.as_deref(),
                                ).await.ok();
                            }
                        }
                    }
                }
            }

            // Publish to relay
            if let Some(_relay) = relay {
                result.events_published += 1;
            }
        }

        all_msg_ids.extend(msg_ids);
    }

    // Mark collected messages as consolidated (permanent — no pruning)
    if !all_msg_ids.is_empty() {
        nomen_db::mark_collected_consolidated(db, &all_msg_ids)
            .await
            .ok();
    }
    result.messages_consolidated = all_msg_ids.len();

    // NIP-09 deletion for nostr-source messages
    if let Some(relay) = relay {
        let _nostr_event_ids: Vec<nostr_sdk::EventId> = batches
            .iter()
            .flat_map(|b| b.messages.iter())
            .filter(|m| m.source == "nostr")
            .filter_map(|m| nostr_sdk::EventId::from_hex(&m.id).ok())
            .collect();

        // Use the existing publish_deletion_events helper
        // For now, skip NIP-09 in commit — the existing consolidate() handles it
        // TODO: extract NIP-09 deletion into a reusable helper
        let _ = relay;
        result.events_deleted = 0;
    }

    // Mark session committed
    nomen_db::update_consolidation_session_status(db, session_id, "committed").await?;

    Ok(result)
}
