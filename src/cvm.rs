//! CVM (Context-VM) server: exposes nomen operations over Nostr via the ContextVM SDK.
//!
//! Uses the canonical `api::dispatch` layer for all operations.
//! ACL (allowed npubs) and rate limiting are kept as application-level middleware.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use anyhow::Result;
use contextvm_sdk::gateway::{GatewayConfig, NostrMCPGateway};
use contextvm_sdk::transport::server::NostrServerTransportConfig;
use contextvm_sdk::{EncryptionMode, JsonRpcMessage, ServerInfo};
use nostr_sdk::prelude::*;
use serde_json::{json, Value};
use tracing::{debug, error, info, warn};

use crate::Nomen;

// ── Rate limiter ────────────────────────────────────────────────────

struct RateLimiter {
    max_per_minute: u32,
    state: Mutex<HashMap<String, (u32, u64)>>,
}

impl RateLimiter {
    fn new(max_per_minute: u32) -> Self {
        Self {
            max_per_minute,
            state: Mutex::new(HashMap::new()),
        }
    }

    fn check(&self, npub: &str, now: u64) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let entry = state.entry(npub.to_string()).or_insert((0, now));
        if now - entry.1 >= 60 {
            entry.0 = 0;
            entry.1 = now;
        }
        if entry.0 >= self.max_per_minute {
            return false;
        }
        entry.0 += 1;
        true
    }
}

// ── CVM Server ──────────────────────────────────────────────────────

pub struct CvmServer {
    nomen: Nomen,
    gateway: NostrMCPGateway,
    allowed_npubs: HashSet<String>,
    rate_limiter: RateLimiter,
    default_channel: String,
    announce: bool,
}

impl CvmServer {
    pub async fn new(
        nomen: Nomen,
        keys: Keys,
        relay_url: &str,
        encryption: EncryptionMode,
        allowed_npubs: Vec<String>,
        rate_limit: u32,
        default_channel: String,
        announce: bool,
    ) -> Result<Self> {
        let server_info = ServerInfo {
            name: Some("nomen".to_string()),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            about: Some("Nostr-native agent memory system".to_string()),
            ..Default::default()
        };

        let transport_config = NostrServerTransportConfig {
            relay_urls: vec![relay_url.to_string()],
            encryption_mode: encryption,
            server_info: Some(server_info.clone()),
            allowed_public_keys: if allowed_npubs.is_empty() {
                vec![]
            } else {
                allowed_npubs
                    .iter()
                    .filter_map(|s| PublicKey::parse(s).ok())
                    .map(|pk| pk.to_hex())
                    .collect()
            },
            ..Default::default()
        };

        let gateway_config = GatewayConfig {
            nostr_config: transport_config,
        };

        let gateway = NostrMCPGateway::new(keys, gateway_config).await?;

        Ok(Self {
            nomen,
            gateway,
            allowed_npubs: allowed_npubs.into_iter().collect(),
            rate_limiter: RateLimiter::new(rate_limit),
            default_channel,
            announce,
        })
    }

    /// Run the CVM server event loop.
    pub async fn run(mut self) -> Result<()> {
        let mut rx = self.gateway.start().await?;

        if self.announce {
            match self.gateway.announce().await {
                Ok(event_id) => info!(%event_id, "Published CVM server announcement"),
                Err(e) => warn!("Failed to publish server announcement: {e}"),
            }
        }

        info!("CVM server listening for requests");

        while let Some(incoming) = rx.recv().await {
            let client_pubkey = &incoming.client_pubkey;

            // ACL check (skip if allowlist is empty = allow all)
            if !self.allowed_npubs.is_empty() && !self.allowed_npubs.contains(client_pubkey) {
                warn!(client = %client_pubkey, "Rejecting request from unauthorized npub");
                let error_resp = make_error_response(
                    extract_request_id(&incoming.message),
                    -32600,
                    "Unauthorized",
                );
                if let Err(e) = self.gateway.send_response(&incoming.event_id, error_resp).await {
                    error!("Failed to send error response: {e}");
                }
                continue;
            }

            // Rate limit check
            let now = nostr_sdk::Timestamp::now().as_u64();
            if !self.rate_limiter.check(client_pubkey, now) {
                warn!(client = %client_pubkey, "Rate-limited CVM request");
                let error_resp = make_error_response(
                    extract_request_id(&incoming.message),
                    -32000,
                    "Rate limit exceeded",
                );
                if let Err(e) = self.gateway.send_response(&incoming.event_id, error_resp).await {
                    error!("Failed to send rate limit response: {e}");
                }
                continue;
            }

            debug!(
                client = %client_pubkey,
                event_id = %incoming.event_id,
                encrypted = incoming.is_encrypted,
                "Processing CVM request"
            );

            let response = self.handle_message(&incoming.message).await;

            if let Err(e) = self.gateway.send_response(&incoming.event_id, response).await {
                error!(event_id = %incoming.event_id, "Failed to send CVM response: {e}");
            }
        }

        info!("CVM server shutting down");
        self.gateway.stop().await?;
        Ok(())
    }

