//! Shared tool definitions and handlers for MCP and CVM interfaces.

use anyhow::Result;
use serde_json::{json, Value};

use crate::entities;
use crate::ingest;
use crate::search;
use crate::send;
use crate::Nomen;

/// Returns the JSON tool schema list for all nomen tools.
pub fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "nomen_search",
                "description": "Search memories using hybrid semantic + full-text search with optional graph expansion",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" },
                        "tier": { "type": "string", "description": "Filter by tier (public/group/private)" },
                        "scope": { "type": "string", "description": "Filter by scope" },
                        "limit": { "type": "integer", "description": "Max results (default 10)" },
                        "session_id": { "type": "string", "description": "Session ID to auto-derive tier/scope" },
                        "vector_weight": { "type": "number", "description": "Vector similarity weight 0.0-1.0 (default 0.7)" },
                        "text_weight": { "type": "number", "description": "Full-text BM25 weight 0.0-1.0 (default 0.3)" },
                        "aggregate": { "type": "boolean", "description": "Aggregate similar results (>0.85 similarity)" },
                        "graph_expand": { "type": "boolean", "description": "Traverse graph edges to surface related memories (default false)" },
                        "max_hops": { "type": "integer", "description": "Max hops for graph traversal (default 1, requires graph_expand)" }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "nomen_store",
                "description": "Store a new memory",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string", "description": "Topic/namespace for the memory" },
                        "summary": { "type": "string", "description": "Short summary" },
                        "detail": { "type": "string", "description": "Full detail text" },
                        "tier": { "type": "string", "description": "Visibility tier (public/group/private, default public)" },
                        "scope": { "type": "string", "description": "Scope for group tier" },
                        "confidence": { "type": "number", "description": "Confidence score 0.0-1.0 (default 0.8)" },
                        "session_id": { "type": "string", "description": "Session ID to auto-derive tier/scope" }
                    },
                    "required": ["topic", "summary"]
                }
            },
            {
                "name": "nomen_ingest",
                "description": "Ingest a raw message for later consolidation",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "description": "Source system (e.g. telegram, nostr, webhook)" },
                        "sender": { "type": "string", "description": "Sender identifier" },
                        "channel": { "type": "string", "description": "Channel/room name" },
                        "content": { "type": "string", "description": "Message content" },
                        "metadata": { "type": "object", "description": "Optional metadata" },
                        "session_id": { "type": "string", "description": "Session ID to auto-derive tier/scope" }
                    },
                    "required": ["source", "sender", "content"]
                }
            },
            {
                "name": "nomen_messages",
                "description": "Query raw messages with filters",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "description": "Filter by source" },
                        "channel": { "type": "string", "description": "Filter by channel" },
                        "sender": { "type": "string", "description": "Filter by sender" },
                        "room": { "type": "string", "description": "Filter by room" },
                        "topic": { "type": "string", "description": "Filter by topic" },
                        "thread": { "type": "string", "description": "Filter by thread" },
                        "since": { "type": "string", "description": "Show messages since (RFC3339 timestamp)" },
                        "until": { "type": "string", "description": "Show messages until (RFC3339 timestamp)" },
                        "order": { "type": "string", "description": "Sort order: asc or desc (default desc)" },
                        "include_consolidated": { "type": "boolean", "description": "Include consolidated messages (default false)" },
                        "limit": { "type": "integer", "description": "Max results (default 50)" }
                    }
                }
            },
            {
                "name": "nomen_message_search",
                "description": "Full-text BM25 search over raw messages",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" },
                        "source": { "type": "string", "description": "Filter by source" },
                        "room": { "type": "string", "description": "Filter by room" },
                        "topic": { "type": "string", "description": "Filter by topic" },
                        "sender": { "type": "string", "description": "Filter by sender" },
                        "since": { "type": "string", "description": "Show messages since (RFC3339 timestamp)" },
                        "until": { "type": "string", "description": "Show messages until (RFC3339 timestamp)" },
                        "include_consolidated": { "type": "boolean", "description": "Include consolidated messages (default false)" },
                        "limit": { "type": "integer", "description": "Max results (default 50)" }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "nomen_entities",
                "description": "List or search extracted entities",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "description": "Filter by kind (person/project/concept/place/organization)" },
                        "query": { "type": "string", "description": "Search query for entity names" }
                    }
                }
            },
            {
                "name": "nomen_consolidate",
                "description": "Trigger consolidation of raw messages into memories",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "channel": { "type": "string", "description": "Filter by channel" },
                        "since": { "type": "string", "description": "Only consolidate messages since (RFC3339)" }
                    }
                }
            },
            {
                "name": "nomen_delete",
                "description": "Delete a memory by topic or event ID",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string", "description": "Topic to delete" },
                        "id": { "type": "string", "description": "Event ID to delete" }
                    }
                }
            },
            {
                "name": "nomen_groups",
                "description": "Manage groups: list, members, create, add_member, remove_member",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "description": "Action: list, members, create, add_member, remove_member" },
                        "id": { "type": "string", "description": "Group id (required for all except list)" },
                        "name": { "type": "string", "description": "Group name (required for create)" },
                        "members": { "type": "array", "items": { "type": "string" }, "description": "Initial members (for create)" },
                        "npub": { "type": "string", "description": "Member npub (for add_member/remove_member)" },
                        "nostr_group": { "type": "string", "description": "NIP-29 group id (for create)" },
                        "relay": { "type": "string", "description": "Relay URL (for create)" }
                    },
                    "required": ["action"]
                }
            },
            {
                "name": "nomen_send",
                "description": "Send a message to a recipient via a specific channel",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "recipient": { "type": "string", "description": "npub1... for DM, group:<id> for group, 'public' for broadcast" },
                        "content": { "type": "string", "description": "Message body" },
                        "channel": { "type": "string", "description": "Delivery channel: nostr, telegram, etc. Default: nostr" },
                        "metadata": { "type": "object", "description": "Platform-specific extras" }
                    },
                    "required": ["recipient", "content"]
                }
            },
            {
                "name": "nomen_list",
                "description": "List memories from local database",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tier": { "type": "string", "description": "Filter by tier" },
                        "limit": { "type": "integer", "description": "Max results (default 100)" },
                        "stats": { "type": "boolean", "description": "Include memory statistics" }
                    }
                }
            },
            {
                "name": "nomen_sync",
                "description": "Sync memories from relay to local database",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "nomen_embed",
                "description": "Generate embeddings for memories that lack them",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer", "description": "Max memories to embed (default 100)" }
                    }
                }
            },
            {
                "name": "nomen_prune",
                "description": "Prune old/unused memories and consolidated raw messages",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "days": { "type": "integer", "description": "Delete items older than N days (default 90)" },
                        "dry_run": { "type": "boolean", "description": "Preview without deleting" }
                    }
                }
            }
        ]
    })
}

