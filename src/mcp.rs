//! MCP (Model Context Protocol) server implementation.
//!
//! Implements JSON-RPC 2.0 over stdio for MCP-compatible agents.
//! Tool logic is shared via [`crate::tools`].

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
            "tools/list" => JsonRpcResponse::success(id, crate::tools::tools_list()),
            "tools/call" => self.handle_tool_call(id, &req.params).await,
            "ping" => JsonRpcResponse::success(id, json!({})),
            _ => JsonRpcResponse::method_not_found(id, &req.method),
        }
    }

    async fn handle_tool_call(&self, id: Value, params: &Value) -> JsonRpcResponse {
        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        debug!(tool = tool_name, "MCP tool call");

        let result = crate::tools::dispatch_tool(
            &self.nomen,
            &self.default_channel,
            tool_name,
            &arguments,
        )
        .await;

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
