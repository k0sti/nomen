//! MCP (Model Context Protocol) server implementation.
//!
//! Implements JSON-RPC 2.0 over stdio for MCP-compatible agents.

use std::io::{self, BufRead, Write};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tracing::{debug, error, info};

use crate::consolidate;
use crate::db;
use crate::embed::Embedder;
use crate::entities;
use crate::ingest;
use crate::relay::RelayManager;
use crate::search;

// ── JSON-RPC types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl JsonRpcResponse {
    fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Value, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
        }
    }

    fn method_not_found(id: Value, method: &str) -> Self {
        Self::error(id, -32601, format!("Method not found: {method}"))
    }
}

// ── MCP protocol types ──────────────────────────────────────────────

const SERVER_NAME: &str = "nomen";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

fn server_info() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION
        }
    })
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "nomen_search",
                "description": "Search memories using hybrid semantic + full-text search",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" },
                        "tier": { "type": "string", "description": "Filter by tier (public/group/private)" },
                        "scope": { "type": "string", "description": "Filter by scope" },
                        "limit": { "type": "integer", "description": "Max results (default 10)" }
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
                        "confidence": { "type": "number", "description": "Confidence score 0.0-1.0 (default 0.8)" }
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
                        "metadata": { "type": "object", "description": "Optional metadata" }
                    },
                    "required": ["source", "sender", "content"]
                }
            },
            {
                "name": "nomen_messages",
                "description": "Query raw messages",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "description": "Filter by source" },
                        "channel": { "type": "string", "description": "Filter by channel" },
                        "sender": { "type": "string", "description": "Filter by sender" },
                        "since": { "type": "string", "description": "Show messages since (RFC3339 timestamp)" },
                        "limit": { "type": "integer", "description": "Max results (default 50)" }
                    }
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
            }
        ]
    })
}

// ── MCP Server ──────────────────────────────────────────────────────

struct McpServer {
    db: Surreal<Db>,
    embedder: Box<dyn Embedder>,
    #[allow(dead_code)]
    relay: Option<RelayManager>,
}

impl McpServer {
    async fn handle_request(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let id = req.id.clone().unwrap_or(Value::Null);

        match req.method.as_str() {
            "initialize" => JsonRpcResponse::success(id, server_info()),
            "notifications/initialized" => {
                // No response needed for notifications, but we return success
                // since this has an id
                JsonRpcResponse::success(id, json!({}))
            }
            "tools/list" => JsonRpcResponse::success(id, tools_list()),
            "tools/call" => self.handle_tool_call(id, &req.params).await,
            "ping" => JsonRpcResponse::success(id, json!({})),
            _ => JsonRpcResponse::method_not_found(id, &req.method),
        }
    }