/// Resolve session_id from args to (tier, scope). Returns (None, None) if no session_id.
pub fn resolve_session_from_args(
    nomen: &Nomen,
    default_channel: &str,
    args: &Value,
) -> Result<(Option<String>, Option<String>)> {
    let session_id = args.get("session_id").and_then(|v| v.as_str());
    if let Some(sid) = session_id {
        let resolved = nomen.resolve_session(sid, default_channel)?;
        Ok((Some(resolved.tier), Some(resolved.scope)))
    } else {
        Ok((None, None))
    }
}

/// Dispatch a tool call by name. Returns the text result or an error.
pub async fn dispatch_tool(
    nomen: &Nomen,
    default_channel: &str,
    tool_name: &str,
    args: &Value,
) -> Result<String> {
    match tool_name {
        "nomen_search" => tool_search(nomen, default_channel, args).await,
        "nomen_store" => tool_store(nomen, default_channel, args).await,
        "nomen_ingest" => tool_ingest(nomen, default_channel, args).await,
        "nomen_messages" => tool_messages(nomen, default_channel, args).await,
        "nomen_message_search" => tool_message_search(nomen, default_channel, args).await,
        "nomen_entities" => tool_entities(nomen, default_channel, args).await,
        "nomen_consolidate" => tool_consolidate(nomen, default_channel, args).await,
        "nomen_delete" => tool_delete(nomen, default_channel, args).await,
        "nomen_groups" => tool_groups(nomen, default_channel, args).await,
        "nomen_send" => tool_send(nomen, default_channel, args).await,
        "nomen_list" => tool_list(nomen, default_channel, args).await,
        "nomen_sync" => tool_sync(nomen, default_channel, args).await,
        "nomen_embed" => tool_embed(nomen, default_channel, args).await,
        "nomen_prune" => tool_prune(nomen, default_channel, args).await,
        _ => anyhow::bail!("Unknown tool: {tool_name}"),
    }
}

