//! Integration tests for Nomen using temp-dir SurrealDB (SurrealKv).

use anyhow::Result;
use nomen::groups::GroupStoreExt;
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;

/// Initialize a SurrealDB instance in a temp directory with the nomen schema.
async fn init_test_db() -> Result<(Surreal<Db>, tempfile::TempDir)> {
    let tmp = tempfile::tempdir()?;
    let db = Surreal::new::<SurrealKv>(tmp.path()).versioned().await?;
    db.use_ns("nomen_test").use_db("nomen_test").await?;
    db.query(nomen::db::SCHEMA).await?.check()?;
    Ok((db, tmp))
}

#[tokio::test]
async fn test_store_and_search() -> Result<()> {
    let (db, _tmp) = init_test_db().await?;

    // Store a memory
    let parsed = nomen::memory::ParsedMemory {
        tier: "public".to_string(),
        visibility: "public".to_string(),
        topic: "rust/error-handling".to_string(),
        model: "test".to_string(),
        content: "Use anyhow for application errors\n\nanyhow provides easy error context chaining"
            .to_string(),
        created_at: nostr_sdk::Timestamp::now(),
        d_tag: "snow:memory:rust/error-handling".to_string(),
        source: "test".to_string(),
        importance: None,
    };

    nomen::db::store_memory_direct(&db, &parsed, "test-event-1").await?;

    // Search for it using text search
    let results = nomen::db::search_memories(&db, "anyhow error", None, None, 10).await?;
    assert!(
        !results.is_empty(),
        "Should find stored memory via text search"
    );
    assert_eq!(results[0].topic, "rust/error-handling");

    // Delete it
    nomen::db::delete_memory_by_dtag(&db, "snow:memory:rust/error-handling").await?;
    let results = nomen::db::search_memories(&db, "anyhow error", None, None, 10).await?;
    assert!(results.is_empty(), "Memory should be deleted");

    Ok(())
}

#[tokio::test]
async fn test_ingest_and_consolidate() -> Result<()> {
    let (db, _tmp) = init_test_db().await?;
    let embedder = nomen::embed::NoopEmbedder;

    // Ingest messages as kind 30100 collected events
    for i in 0..5 {
        let now = chrono::Utc::now().timestamp() + i;
        let event = nomen_core::collected::CollectedEvent {
            kind: nomen_core::kinds::COLLECTED_MESSAGE_KIND,
            created_at: now,
            pubkey: String::new(),
            tags: vec![
                vec!["d".to_string(), format!("test:msg-{i}")],
                vec![
                    "proxy".to_string(),
                    format!("test:msg-{i}"),
                    "test".to_string(),
                ],
                vec!["sender".to_string(), "alice".to_string()],
                vec!["chat".to_string(), "general".to_string()],
            ],
            content: format!("Test message number {i} about Rust programming"),
            id: None,
            sig: None,
        };
        nomen::db::store_collected_event(&db, &event).await?;
    }

    // Verify messages were stored
    let filter = nomen_core::collected::CollectedEventFilter {
        platform: Some(vec!["test".to_string()]),
        community_id: None,
        chat_id: None,
        sender_id: None,
        thread_id: None,
        since: None,
        until: None,
        limit: None,
    };
    let messages = nomen::db::query_collected_events(&db, &filter).await?;
    assert_eq!(messages.len(), 5, "Should have 5 messages");

    // Run consolidation
    let config = nomen::consolidate::ConsolidationConfig {
        batch_size: 50,
        min_messages: 3,
        llm_provider: Box::new(nomen::consolidate::NoopLlmProvider),
        ..Default::default()
    };
    let report = nomen::consolidate::consolidate(&db, &embedder, &config, None).await?;
    assert_eq!(report.messages_processed, 5);
    assert!(
        report.memories_created > 0,
        "Should create at least 1 memory"
    );

    // Verify messages are marked consolidated
    let unconsolidated = nomen::db::get_unconsolidated_collected(&db, 100, None, None).await?;
    assert_eq!(
        unconsolidated.len(),
        0,
        "All messages should be consolidated"
    );

    Ok(())
}

