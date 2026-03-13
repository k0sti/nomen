//! MCP (Model Context Protocol) server implementation.
//!
//! Implements JSON-RPC 2.0 over stdio for MCP-compatible agents.
//! Routes all tool calls through the canonical `api::dispatch` layer.

use std::io::{self, BufRead, Write};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, error, info};

use crate::Nomen;

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

/// V2 tool definitions as a JSON Value (for CVM tools/list).
pub fn v2_tools_list_value() -> Value {
    v2_tools_list()
}

/// V2 tool definitions with underscore naming for MCP compatibility.
fn v2_tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "memory_search",
                "description": "Search memories using hybrid semantic + full-text search with optional graph expansion",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" },
                        "visibility": { "type": "string", "description": "Filter by visibility (public/group/circle/personal/internal)" },
                        "scope": { "type": "string", "description": "Filter by scope" },
                        "limit": { "type": "integer", "description": "Max results (default 10)" },
                        "retrieval": {
                            "type": "object",
                            "description": "Search tuning parameters",
                            "properties": {
                                "vector_weight": { "type": "number", "description": "Vector similarity weight 0.0-1.0 (default 0.7)" },
                                "text_weight": { "type": "number", "description": "Full-text BM25 weight 0.0-1.0 (default 0.3)" },
                                "aggregate": { "type": "boolean", "description": "Merge similar results" },
                                "graph_expand": { "type": "boolean", "description": "Traverse graph edges" },
                                "max_hops": { "type": "integer", "description": "Max graph hops (default 1)" }
                            }
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "memory_put",
                "description": "Create or replace a named memory. Publishes to relay and stores locally.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string", "description": "Topic/namespace for the memory" },
                        "summary": { "type": "string", "description": "Short summary" },
                        "detail": { "type": "string", "description": "Full detail text" },
                        "visibility": { "type": "string", "description": "Visibility (public/group/circle/personal/internal, default: public)" },
                        "scope": { "type": "string", "description": "Scope (required for group/circle)" },
                        "confidence": { "type": "number", "description": "Confidence score 0.0-1.0 (default 0.8)" },
                        "metadata": { "type": "object", "description": "Arbitrary metadata" }
                    },
                    "required": ["topic", "summary"]
                }
            },
            {
                "name": "memory_get",
                "description": "Retrieve a single memory by topic or d_tag. Deterministic fetch, not search.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string", "description": "Topic to retrieve" },
                        "d_tag": { "type": "string", "description": "Direct d_tag lookup" },
                        "visibility": { "type": "string", "description": "For topic → d_tag resolution" },
                        "scope": { "type": "string", "description": "For topic → d_tag resolution" }
                    }
                }
            },
            {
                "name": "memory_list",
                "description": "List memories from local database with optional filters",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "visibility": { "type": "string", "description": "Filter by visibility" },
                        "scope": { "type": "string", "description": "Filter by scope" },
                        "limit": { "type": "integer", "description": "Max results (default 100)" },
                        "stats": { "type": "boolean", "description": "Include memory statistics" }
                    }
                }
            },
            {
                "name": "memory_delete",
                "description": "Delete a memory by topic, d_tag, or event ID. Removes from local DB and relay.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string", "description": "Topic to delete" },
                        "d_tag": { "type": "string", "description": "Direct d_tag lookup" },
                        "id": { "type": "string", "description": "Nostr event ID" },
                        "visibility": { "type": "string", "description": "For topic → d_tag resolution" },
                        "scope": { "type": "string", "description": "For topic → d_tag resolution" }
                    }
                }
            },
            {
                "name": "message_ingest",
                "description": "Ingest a raw message for later consolidation",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "content": { "type": "string", "description": "Message content" },
                        "source": { "type": "string", "description": "Source system" },
                        "sender": { "type": "string", "description": "Sender identifier" },
                        "channel": { "type": "string", "description": "Channel/room identity" },
                        "source_id": { "type": "string", "description": "Source-specific message ID" },
                        "metadata": { "type": "object", "description": "Arbitrary metadata" }
                    },
                    "required": ["content"]
                }
            },
            {
                "name": "message_list",
                "description": "Query raw messages with filters",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "description": "Filter by source" },
                        "channel": { "type": "string", "description": "Filter by channel" },
                        "sender": { "type": "string", "description": "Filter by sender" },
                        "since": { "type": "string", "description": "RFC3339 timestamp" },
                        "limit": { "type": "integer", "description": "Max results (default 50)" }
                    }
                }
            },
            {
                "name": "message_context",
                "description": "Get messages surrounding a specific message (context window)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "source_id": { "type": "string", "description": "Source message ID to center on" },
                        "before": { "type": "integer", "description": "Messages before (default 5)" },
                        "after": { "type": "integer", "description": "Messages after (default 5)" }
                    },
                    "required": ["source_id"]
                }
            },
            {
                "name": "message_send",
                "description": "Send a message to a recipient via relay",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "recipient": { "type": "string", "description": "npub1... for DM, group:<id> for group, 'public' for broadcast" },
                        "content": { "type": "string", "description": "Message body" },
                        "channel": { "type": "string", "description": "Delivery channel (default: nostr)" },
                        "metadata": { "type": "object", "description": "Platform-specific extras" }
                    },
                    "required": ["recipient", "content"]
                }
            },
            {
                "name": "entity_list",
                "description": "List or search extracted entities",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "description": "Filter by kind (person/project/concept/place/organization/technology)" },
                        "query": { "type": "string", "description": "Search query for entity names" }
                    }
                }
            },
            {
                "name": "entity_relationships",
                "description": "List entity relationships",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Filter by entity name" }
                    }
                }
            },
            {
                "name": "memory_consolidate",
                "description": "Trigger consolidation of raw messages into memories",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "channel": { "type": "string", "description": "Filter by channel" },
                        "since": { "type": "string", "description": "Only messages since (RFC3339)" },
                        "min_messages": { "type": "integer", "description": "Minimum messages to trigger (default 3)" },
                        "batch_size": { "type": "integer", "description": "Max messages per run (default 50)" },
                        "dry_run": { "type": "boolean", "description": "Preview without publishing" },
                        "older_than": { "type": "string", "description": "Duration filter (e.g. 30m, 1h)" }
                    }
                }
            },
            {
                "name": "memory_cluster",
                "description": "Synthesize related memories by namespace prefix",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "prefix": { "type": "string", "description": "Only fuse under this prefix" },
                        "min_members": { "type": "integer", "description": "Min memories per cluster (default 3)" },
                        "namespace_depth": { "type": "integer", "description": "Grouping depth (default 2)" },
                        "dry_run": { "type": "boolean", "description": "Preview without storing" }
                    }
                }
            },
            {
                "name": "memory_sync",
                "description": "Sync memories from relay to local database",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "memory_embed",
                "description": "Generate embeddings for memories that lack them",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer", "description": "Max memories to embed (default 100)" }
                    }
                }
            },
            {
                "name": "memory_prune",
                "description": "Prune old/unused memories and consolidated raw messages",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "days": { "type": "integer", "description": "Delete items older than N days (default 90)" },
                        "dry_run": { "type": "boolean", "description": "Preview without deleting" }
                    }
                }
            },
            {
                "name": "group_list",
                "description": "List all groups",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "group_members",
                "description": "Get members of a group",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Group ID" }
                    },
                    "required": ["id"]
                }
            },
            {
                "name": "group_create",
                "description": "Create a new group",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Group ID" },
                        "name": { "type": "string", "description": "Group name" },
                        "members": { "type": "array", "items": { "type": "string" }, "description": "Initial members" },
                        "nostr_group": { "type": "string", "description": "NIP-29 group ID" },
                        "relay": { "type": "string", "description": "Relay URL" }
                    },
                    "required": ["id", "name"]
                }
            },
            {
                "name": "group_add_member",
                "description": "Add a member to a group",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Group ID" },
                        "npub": { "type": "string", "description": "Member npub" }
                    },
                    "required": ["id", "npub"]
                }
            },
            {
                "name": "group_remove_member",
                "description": "Remove a member from a group",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Group ID" },
                        "npub": { "type": "string", "description": "Member npub" }
                    },
                    "required": ["id", "npub"]
                }
            }
        ]
    })
}