    async fn handle_message(&self, message: &JsonRpcMessage) -> JsonRpcMessage {
        match message {
            JsonRpcMessage::Request(req) => {
                let method = &req.method;
                let params = req.params.clone().unwrap_or(Value::Null);
                let id = req.id.clone();

                debug!(method = %method, "CVM dispatching method");

                match method.as_str() {
                    "initialize" => {
                        let result = json!({
                            "protocolVersion": "2024-11-05",
                            "capabilities": {
                                "tools": {}
                            },
                            "serverInfo": {
                                "name": "nomen",
                                "version": env!("CARGO_PKG_VERSION")
                            }
                        });
                        make_success_response(id, result)
                    }
                    "tools/list" => {
                        // Return v2 tool definitions for MCP-over-CVM clients
                        let tools = crate::mcp::v2_tools_list_value();
                        make_success_response(id, tools)
                    }
                    "tools/call" => {
                        self.handle_tool_call(id, &params).await
                    }
                    "ping" => make_success_response(id, json!({})),
                    _ => {
                        // Direct v2 action dispatch (e.g. "memory.search", "group.list")
                        let api_resp = crate::api::dispatch(
                            &self.nomen,
                            &self.default_channel,
                            method,
                            &params,
                        )
                        .await;
                        let result = serde_json::to_value(&api_resp)
                            .unwrap_or_else(|_| json!({"ok": false}));
                        make_success_response(id, result)
                    }
                }
            }
            JsonRpcMessage::Notification(_) => {
                make_success_response(Value::Null, json!({}))
            }
            _ => {
                make_error_response(Value::Null, -32600, "Invalid request")
            }
        }
    }

    async fn handle_tool_call(&self, id: Value, params: &Value) -> JsonRpcMessage {
        let tool_name = params
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("");
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(json!({}));

        // Map underscore tool name to dot action name
        let action = match crate::api::dispatch::mcp_tool_to_action(tool_name) {
            Some(a) => a,
            None => {
                return make_success_response(
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

        let api_resp = crate::api::dispatch(
            &self.nomen,
            &self.default_channel,
            &action,
            &arguments,
        )
        .await;

        let result_json = serde_json::to_value(&api_resp)
            .unwrap_or_else(|_| json!({"ok": false}));

        make_success_response(
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

// ── Helpers ──────────────────────────────────────────────────────────

fn extract_request_id(message: &JsonRpcMessage) -> Value {
    match message {
        JsonRpcMessage::Request(req) => req.id.clone(),
        _ => Value::Null,
    }
}

fn make_success_response(id: Value, result: Value) -> JsonRpcMessage {
    JsonRpcMessage::Response(contextvm_sdk::JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result,
    })
}

fn make_error_response(id: Value, code: i64, message: &str) -> JsonRpcMessage {
    JsonRpcMessage::ErrorResponse(contextvm_sdk::JsonRpcErrorResponse {
        jsonrpc: "2.0".to_string(),
        id,
        error: contextvm_sdk::JsonRpcError {
            code,
            message: message.to_string(),
            data: None,
        },
    })
}