#[tokio::test]
async fn test_groups() -> Result<()> {
    let (db, _tmp) = init_test_db().await?;

    // Create a group
    nomen::groups::create_group(
        &db,
        "atlantislabs",
        "Atlantis Labs",
        &["npub1abc".to_string(), "npub1def".to_string()],
        None,
        None,
    )
    .await?;

    // Create child group
    nomen::groups::create_group(
        &db,
        "atlantislabs.engineering",
        "Engineering",
        &["npub1abc".to_string()],
        Some("techteam"),
        None,
    )
    .await?;

    // Debug: count records
    #[derive(serde::Deserialize, Debug, SurrealValue)]
    struct CountResult {
        count: usize,
    }
    let count: Option<CountResult> = db
        .query("SELECT count() AS count FROM nomen_group GROUP ALL")
        .await?
        .check()?
        .take(0)?;
    eprintln!("DEBUG: nomen_group count: {:?}", count);

    // Debug: try meta::id query
    #[derive(serde::Deserialize, Debug, SurrealValue)]
    struct IdGroup {
        id: String,
        name: String,
        parent: String,
        members: Vec<String>,
        relay: String,
        nostr_group: String,
        created_at: String,
    }
    let id_groups: Vec<IdGroup> = db.query("SELECT meta::id(id) AS id, name, parent, members, relay, nostr_group, created_at FROM nomen_group ORDER BY id").await?.check()?.take(0)?;
    eprintln!("DEBUG: id_groups: {:?}", id_groups);

    // List groups
    let groups = nomen::groups::list_groups(&db).await?;
    assert_eq!(groups.len(), 2);

    // Check members
    let members = nomen::groups::get_members(&db, "atlantislabs").await?;
    assert_eq!(members.len(), 2);
    assert!(members.contains(&"npub1abc".to_string()));

    // Add member
    nomen::groups::add_member(&db, "atlantislabs", "npub1xyz").await?;
    let members = nomen::groups::get_members(&db, "atlantislabs").await?;
    assert_eq!(members.len(), 3);

    // Remove member
    nomen::groups::remove_member(&db, "atlantislabs", "npub1xyz").await?;
    let members = nomen::groups::get_members(&db, "atlantislabs").await?;
    assert_eq!(members.len(), 2);

    // Verify scope expansion via GroupStore
    let store = nomen::groups::GroupStore::load(&[], &db).await?;
    let scopes = store.expand_scopes("npub1abc");
    assert!(scopes.contains(&"atlantislabs".to_string()));
    assert!(scopes.contains(&"atlantislabs.engineering".to_string()));

    let scopes_def = store.expand_scopes("npub1def");
    assert!(scopes_def.contains(&"atlantislabs".to_string()));
    assert!(!scopes_def.contains(&"atlantislabs.engineering".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_collected_message_lifecycle() -> Result<()> {
    let (db, _tmp) = init_test_db().await?;

    // Store collected events with old timestamps
    let old_ts = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00+00:00")
        .unwrap()
        .timestamp();
    for i in 0..3 {
        let event = nomen_core::collected::CollectedEvent {
            kind: nomen_core::kinds::COLLECTED_MESSAGE_KIND,
            created_at: old_ts + i,
            pubkey: String::new(),
            tags: vec![
                vec!["d".to_string(), format!("test:old-{i}")],
                vec![
                    "proxy".to_string(),
                    format!("test:old-{i}"),
                    "test".to_string(),
                ],
                vec!["sender".to_string(), "bob".to_string()],
                vec!["chat".to_string(), "general".to_string()],
            ],
            content: format!("Old message {i}"),
            id: None,
            sig: None,
        };
        nomen::db::store_collected_event(&db, &event).await?;
    }

    // Verify they're unconsolidated
    let unconsolidated = nomen::db::get_unconsolidated_collected(&db, 100, None, None).await?;
    assert_eq!(unconsolidated.len(), 3);

    // Mark them consolidated
    let d_tags: Vec<String> = unconsolidated.iter().map(|m| m.d_tag.clone()).collect();
    nomen::db::mark_collected_consolidated(&db, &d_tags).await?;

    // Verify they're now consolidated (messages stay, flag changes)
    let unconsolidated = nomen::db::get_unconsolidated_collected(&db, 100, None, None).await?;
    assert_eq!(unconsolidated.len(), 0, "All should be consolidated");

    let total = nomen::db::count_collected_events(&db, None).await?;
    assert_eq!(total, 3, "Messages are permanent — not deleted");

    Ok(())
}
