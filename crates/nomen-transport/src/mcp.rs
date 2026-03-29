//! MCP (Model Context Protocol) server implementation.
//!
//! Implements JSON-RPC 2.0 over stdio for MCP-compatible agents.
//! Routes all tool calls through the canonical `api::dispatch` layer.
//! Supports per-session identity via `identity_auth` tool call.

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, error, info};

use nomen_api::NomenBackend;
use nomen_core::signer::NomenSigner;

// ── JSON-RPC types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
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
                "name": "identity_auth",
                "description": "Authenticate this session with a Nostr nsec. All subsequent operations will use this identity for signing and encryption.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "nsec": { "type": "string", "description": "nsec1... bech32-encoded secret key" }
                    },
                    "required": ["nsec"]
                }
            },
            {
                "name": "memory_search",
                "description": "Search memories using hybrid semantic + full-text search with optional graph expansion",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" },
                        "visibility": { "type": "string", "description": "Filter by visibility (public/group/circle/personal/private)" },
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
                        "content": { "type": "string", "description": "Full memory content (plain text/markdown)" },
                        "visibility": { "type": "string", "description": "Visibility (public/group/circle/personal/private, default: public)" },
                        "scope": { "type": "string", "description": "Scope (required for group/circle)" },
                        "type": { "type": "string", "description": "Memory type (e.g. entity:person, entity:project, cluster)" },
                        "importance": { "type": "integer", "description": "Importance score 1-10" },
                        "rel": { "type": "array", "items": { "type": "array", "items": { "type": "string" } }, "description": "Relationship tags: [[d-tag, relation], ...]" },
                        "ref": { "type": "array", "items": { "type": "string" }, "description": "Reference d-tags of related memories" },
                        "mentions": { "type": "array", "items": { "type": "string" }, "description": "D-tags of entities mentioned in this memory" },
                        "metadata": { "type": "object", "description": "Arbitrary metadata" }
                    },
                    "required": ["topic", "content"]
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
                "description": "Ingest a collected message for later consolidation",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "content": { "type": "string", "description": "Message content" },
                        "source": { "type": "string", "description": "Source system" },
                        "sender": { "type": "string", "description": "Sender identifier" },
                        "platform": { "type": "string", "description": "Normalized platform namespace" },
                        "community_id": { "type": "string", "description": "Optional normalized community/container above chat" },
                        "chat_id": { "type": "string", "description": "Normalized chat identifier" },
                        "thread_id": { "type": "string", "description": "Optional normalized thread/topic identifier" },
                        "source_id": { "type": "string", "description": "Source-specific message ID" },
                        "metadata": { "type": "object", "description": "Arbitrary metadata" }
                    },
                    "required": ["content"]
                }
            },
            {
                "name": "message_query",
                "description": "Query messages with normalized hierarchy filters",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "#platform": { "type": "array", "items": { "type": "string" }, "description": "Filter by platform (e.g. telegram, discord)" },
                        "#community": { "type": "array", "items": { "type": "string" }, "description": "Filter by normalized community_id" },
                        "#chat": { "type": "array", "items": { "type": "string" }, "description": "Filter by normalized chat_id" },
                        "#thread": { "type": "array", "items": { "type": "string" }, "description": "Filter by normalized thread_id" },
                        "#sender": { "type": "array", "items": { "type": "string" }, "description": "Filter by sender identity" },
                        "since": { "type": "string", "description": "RFC3339 timestamp or unix timestamp" },
                        "until": { "type": "string", "description": "RFC3339 timestamp or unix timestamp" },
                        "limit": { "type": "integer", "description": "Max results (default 50)" }
                    }
                }
            },
            {
                "name": "message_context",
                "description": "Get recent conversation context using canonical hierarchy filters",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "#platform": { "type": "array", "items": { "type": "string" }, "description": "Filter by platform (e.g. telegram, discord)" },
                        "#community": { "type": "array", "items": { "type": "string" }, "description": "Filter by normalized community_id" },
                        "#chat": { "type": "array", "items": { "type": "string" }, "description": "Filter by normalized chat_id" },
                        "#thread": { "type": "array", "items": { "type": "string" }, "description": "Filter by normalized thread_id" },
                        "#sender": { "type": "array", "items": { "type": "string" }, "description": "Optional sender filter" },
                        "since": { "type": "integer", "description": "Lower bound unix timestamp" },
                        "before": { "type": "integer", "description": "Upper bound unix timestamp (exclusive-ish context cutoff)" },
                        "limit": { "type": "integer", "description": "Max messages (default 50)" }
                    },
                    "required": ["#chat"]
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
                "description": "Trigger consolidation of messages into memories",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "#platform": { "type": "array", "items": { "type": "string" }, "description": "Filter by platform (e.g. telegram, discord)" },
                        "#community": { "type": "array", "items": { "type": "string" }, "description": "Filter by normalized community_id" },
                        "#chat": { "type": "array", "items": { "type": "string" }, "description": "Filter by normalized chat_id" },
                        "#thread": { "type": "array", "items": { "type": "string" }, "description": "Filter by normalized thread_id" },
                        "since": { "type": "string", "description": "Only messages since (RFC3339)" },
                        "min_messages": { "type": "integer", "description": "Minimum messages to trigger (default 3)" },
                        "batch_size": { "type": "integer", "description": "Max messages per run (default 50)" },
                        "dry_run": { "type": "boolean", "description": "Preview without publishing" },
                        "older_than": { "type": "string", "description": "Duration filter (e.g. 30m, 1h)" }
                    }
                }
            },
            {
                "name": "memory_consolidate_prepare",
                "description": "Prepare consolidation batches for two-phase agent mode. Returns grouped message batches for external LLM processing.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "batch_size": { "type": "integer", "description": "Max messages per batch (default 50)" },
                        "min_messages": { "type": "integer", "description": "Min messages to trigger (default 3)" },
                        "ttl_minutes": { "type": "integer", "description": "Session TTL in minutes (default 60)" }
                    }
                }
            },
            {
                "name": "memory_consolidate_commit",
                "description": "Commit agent-provided extractions for a prepared consolidation session. Runs storage, graph edges, and cleanup.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id": { "type": "string", "description": "Session ID from consolidate_prepare" },
                        "extractions": {
                            "type": "array",
                            "description": "Array of batch extractions",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "batch_id": { "type": "string" },
                                    "memories": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "topic": { "type": "string" },
                                                "content": { "type": "string" },
                                                "importance": { "type": "integer" }
                                            },
                                            "required": ["topic", "content", "importance"]
                                        }
                                    }
                                },
                                "required": ["batch_id", "memories"]
                            }
                        }
                    },
                    "required": ["session_id", "extractions"]
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