    async fn handle_tool_call(&self, id: Value, params: &Value) -> JsonRpcResponse {
        let tool_name = params
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        debug!(tool = tool_name, "MCP tool call");

        let result = match tool_name {
            "nomen_search" => self.tool_search(&arguments).await,
            "nomen_store" => self.tool_store(&arguments).await,
            "nomen_ingest" => self.tool_ingest(&arguments).await,
            "nomen_messages" => self.tool_messages(&arguments).await,
            "nomen_entities" => self.tool_entities(&arguments).await,
            "nomen_consolidate" => self.tool_consolidate(&arguments).await,
            "nomen_delete" => self.tool_delete(&arguments).await,
            _ => Err(anyhow::anyhow!("Unknown tool: {tool_name}")),
        };

        match result {
            Ok(content) => JsonRpcResponse::success(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": content
                    }]
                }),
            ),
            Err(e) => JsonRpcResponse::success(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Error: {e}")
                    }],
                    "isError": true
                }),
            ),
        }
    }

    // ── Tool implementations ────────────────────────────────────

    async fn tool_search(&self, args: &Value) -> Result<String> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let tier = args.get("tier").and_then(|v| v.as_str()).map(String::from);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        if query.is_empty() {
            anyhow::bail!("query parameter is required");
        }

        let opts = search::SearchOptions {
            query,
            tier,
            allowed_scopes: None,
            limit,
            vector_weight: 0.7,
            text_weight: 0.3,
            min_confidence: None,
        };

        let results = search::search(&self.db, self.embedder.as_ref(), &opts).await?;

        if results.is_empty() {
            return Ok("No results found.".to_string());
        }

        let mut output = Vec::new();
        for (i, r) in results.iter().enumerate() {
            output.push(format!(
                "{}. [{}] {} (confidence: {}, match: {:?})\n   {}",
                i + 1,
                r.tier,
                r.topic,
                r.confidence,
                r.match_type,
                r.summary
            ));
        }

        Ok(format!("Found {} results:\n\n{}", results.len(), output.join("\n\n")))
    }

    async fn tool_store(&self, args: &Value) -> Result<String> {
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
        let tier = args
            .get("tier")
            .and_then(|v| v.as_str())
            .unwrap_or("public")
            .to_string();
        let confidence = args
            .get("confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.8);

        if topic.is_empty() || summary.is_empty() {
            anyhow::bail!("topic and summary are required");
        }

        let content = serde_json::json!({
            "summary": summary,
            "detail": if detail.is_empty() { &summary } else { &detail },
            "context": null
        });

        let d_tag = format!("snow:memory:{topic}");

        let parsed = crate::memory::ParsedMemory {
            tier: tier.clone(),
            topic: topic.clone(),
            version: "1".to_string(),
            confidence: format!("{confidence:.2}"),
            model: "mcp/agent".to_string(),
            summary: summary.clone(),
            created_at: nostr_sdk::prelude::Timestamp::now(),
            d_tag,
            source: "mcp".to_string(),
            content_raw: content.to_string(),
            detail: if detail.is_empty() { summary.clone() } else { detail },
        };

        db::store_memory_direct(&self.db, &parsed, "mcp").await?;

        // Generate embedding if available
        if self.embedder.dimensions() > 0 {
            let text = format!("{} {}", parsed.summary, parsed.detail);
            if let Ok(embeddings) = self.embedder.embed(&[text]).await {
                if let Some(embedding) = embeddings.into_iter().next() {
                    let _ = db::store_embedding(&self.db, &parsed.d_tag, embedding).await;
                }
            }
        }

        Ok(format!("Stored memory: {topic} [{tier}] (confidence: {confidence:.2})"))
    }

    async fn tool_ingest(&self, args: &Value) -> Result<String> {
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
        };

        let id = ingest::ingest_message(&self.db, &msg).await?;

        Ok(format!(
            "Ingested message from {sender} [{source}]{} (id: {id})",
            channel
                .as_deref()
                .map(|c| format!(" #{c}"))
                .unwrap_or_default()
        ))
    }

    async fn tool_messages(&self, args: &Value) -> Result<String> {
        let opts = ingest::MessageQuery {
            source: args
                .get("source")
                .and_then(|v| v.as_str())
                .map(String::from),
            channel: args
                .get("channel")
                .and_then(|v| v.as_str())
                .map(String::from),
            sender: args
                .get("sender")
                .and_then(|v| v.as_str())
                .map(String::from),
            since: args
                .get("since")
                .and_then(|v| v.as_str())
                .map(String::from),
            limit: Some(
                args.get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50) as usize,
            ),
            consolidated_only: false,
        };

        let messages = ingest::get_messages(&self.db, &opts).await?;

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

        Ok(format!("{} messages:\n\n{}", messages.len(), output.join("\n\n")))
    }

    async fn tool_entities(&self, args: &Value) -> Result<String> {
        let kind_filter = args.get("kind").and_then(|v| v.as_str());
        let kind = kind_filter.and_then(entities::EntityKind::from_str);

        if kind_filter.is_some() && kind.is_none() {
            anyhow::bail!(
                "Unknown entity kind. Valid: person, project, concept, place, organization"
            );
        }

        let entity_list = db::list_entities(&self.db, kind.as_ref()).await?;

        if entity_list.is_empty() {
            return Ok("No entities found.".to_string());
        }

        // If query is provided, filter by name
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
            output.push(format!("{} [{}] (created: {})", e.name, e.kind, e.created_at));
        }

        Ok(format!("{} entities:\n{}", filtered.len(), output.join("\n")))
    }

    async fn tool_consolidate(&self, _args: &Value) -> Result<String> {
        let config = consolidate::ConsolidationConfig::default();
        let report =
            consolidate::consolidate(&self.db, self.embedder.as_ref(), &config).await?;

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

    async fn tool_delete(&self, args: &Value) -> Result<String> {
        let topic = args.get("topic").and_then(|v| v.as_str());
        let id = args.get("id").and_then(|v| v.as_str());

        if topic.is_none() && id.is_none() {
            anyhow::bail!("Provide either topic or id");
        }

        if let Some(topic) = topic {
            let d_tag = format!("snow:memory:{topic}");
            db::delete_memory_by_dtag(&self.db, &d_tag).await?;
            Ok(format!("Deleted memory with topic: {topic}"))
        } else {
            let id = id.unwrap();
            db::delete_memory_by_nostr_id(&self.db, id).await?;
            Ok(format!("Deleted memory with id: {id}"))
        }
    }
}

// ── Stdio event loop ────────────────────────────────────────────────

pub async fn serve_stdio(
    db: Surreal<Db>,
    embedder: Box<dyn Embedder>,
    relay: Option<RelayManager>,
) -> Result<()> {
    let server = McpServer {
        db,
        embedder,
        relay,
    };

    info!("Nomen MCP server starting on stdio");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to read stdin: {e}");
                break;
            }
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = JsonRpcResponse::error(
                    Value::Null,
                    -32700,
                    format!("Parse error: {e}"),
                );
                let _ = write_response(&mut stdout, &err_resp);
                continue;
            }
        };

        // Notifications (no id) don't get responses
        let is_notification = req.id.is_none()
            || req.method.starts_with("notifications/");

        let response = server.handle_request(&req).await;

        if !is_notification {
            write_response(&mut stdout, &response)?;
        }
    }

    info!("MCP server shutting down");
    Ok(())
}

fn write_response(stdout: &mut io::Stdout, response: &JsonRpcResponse) -> Result<()> {
    let json = serde_json::to_string(response)?;
    writeln!(stdout, "{json}")?;
    stdout.flush()?;
    Ok(())
}