// ── MCP Server ──────────────────────────────────────────────────────

struct McpServer {
    nomen: Nomen,
    default_channel: String,
}

impl McpServer {
    async fn handle_request(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let id = req.id.clone().unwrap_or(Value::Null);

        match req.method.as_str() {
            "initialize" => JsonRpcResponse::success(id, server_info()),
            "notifications/initialized" => JsonRpcResponse::success(id, json!({})),
            "tools/list" => JsonRpcResponse::success(id, v2_tools_list()),
            "tools/call" => self.handle_tool_call(id, &req.params).await,
            "ping" => JsonRpcResponse::success(id, json!({})),
            _ => JsonRpcResponse::method_not_found(id, &req.method),
        }
    }

    async fn handle_tool_call(&self, id: Value, params: &Value) -> JsonRpcResponse {
        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        debug!(tool = tool_name, "MCP tool call");

        // Map underscore tool name to dot action name
        let action = match crate::api::dispatch::mcp_tool_to_action(tool_name) {
            Some(a) => a,
            None => {
                return JsonRpcResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Unknown tool: {tool_name}")
                        }],
                        "isError": true
                    }),
                );
            }
        };

        let api_resp =
            crate::api::dispatch(&self.nomen, &self.default_channel, &action, &arguments).await;

        let result_json = serde_json::to_value(&api_resp).unwrap_or_else(|_| json!({"ok": false}));

        JsonRpcResponse::success(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": result_json.to_string()
                }]
            }),
        )
    }
}

// ── Stdio event loop ────────────────────────────────────────────────

pub async fn serve_stdio(nomen: Nomen, default_channel: String) -> Result<()> {
    let server = McpServer {
        nomen,
        default_channel,
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
                let err_resp =
                    JsonRpcResponse::error(Value::Null, -32700, format!("Parse error: {e}"));
                let _ = write_response(&mut stdout, &err_resp);
                continue;
            }
        };

        // Notifications (no id) don't get responses
        let is_notification = req.id.is_none() || req.method.starts_with("notifications/");

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