pub async fn tool_search(nomen: &Nomen, default_channel: &str, args: &Value) -> Result<String> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut tier = args.get("tier").and_then(|v| v.as_str()).map(String::from);
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let vector_weight = args.get("vector_weight").and_then(|v| v.as_f64()).unwrap_or(0.7) as f32;
    let text_weight = args.get("text_weight").and_then(|v| v.as_f64()).unwrap_or(0.3) as f32;
    let aggregate = args.get("aggregate").and_then(|v| v.as_bool()).unwrap_or(false);
    let graph_expand = args.get("graph_expand").and_then(|v| v.as_bool()).unwrap_or(false);
    let max_hops = args.get("max_hops").and_then(|v| v.as_u64()).unwrap_or(1) as usize;

    if query.is_empty() {
        anyhow::bail!("query parameter is required");
    }

    let (session_tier, session_scope) = resolve_session_from_args(nomen, default_channel, args)?;
    if tier.is_none() {
        tier = session_tier;
    }
    let allowed_scopes = session_scope.map(|s| vec![s]);

    let opts = search::SearchOptions {
        query,
        tier,
        allowed_scopes,
        limit,
        vector_weight,
        text_weight,
        aggregate,
        graph_expand,
        max_hops,
        ..Default::default()
    };

    let results = nomen.search(opts).await?;

    if results.is_empty() {
        return Ok("No results found.".to_string());
    }

    let mut output = Vec::new();
    for (i, r) in results.iter().enumerate() {
        let contradicts_prefix = if r.contradicts { "[CONTRADICTS] " } else { "" };
        let graph_suffix = match r.graph_edge {
            Some(ref edge) => format!(", via: {edge}"),
            None => String::new(),
        };
        output.push(format!(
            "{}. [{}] {}{} (match: {:?}{})\n   {}",
            i + 1,
            r.visibility,
            contradicts_prefix,
            r.topic,
            r.match_type,
            graph_suffix,
            crate::memory::first_line(&r.detail)
        ));
    }

    Ok(format!(
        "Found {} results:\n\n{}",
        results.len(),
        output.join("\n\n")
    ))
}

pub async fn tool_store(nomen: &Nomen, default_channel: &str, args: &Value) -> Result<String> {
    let topic = args
        .get("topic")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let summary = args
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let detail = args
        .get("detail")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut tier = args.get("tier").and_then(|v| v.as_str()).map(String::from);
    let confidence = args
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.8);

    let (session_tier, _session_scope) = resolve_session_from_args(nomen, default_channel, args)?;
    if tier.is_none() {
        tier = session_tier;
    }
    let tier = tier.unwrap_or_else(|| "public".to_string());

    if topic.is_empty() || summary.is_empty() {
        anyhow::bail!("topic and summary are required");
    }

    let mem = crate::NewMemory {
        topic: topic.clone(),
        summary,
        detail,
        tier: tier.clone(),
        confidence,
        source: Some("mcp".to_string()),
        model: Some("mcp/agent".to_string()),
    };

    nomen.store(mem).await?;

    Ok(format!(
        "Stored memory: {topic} [{tier}] (confidence: {confidence:.2})"
    ))
}

