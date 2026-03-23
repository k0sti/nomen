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
        content: "Use anyhow for application errors\n\nanyhow provides easy error context chaining".to_string(),
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

    // Ingest several messages
    for i in 0..5 {
        let msg = nomen::ingest::RawMessage {
            source: "test".to_string(),
            source_id: Some(format!("msg-{i}")),
            sender: "alice".to_string(),
            channel: Some("general".to_string()),
            content: format!("Test message number {i} about Rust programming"),
            metadata: None,
            created_at: None,
        };
        nomen::ingest::ingest_message(&db, &msg).await?;
    }

    // Verify messages were stored
    let query = nomen::ingest::MessageQuery {
        source: Some("test".to_string()),
        ..Default::default()
    };
    let messages = nomen::ingest::get_messages(&db, &query).await?;
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
    let query_consolidated = nomen::ingest::MessageQuery {
        source: Some("test".to_string()),
        consolidated_only: true,
        ..Default::default()
    };
    let consolidated = nomen::ingest::get_messages(&db, &query_consolidated).await?;
    assert_eq!(consolidated.len(), 5, "All messages should be consolidated");

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
async fn test_prune() -> Result<()> {
    let (db, _tmp) = init_test_db().await?;

    // Ingest messages with old timestamps
    let old_date = "2025-01-01T00:00:00+00:00";
    for i in 0..3 {
        let msg = nomen::ingest::RawMessage {
            source: "test".to_string(),
            source_id: Some(format!("old-{i}")),
            sender: "bob".to_string(),
            channel: Some("general".to_string()),
            content: format!("Old message {i}"),
            metadata: None,
            created_at: Some(old_date.to_string()),
        };
        nomen::ingest::ingest_message(&db, &msg).await?;
    }

    // Mark them consolidated
    let query = nomen::ingest::MessageQuery::default();
    let msgs = nomen::ingest::get_messages(&db, &query).await?;
    let ids: Vec<String> = msgs.iter().map(|m| m.id.clone()).collect();
    nomen::db::mark_messages_consolidated(&db, &ids).await?;

    // Prune messages older than 2025-06-01
    let pruned = nomen::db::prune_old_messages(&db, "2025-06-01T00:00:00+00:00").await?;
    assert_eq!(pruned, 3, "Should prune all 3 old consolidated messages");

    Ok(())
}
