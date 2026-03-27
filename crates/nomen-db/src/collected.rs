//! collected_message table CRUD — kind 30100 event storage and tag-indexed queries.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;
use tracing::debug;

use nomen_core::collected::{CollectedEvent, CollectedEventFilter};

use crate::deserialize_option_string;

/// A collected message record as stored in SurrealDB.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct CollectedMessageRecord {
    #[serde(default)]
    pub event_json: String,
    pub d_tag: String,
    pub kind: i64,
    pub pubkey: String,
    pub created_at: i64,
    pub content: String,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub platform: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub community_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub community_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub chat_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub sender_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub thread_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub chat_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub chat_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub thread_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub message_id: Option<String>,
    #[serde(default)]
    pub consolidated: bool,
}

/// Result of a store operation.
#[derive(Debug, Serialize)]
pub struct StoreResult {
    pub d_tag: String,
    pub stored: bool,
    pub replaced: bool,
}

/// Store a kind 30100 collected event, upserting by d-tag.
pub async fn store_collected_event(
    db: &Surreal<Db>,
    event: &CollectedEvent,
) -> Result<StoreResult> {
    let d_tag = event
        .d_tag()
        .ok_or_else(|| anyhow::anyhow!("event missing d tag"))?
        .to_string();

    let event_json = serde_json::to_string(event)?;

    // Check if record with this d_tag already exists
    #[derive(Deserialize, SurrealValue)]
    struct ExistsRow {
        #[allow(dead_code)]
        d_tag: String,
    }
    let existing: Option<ExistsRow> = db
        .query("SELECT d_tag FROM collected_message WHERE d_tag = $dtag LIMIT 1")
        .bind(("dtag", d_tag.clone()))
        .await?
        .check()?
        .take(0)?;
    let replaced = existing.is_some();

    #[derive(Serialize, SurrealValue)]
    struct NewCollectedMessage {
        event_json: String,
        d_tag: String,
        kind: i64,
        pubkey: String,
        created_at: i64,
        content: String,
        platform: Option<String>,
        community_id: Option<String>,
        community_type: Option<String>,
        chat_id: Option<String>,
        sender_id: Option<String>,
        thread_id: Option<String>,
        chat_type: Option<String>,
        chat_name: Option<String>,
        thread_type: Option<String>,
        message_id: Option<String>,
        consolidated: bool,
    }

    let record = NewCollectedMessage {
        event_json,
        d_tag: d_tag.clone(),
        kind: event.kind as i64,
        pubkey: event.pubkey.clone(),
        created_at: event.created_at,
        content: event.content.clone(),
        platform: event.platform().map(String::from),
        community_id: event.community_id().map(String::from),
        community_type: event.community_type().map(String::from),
        chat_id: event.chat_id().map(String::from),
        sender_id: event.sender_id().map(String::from),
        thread_id: event.thread_id().map(String::from),
        chat_type: event.chat_type().map(String::from),
        chat_name: event.chat_name().map(String::from),
        thread_type: event.thread_type().map(String::from),
        message_id: event.message_id().map(String::from),
        consolidated: false,
    };

    if replaced {
        // Update existing record, preserve consolidated
        db.query(
            "UPDATE collected_message SET \
             event_json = $ej, kind = $k, pubkey = $pk, created_at = $ca, \
             content = $ct, platform = $pl, community_id = $coi, community_type = $cot, \
             chat_id = $ci, sender_id = $si, thread_id = $ti, chat_type = $ctp, \
             chat_name = $cn, thread_type = $tt, message_id = $mid \
             WHERE d_tag = $dtag",
        )
        .bind(("ej", record.event_json))
        .bind(("k", record.kind))
        .bind(("pk", record.pubkey))
        .bind(("ca", record.created_at))
        .bind(("ct", record.content))
        .bind(("pl", record.platform))
        .bind(("coi", record.community_id))
        .bind(("cot", record.community_type))
        .bind(("ci", record.chat_id))
        .bind(("si", record.sender_id))
        .bind(("ti", record.thread_id))
        .bind(("ctp", record.chat_type))
        .bind(("cn", record.chat_name))
        .bind(("tt", record.thread_type))
        .bind(("mid", record.message_id))
        .bind(("dtag", d_tag.clone()))
        .await?
        .check()?;
    } else {
        db.query("CREATE collected_message CONTENT $record")
            .bind(("record", record))
            .await?
            .check()?;
    }

    debug!(d_tag = %d_tag, replaced, "Stored collected event");

    Ok(StoreResult {
        d_tag,
        stored: true,
        replaced,
    })
}