pub async fn tool_ingest(nomen: &Nomen, _default_channel: &str, args: &Value) -> Result<String> {
    let source = args
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("mcp")
        .to_string();
    let sender = args
        .get("sender")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let channel = args
        .get("channel")
        .and_then(|v| v.as_str())
        .map(String::from);
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let metadata = args.get("metadata").map(|v| v.to_string());

    if content.is_empty() {
        anyhow::bail!("content is required");
    }

    let msg = ingest::RawMessage {
        source: source.clone(),
        source_id: None,
        sender: sender.clone(),
        channel: channel.clone(),
        content,
        metadata,
        created_at: None,
        ..Default::default()
    };

    let id = nomen.ingest_message(msg).await?;

    Ok(format!(
        "Ingested message from {sender} [{source}]{} (id: {id})",
        channel
            .as_deref()
            .map(|c| format!(" #{c}"))
            .unwrap_or_default()
    ))
}

pub async fn tool_messages(nomen: &Nomen, _default_channel: &str, args: &Value) -> Result<String> {
    let opts = ingest::MessageQuery {
        source: args.get("source").and_then(|v| v.as_str()).map(String::from),
        channel: args.get("channel").and_then(|v| v.as_str()).map(String::from),
        sender: args.get("sender").and_then(|v| v.as_str()).map(String::from),
        since: args.get("since").and_then(|v| v.as_str()).map(String::from),
        until: args.get("until").and_then(|v| v.as_str()).map(String::from),
        room: args.get("room").and_then(|v| v.as_str()).map(String::from),
        topic: args.get("topic").and_then(|v| v.as_str()).map(String::from),
        thread: args.get("thread").and_then(|v| v.as_str()).map(String::from),
        limit: Some(args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize),
        include_consolidated: args.get("include_consolidated").and_then(|v| v.as_bool()).unwrap_or(false),
        order: args.get("order").and_then(|v| v.as_str()).map(String::from),
    };

    let messages = nomen.get_messages(opts).await?;

    if messages.is_empty() {
        return Ok("No messages found.".to_string());
    }

    let mut output = Vec::new();
    for msg in &messages {
        let channel_str = if msg.channel.is_empty() {
            String::new()
        } else {
            format!(" #{}", msg.channel)
        };
        output.push(format!(
            "[{}] {}{}: {}\n  {}",
            msg.source, msg.sender, channel_str, msg.content, msg.created_at
        ));
    }

    Ok(format!(
        "{} messages:\n\n{}",
        messages.len(),
        output.join("\n\n")
    ))
}

pub async fn tool_message_search(nomen: &Nomen, _default_channel: &str, args: &Value) -> Result<String> {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
    if query.is_empty() {
        anyhow::bail!("query parameter is required");
    }

    let source = args.get("source").and_then(|v| v.as_str());
    let room = args.get("room").and_then(|v| v.as_str());
    let topic = args.get("topic").and_then(|v| v.as_str());
    let sender = args.get("sender").and_then(|v| v.as_str());
    let since = args.get("since").and_then(|v| v.as_str());
    let until = args.get("until").and_then(|v| v.as_str());
    let include_consolidated = args.get("include_consolidated").and_then(|v| v.as_bool()).unwrap_or(false);
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

    let results = nomen.search_messages(
        query, source, room, topic, sender, since, until, include_consolidated, limit,
    ).await?;

    if results.is_empty() {
        return Ok("No messages found.".to_string());
    }

    let mut output = Vec::new();
    for msg in &results {
        let channel_str = if msg.channel.is_empty() {
            String::new()
        } else {
            format!(" #{}", msg.channel)
        };
        output.push(format!(
            "[{}] {}{}: {} (score: {:.2})\n  {}",
            msg.source, msg.sender, channel_str, msg.content, msg.score, msg.created_at
        ));
    }

    Ok(format!(
        "{} messages:\n\n{}",
        results.len(),
        output.join("\n\n")
    ))
}