pub struct McpServer {
    /// The base backend (global identity).
    base_nomen: Arc<dyn NomenBackend>,
    /// The active backend — either base or a SessionBackend with per-session signer.
    active_nomen: Arc<dyn NomenBackend>,
    /// Whether the client has authenticated via identity.auth.
    authenticated: bool,
}

impl McpServer {
    pub fn new(nomen: Arc<dyn NomenBackend>) -> Self {
        Self {
            base_nomen: nomen.clone(),
            active_nomen: nomen,
            authenticated: false,
        }
    }

    /// Get a reference to the active backend (for cross-transport testing).
    pub fn backend(&self) -> &dyn NomenBackend {
        &*self.active_nomen
    }

    pub async fn handle_request(&mut self, req: &JsonRpcRequest) -> JsonRpcResponse {
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

    pub async fn handle_tool_call(&mut self, id: Value, params: &Value) -> JsonRpcResponse {
        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        debug!(tool = tool_name, "MCP tool call");

        // Map underscore tool name to dot action name
        let action = match nomen_api::mcp_tool_to_action(tool_name) {
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

        // Intercept identity.auth to set up per-session signer
        if action == "identity.auth" {
            return self.handle_identity_auth(id, &arguments).await;
        }

        // Require authentication before any other action
        if !self.authenticated {
            return JsonRpcResponse::success(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": "{\"ok\":false,\"error\":{\"code\":\"auth_required\",\"message\":\"Authentication required. Call identity_auth first.\"}}"
                    }]
                }),
            );
        }

        let api_resp = nomen_api::dispatch(&*self.active_nomen, &action, &arguments).await;

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

    /// Handle identity.auth: parse nsec, create KeysSigner, wrap backend.
    async fn handle_identity_auth(&mut self, id: Value, arguments: &Value) -> JsonRpcResponse {
        // Use dispatch to validate the nsec and get the pubkey
        let api_resp = nomen_api::dispatch(&*self.base_nomen, "identity.auth", arguments).await;

        if !api_resp.ok {
            let result_json =
                serde_json::to_value(&api_resp).unwrap_or_else(|_| json!({"ok": false}));
            return JsonRpcResponse::success(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": result_json.to_string()
                    }]
                }),
            );
        }

        // Now create the session signer
        let nsec = arguments.get("nsec").and_then(|v| v.as_str()).unwrap_or("");
        match nostr_sdk::Keys::parse(nsec) {
            Ok(keys) => {
                let signer = Arc::new(nomen_relay::signer::KeysSigner::new(keys));
                let pubkey_hex = signer.public_key().to_hex();
                self.active_nomen = Arc::new(nomen_api::SessionBackend::new(
                    self.base_nomen.clone(),
                    signer,
                ));
                self.authenticated = true;
                info!(pubkey = %pubkey_hex, "MCP: session identity set");

                let result_json =
                    serde_json::to_value(&api_resp).unwrap_or_else(|_| json!({"ok": false}));
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
            Err(e) => {
                let err = json!({"ok": false, "error": {"code": "invalid_params", "message": format!("invalid nsec: {e}")}});
                JsonRpcResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": err.to_string()
                        }]
                    }),
                )
            }
        }
    }
}

// ── Stdio event loop ────────────────────────────────────────────────

/// Serve MCP over stdio with session identity support.
pub async fn serve_stdio_arc(nomen: Arc<dyn NomenBackend>) -> Result<()> {
    let mut server = McpServer::new(nomen);

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