/// Query collected events with tag-based filtering.
pub async fn query_collected_events(
    db: &Surreal<Db>,
    filter: &CollectedEventFilter,
) -> Result<Vec<CollectedMessageRecord>> {
    let mut conditions = Vec::new();

    if let Some(ref platforms) = filter.platform {
        if platforms.len() == 1 {
            conditions.push("platform = $platform".to_string());
        } else if !platforms.is_empty() {
            conditions.push("platform IN $platforms".to_string());
        }
    }
    if let Some(ref communities) = filter.community_id {
        if communities.len() == 1 {
            conditions.push("community_id = $community_id".to_string());
        } else if !communities.is_empty() {
            conditions.push("community_id IN $community_ids".to_string());
        }
    }
    if let Some(ref chats) = filter.chat_id {
        if chats.len() == 1 {
            conditions.push("chat_id = $chat_id".to_string());
        } else if !chats.is_empty() {
            conditions.push("chat_id IN $chat_ids".to_string());
        }
    }
    if let Some(ref senders) = filter.sender_id {
        if senders.len() == 1 {
            conditions.push("sender_id = $sender_id".to_string());
        } else if !senders.is_empty() {
            conditions.push("sender_id IN $sender_ids".to_string());
        }
    }
    if let Some(ref threads) = filter.thread_id {
        if threads.len() == 1 {
            conditions.push("thread_id = $thread_id".to_string());
        } else if !threads.is_empty() {
            conditions.push("thread_id IN $thread_ids".to_string());
        }
    }
    if filter.since.is_some() {
        conditions.push("created_at >= $since".to_string());
    }
    if filter.until.is_some() {
        conditions.push("created_at <= $until".to_string());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let limit = filter.limit.unwrap_or(100);
    let sql = format!(
        "SELECT event_json, d_tag, kind, pubkey, created_at, content, \
         platform, community_id, community_type, chat_id, sender_id, thread_id, \
         chat_type, chat_name, thread_type, message_id, consolidated \
         FROM collected_message {where_clause} \
         ORDER BY created_at ASC LIMIT {limit}"
    );

    let mut q = db.query(&sql);

    if let Some(ref platforms) = filter.platform {
        if platforms.len() == 1 {
            q = q.bind(("platform", platforms[0].clone()));
        } else if !platforms.is_empty() {
            q = q.bind(("platforms", platforms.clone()));
        }
    }
    if let Some(ref communities) = filter.community_id {
        if communities.len() == 1 {
            q = q.bind(("community_id", communities[0].clone()));
        } else if !communities.is_empty() {
            q = q.bind(("community_ids", communities.clone()));
        }
    }
    if let Some(ref chats) = filter.chat_id {
        if chats.len() == 1 {
            q = q.bind(("chat_id", chats[0].clone()));
        } else if !chats.is_empty() {
            q = q.bind(("chat_ids", chats.clone()));
        }
    }
    if let Some(ref senders) = filter.sender_id {
        if senders.len() == 1 {
            q = q.bind(("sender_id", senders[0].clone()));
        } else if !senders.is_empty() {
            q = q.bind(("sender_ids", senders.clone()));
        }
    }
    if let Some(ref threads) = filter.thread_id {
        if threads.len() == 1 {
            q = q.bind(("thread_id", threads[0].clone()));
        } else if !threads.is_empty() {
            q = q.bind(("thread_ids", threads.clone()));
        }
    }
    if let Some(since) = filter.since {
        q = q.bind(("since", since));
    }
    if let Some(until) = filter.until {
        q = q.bind(("until", until));
    }

    let results: Vec<CollectedMessageRecord> = q.await?.check()?.take(0)?;
    Ok(results)
}

/// Get a single collected event by d-tag.
pub async fn get_collected_event(
    db: &Surreal<Db>,
    d_tag: &str,
) -> Result<Option<CollectedMessageRecord>> {
    let result: Option<CollectedMessageRecord> = db
        .query(
            "SELECT event_json, d_tag, kind, pubkey, created_at, content, \
             platform, community_id, community_type, chat_id, sender_id, thread_id, \
             chat_type, chat_name, thread_type, message_id, consolidated \
             FROM collected_message WHERE d_tag = $dtag LIMIT 1",
        )
        .bind(("dtag", d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(result)
}

/// Count collected events, optionally filtered.
pub async fn count_collected_events(db: &Surreal<Db>, consolidated: Option<bool>) -> Result<usize> {
    #[derive(Deserialize, SurrealValue)]
    struct CountRow {
        count: usize,
    }

    let sql = if let Some(extracted) = consolidated {
        format!(
            "SELECT count() AS count FROM collected_message WHERE consolidated = {extracted} GROUP ALL"
        )
    } else {
        "SELECT count() AS count FROM collected_message GROUP ALL".to_string()
    };

    let result: Option<CountRow> = db.query(&sql).await?.check()?.take(0)?;
    Ok(result.map(|r| r.count).unwrap_or(0))
}

/// Get unconsolidated collected events for the consolidation pipeline.
/// Returns records with `consolidated = false`, ordered by created_at ASC.
pub async fn get_unconsolidated_collected(
    db: &Surreal<Db>,
    limit: usize,
    before_ts: Option<i64>,
    filter: Option<&CollectedEventFilter>,
) -> Result<Vec<CollectedMessageRecord>> {
    let mut conditions = vec!["consolidated = false".to_string()];
    if before_ts.is_some() {
        conditions.push("created_at < $before".to_string());
    }
    if let Some(filter) = filter {
        if let Some(ref platforms) = filter.platform {
            if platforms.len() == 1 {
                conditions.push("platform = $platform".to_string());
            } else if !platforms.is_empty() {
                conditions.push("platform IN $platforms".to_string());
            }
        }
        if let Some(ref communities) = filter.community_id {
            if communities.len() == 1 {
                conditions.push("community_id = $community_id".to_string());
            } else if !communities.is_empty() {
                conditions.push("community_id IN $community_ids".to_string());
            }
        }
        if let Some(ref chats) = filter.chat_id {
            if chats.len() == 1 {
                conditions.push("chat_id = $chat_id".to_string());
            } else if !chats.is_empty() {
                conditions.push("chat_id IN $chat_ids".to_string());
            }
        }
        if let Some(ref threads) = filter.thread_id {
            if threads.len() == 1 {
                conditions.push("thread_id = $thread_id".to_string());
            } else if !threads.is_empty() {
                conditions.push("thread_id IN $thread_ids".to_string());
            }
        }
        if filter.since.is_some() {
            conditions.push("created_at >= $since".to_string());
        }
        if filter.until.is_some() {
            conditions.push("created_at <= $until".to_string());
        }
    }
    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT event_json, d_tag, kind, pubkey, created_at, content, \
         platform, community_id, community_type, chat_id, sender_id, thread_id, \
         chat_type, chat_name, thread_type, message_id, consolidated \
         FROM collected_message WHERE {where_clause} \
         ORDER BY created_at ASC LIMIT {limit}"
    );

    let mut q = db.query(&sql);
    if let Some(before) = before_ts {
        q = q.bind(("before", before));
    }
    if let Some(filter) = filter {
        if let Some(ref platforms) = filter.platform {
            if platforms.len() == 1 {
                q = q.bind(("platform", platforms[0].clone()));
            } else if !platforms.is_empty() {
                q = q.bind(("platforms", platforms.clone()));
            }
        }
        if let Some(ref communities) = filter.community_id {
            if communities.len() == 1 {
                q = q.bind(("community_id", communities[0].clone()));
            } else if !communities.is_empty() {
                q = q.bind(("community_ids", communities.clone()));
            }
        }
        if let Some(ref chats) = filter.chat_id {
            if chats.len() == 1 {
                q = q.bind(("chat_id", chats[0].clone()));
            } else if !chats.is_empty() {
                q = q.bind(("chat_ids", chats.clone()));
            }
        }
        if let Some(ref threads) = filter.thread_id {
            if threads.len() == 1 {
                q = q.bind(("thread_id", threads[0].clone()));
            } else if !threads.is_empty() {
                q = q.bind(("thread_ids", threads.clone()));
            }
        }
        if let Some(since) = filter.since {
            q = q.bind(("since", since));
        }
        if let Some(until) = filter.until {
            q = q.bind(("until", until));
        }
    }

    let results: Vec<CollectedMessageRecord> = q.await?.check()?.take(0)?;
    Ok(results)
}

/// Mark collected events as consolidated by their d-tags.
/// Messages stay in the table (permanent archive) — only the flag changes.
pub async fn mark_collected_consolidated(db: &Surreal<Db>, d_tags: &[String]) -> Result<()> {
    if d_tags.is_empty() {
        return Ok(());
    }
    db.query("UPDATE collected_message SET consolidated = true WHERE d_tag IN $dtags")
        .bind(("dtags", d_tags.to_vec()))
        .await?
        .check()?;
    debug!(
        count = d_tags.len(),
        "Marked collected events as consolidated"
    );
    Ok(())
}

/// A BM25 search result with relevance score.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct CollectedSearchResult {
    #[serde(default)]
    pub event_json: String,
    pub d_tag: String,
    pub kind: i64,
    pub pubkey: String,
    pub created_at: i64,
    pub content: String,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub platform: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub community_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub chat_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub sender_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_string")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub score: f64,
}

/// BM25 fulltext search over collected messages.
///
/// Uses the `cm_fulltext` index for relevance scoring. Tag filters narrow
/// the search scope before BM25 scoring.
pub async fn search_collected_events(
    db: &Surreal<Db>,
    query: &str,
    filter: &CollectedEventFilter,
) -> Result<Vec<CollectedSearchResult>> {
    let mut conditions = vec!["content @@ $query".to_string()];

    if let Some(ref platforms) = filter.platform {
        if platforms.len() == 1 {
            conditions.push("platform = $platform".to_string());
        } else if !platforms.is_empty() {
            conditions.push("platform IN $platforms".to_string());
        }
    }
    if let Some(ref communities) = filter.community_id {
        if communities.len() == 1 {
            conditions.push("community_id = $community_id".to_string());
        } else if !communities.is_empty() {
            conditions.push("community_id IN $community_ids".to_string());
        }
    }
    if let Some(ref chats) = filter.chat_id {
        if chats.len() == 1 {
            conditions.push("chat_id = $chat_id".to_string());
        } else if !chats.is_empty() {
            conditions.push("chat_id IN $chat_ids".to_string());
        }
    }
    if let Some(ref senders) = filter.sender_id {
        if senders.len() == 1 {
            conditions.push("sender_id = $sender_id".to_string());
        } else if !senders.is_empty() {
            conditions.push("sender_id IN $sender_ids".to_string());
        }
    }
    if let Some(ref threads) = filter.thread_id {
        if threads.len() == 1 {
            conditions.push("thread_id = $thread_id".to_string());
        } else if !threads.is_empty() {
            conditions.push("thread_id IN $thread_ids".to_string());
        }
    }
    if filter.since.is_some() {
        conditions.push("created_at >= $since".to_string());
    }
    if filter.until.is_some() {
        conditions.push("created_at <= $until".to_string());
    }

    let where_clause = format!("WHERE {}", conditions.join(" AND "));
    let limit = filter.limit.unwrap_or(20);

    let sql = format!(
        "SELECT event_json, d_tag, kind, pubkey, created_at, content, \
         platform, community_id, chat_id, sender_id, thread_id, \
         search::score(0) AS score \
         FROM collected_message {where_clause} \
         ORDER BY score DESC LIMIT {limit}"
    );

    let mut q = db.query(&sql).bind(("query", query.to_string()));

    if let Some(ref platforms) = filter.platform {
        if platforms.len() == 1 {
            q = q.bind(("platform", platforms[0].clone()));
        } else if !platforms.is_empty() {
            q = q.bind(("platforms", platforms.clone()));
        }
    }
    if let Some(ref communities) = filter.community_id {
        if communities.len() == 1 {
            q = q.bind(("community_id", communities[0].clone()));
        } else if !communities.is_empty() {
            q = q.bind(("community_ids", communities.clone()));
        }
    }
    if let Some(ref chats) = filter.chat_id {
        if chats.len() == 1 {
            q = q.bind(("chat_id", chats[0].clone()));
        } else if !chats.is_empty() {
            q = q.bind(("chat_ids", chats.clone()));
        }
    }
    if let Some(ref senders) = filter.sender_id {
        if senders.len() == 1 {
            q = q.bind(("sender_id", senders[0].clone()));
        } else if !senders.is_empty() {
            q = q.bind(("sender_ids", senders.clone()));
        }
    }
    if let Some(ref threads) = filter.thread_id {
        if threads.len() == 1 {
            q = q.bind(("thread_id", threads[0].clone()));
        } else if !threads.is_empty() {
            q = q.bind(("thread_ids", threads.clone()));
        }
    }
    if let Some(since) = filter.since {
        q = q.bind(("since", since));
    }
    if let Some(until) = filter.until {
        q = q.bind(("until", until));
    }

    let results: Vec<CollectedSearchResult> = q.await?.check()?.take(0)?;
    Ok(results)
}

/// Count unconsolidated collected events.
pub async fn count_unconsolidated_collected(db: &Surreal<Db>) -> Result<usize> {
    #[derive(Deserialize, SurrealValue)]
    struct CountRow {
        count: usize,
    }
    let result: Option<CountRow> = db
        .query(
            "SELECT count() AS count FROM collected_message WHERE consolidated = false GROUP ALL",
        )
        .await?
        .check()?
        .take(0)?;
    Ok(result.map(|r| r.count).unwrap_or(0))
}