pub async fn tool_entities(nomen: &Nomen, _default_channel: &str, args: &Value) -> Result<String> {
    let kind_filter = args.get("kind").and_then(|v| v.as_str());
    let kind = kind_filter.and_then(entities::EntityKind::from_str);

    if kind_filter.is_some() && kind.is_none() {
        anyhow::bail!(
            "Unknown entity kind. Valid: person, project, concept, place, organization"
        );
    }

    let entity_list = nomen.entities(kind_filter).await?;

    if entity_list.is_empty() {
        return Ok("No entities found.".to_string());
    }

    let query = args.get("query").and_then(|v| v.as_str());
    let filtered: Vec<_> = if let Some(q) = query {
        let q_lower = q.to_lowercase();
        entity_list
            .iter()
            .filter(|e| e.name.to_lowercase().contains(&q_lower))
            .collect()
    } else {
        entity_list.iter().collect()
    };

    if filtered.is_empty() {
        return Ok("No matching entities found.".to_string());
    }

    let mut output = Vec::new();
    for e in &filtered {
        output.push(format!(
            "{} [{}] (created: {})",
            e.name, e.kind, e.created_at
        ));
    }

    Ok(format!(
        "{} entities:\n{}",
        filtered.len(),
        output.join("\n")
    ))
}

pub async fn tool_consolidate(nomen: &Nomen, _default_channel: &str, _args: &Value) -> Result<String> {
    let opts = crate::ConsolidateOptions::default();
    let report = nomen.consolidate(opts).await?;

    if report.memories_created == 0 {
        Ok("Nothing to consolidate.".to_string())
    } else {
        Ok(format!(
            "Consolidated {} messages into {} memories. Channels: {}",
            report.messages_processed,
            report.memories_created,
            if report.channels.is_empty() {
                "(none)".to_string()
            } else {
                report.channels.join(", ")
            }
        ))
    }
}

pub async fn tool_delete(nomen: &Nomen, _default_channel: &str, args: &Value) -> Result<String> {
    let topic = args.get("topic").and_then(|v| v.as_str());
    let id = args.get("id").and_then(|v| v.as_str());

    if topic.is_none() && id.is_none() {
        anyhow::bail!("Provide either topic or id");
    }

    nomen.delete(topic, id).await?;

    if let Some(topic) = topic {
        Ok(format!("Deleted memory with topic: {topic}"))
    } else {
        Ok(format!("Deleted memory with id: {}", id.unwrap()))
    }
}

pub async fn tool_groups(nomen: &Nomen, _default_channel: &str, args: &Value) -> Result<String> {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");

    match action {
        "list" => {
            let group_list = nomen.group_list().await?;
            if group_list.is_empty() {
                return Ok("No groups found.".to_string());
            }
            let mut output = Vec::new();
            for g in &group_list {
                let members_str = if g.members.is_empty() {
                    "(no members)".to_string()
                } else {
                    format!("{} member(s)", g.members.len())
                };
                output.push(format!("{} — {} [{}]", g.id, g.name, members_str));
            }
            Ok(format!(
                "{} groups:\n{}",
                group_list.len(),
                output.join("\n")
            ))
        }
        "members" => {
            let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if id.is_empty() {
                anyhow::bail!("id is required for members action");
            }
            let members = nomen.group_members(id).await?;
            if members.is_empty() {
                Ok(format!("Group {id} has no members."))
            } else {
                Ok(format!(
                    "{} members of {id}:\n{}",
                    members.len(),
                    members.join("\n")
                ))
            }
        }
        "create" => {
            let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if id.is_empty() || name.is_empty() {
                anyhow::bail!("id and name are required for create action");
            }
            let members: Vec<String> = args
                .get("members")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let nostr_group = args.get("nostr_group").and_then(|v| v.as_str());
            let relay = args.get("relay").and_then(|v| v.as_str());

            nomen.group_create(id, name, &members, nostr_group, relay).await?;
            Ok(format!("Created group: {id} ({name})"))
        }
        "add_member" => {
            let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let npub = args.get("npub").and_then(|v| v.as_str()).unwrap_or("");
            if id.is_empty() || npub.is_empty() {
                anyhow::bail!("id and npub are required for add_member action");
            }
            nomen.group_add_member(id, npub).await?;
            Ok(format!("Added {npub} to group {id}"))
        }
        "remove_member" => {
            let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let npub = args.get("npub").and_then(|v| v.as_str()).unwrap_or("");
            if id.is_empty() || npub.is_empty() {
                anyhow::bail!("id and npub are required for remove_member action");
            }
            nomen.group_remove_member(id, npub).await?;
            Ok(format!("Removed {npub} from group {id}"))
        }
        _ => anyhow::bail!(
            "Unknown action: {action}. Valid: list, members, create, add_member, remove_member"
        ),
    }
}

pub async fn tool_send(nomen: &Nomen, _default_channel: &str, args: &Value) -> Result<String> {
    let recipient = args.get("recipient").and_then(|v| v.as_str()).unwrap_or("");
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let channel = args
        .get("channel")
        .and_then(|v| v.as_str())
        .map(String::from);
    let metadata = args.get("metadata").cloned();

    if recipient.is_empty() || content.is_empty() {
        anyhow::bail!("recipient and content are required");
    }

    let target = send::parse_recipient(recipient)?;
    let opts = send::SendOptions {
        target,
        content: content.to_string(),
        channel,
        metadata,
    };

    let result = nomen.send(opts).await?;

    let accepted_count = result.accepted.len();
    let rejected_count = result.rejected.len();
    Ok(format!(
        "Sent to {recipient}: event_id={}, accepted={accepted_count}, rejected={rejected_count}",
        result.event_id
    ))
}

pub async fn tool_list(nomen: &Nomen, _default_channel: &str, args: &Value) -> Result<String> {
    let tier = args.get("tier").and_then(|v| v.as_str()).map(String::from);
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
    let include_stats = args.get("stats").and_then(|v| v.as_bool()).unwrap_or(false);

    let report = nomen.list(crate::ListOptions { tier, limit, include_stats }).await?;

    if report.memories.is_empty() && report.stats.is_none() {
        return Ok("No memories found.".to_string());
    }

    let mut output = Vec::new();

    if let Some(ref stats) = report.stats {
        output.push(format!(
            "Stats: {} total, {} named, {} pending ephemeral",
            stats.total, stats.named, stats.pending
        ));
    }

    for m in &report.memories {
        let summary = m.summary.as_deref().unwrap_or(&m.content);
        let summary_display = if summary.len() > 100 {
            format!("{}...", &summary[..100])
        } else {
            summary.to_string()
        };
        output.push(format!(
            "[{}] {} (v{}, confidence: {})\n   {}",
            m.tier,
            m.topic,
            m.version,
            m.confidence.map(|c| format!("{c:.2}")).unwrap_or("?".to_string()),
            summary_display
        ));
    }

    Ok(format!(
        "{} memories:\n\n{}",
        report.memories.len(),
        output.join("\n\n")
    ))
}

pub async fn tool_sync(nomen: &Nomen, _default_channel: &str, _args: &Value) -> Result<String> {
    let report = nomen.sync().await?;
    Ok(format!(
        "Sync complete: {} stored, {} skipped, {} errors",
        report.stored, report.skipped, report.errors
    ))
}

pub async fn tool_embed(nomen: &Nomen, _default_channel: &str, args: &Value) -> Result<String> {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
    let report = nomen.embed(limit).await?;

    if report.total == 0 {
        Ok("All memories already have embeddings.".to_string())
    } else {
        Ok(format!(
            "Embedded {} of {} memories",
            report.embedded, report.total
        ))
    }
}

pub async fn tool_prune(nomen: &Nomen, _default_channel: &str, args: &Value) -> Result<String> {
    let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(90);
    let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

    let report = nomen.prune(days, dry_run).await?;

    let prefix = if dry_run { "[DRY RUN] " } else { "" };
    Ok(format!(
        "{prefix}{} memories pruned, {} raw messages pruned",
        report.memories_pruned, report.raw_messages_pruned
    ))
}
