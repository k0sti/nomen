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

use nomen_api::NomenBackend;

// ── Rate limiter ────────────────────────────────────────────────────

pub(crate) struct RateLimiter {
    pub(crate) max_per_minute: u32,
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
    nomen: Box<dyn NomenBackend>,
    gateway: NostrMCPGateway,
    allowed_npubs: HashSet<String>,
    rate_limiter: RateLimiter,
    default_channel: String,
    announce: bool,
}

impl CvmServer {
    pub async fn new(
        nomen: Box<dyn NomenBackend>,
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
                Ok(event_id) => info!(%event_id, "CVM: published server announcement"),
                Err(e) => warn!("CVM: failed to publish server announcement: {e}"),
            }
        }

        info!(
            allowed_npubs = self.allowed_npubs.len(),
            rate_limit = self.rate_limiter.max_per_minute,
            "CVM server listening for requests"
        );

        while let Some(incoming) = rx.recv().await {
            let client_pubkey = &incoming.client_pubkey;
            let method = incoming.message.method().unwrap_or("<non-request>");

            info!(
                client = %client_pubkey,
                event_id = %incoming.event_id,
                method = %method,
                encrypted = incoming.is_encrypted,
                "CVM: incoming request"
            );

            // ACL check (skip if allowlist is empty = allow all)
            if !self.allowed_npubs.is_empty() && !self.allowed_npubs.contains(client_pubkey) {
                warn!(
                    client = %client_pubkey,
                    method = %method,
                    allowlist_size = self.allowed_npubs.len(),
                    "CVM: rejected — unauthorized npub"
                );
                let error_resp = make_error_response(
                    extract_request_id(&incoming.message),
                    -32600,
                    "Unauthorized",
                );
                if let Err(e) = self
                    .gateway
                    .send_response(&incoming.event_id, error_resp)
                    .await
                {
                    error!(event_id = %incoming.event_id, "CVM: failed to send error response: {e}");
                }
                continue;
            }

            // Rate limit check
            let now = nostr_sdk::Timestamp::now().as_u64();
            if !self.rate_limiter.check(client_pubkey, now) {
                warn!(
                    client = %client_pubkey,
                    method = %method,
                    "CVM: rejected — rate limit exceeded"
                );
                let error_resp = make_error_response(
                    extract_request_id(&incoming.message),
                    -32000,
                    "Rate limit exceeded",
                );
                if let Err(e) = self
                    .gateway
                    .send_response(&incoming.event_id, error_resp)
                    .await
                {
                    error!(event_id = %incoming.event_id, "CVM: failed to send rate-limit response: {e}");
                }
                continue;
            }

            debug!(
                client = %client_pubkey,
                event_id = %incoming.event_id,
                method = %method,
                "CVM: dispatching request"
            );

            let response = self.handle_message(&incoming.message).await;

            let is_error = matches!(&response, JsonRpcMessage::ErrorResponse(_));
            match self
                .gateway
                .send_response(&incoming.event_id, response)
                .await
            {
                Ok(()) => {
                    info!(
                        event_id = %incoming.event_id,
                        method = %method,
                        is_error = is_error,
                        "CVM: response sent successfully"
                    );
                }
                Err(e) => {
                    error!(
                        event_id = %incoming.event_id,
                        method = %method,
                        "CVM: failed to send response: {e}"
                    );
                }
            }
        }

        info!("CVM server shutting down");
        self.gateway.stop().await?;
        Ok(())
    }

    /// Handle a single JSON-RPC message. Public for testing.
    pub async fn handle_message(&self, message: &JsonRpcMessage) -> JsonRpcMessage {
        match message {
            JsonRpcMessage::Request(req) => {
                let method = &req.method;
                let params = req.params.clone().unwrap_or(Value::Null);
                let id = req.id.clone();

                debug!(method = %method, id = %id, "CVM: handling request method");

                match method.as_str() {
                    "initialize" => {
                        info!(method = "initialize", "CVM: returning server capabilities");
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
                        let tools = crate::mcp::v2_tools_list_value();
                        info!(method = "tools/list", "CVM: returning tool definitions");
                        make_success_response(id, tools)
                    }
                    "tools/call" => {
                        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                        info!(method = "tools/call", tool = %tool_name, "CVM: dispatching tool call");
                        self.handle_tool_call(id, &params).await
                    }
                    "ping" => {
                        debug!(method = "ping", "CVM: pong");
                        make_success_response(id, json!({}))
                    }
                    _ => {
                        // Direct v2 action dispatch (e.g. "memory.search", "group.list")
                        info!(action = %method, "CVM: direct action dispatch");
                        let api_resp = nomen_api::dispatch(
                            &*self.nomen,
                            &self.default_channel,
                            method,
                            &params,
                        )
                        .await;
                        let result = serde_json::to_value(&api_resp)
                            .unwrap_or_else(|_| json!({"ok": false}));
                        if !api_resp.ok {
                            warn!(action = %method, "CVM: action dispatch returned error");
                        }
                        make_success_response(id, result)
                    }
                }
            }
            JsonRpcMessage::Notification(notif) => {
                debug!(method = %notif.method, "CVM: received notification (no response needed)");
                make_success_response(Value::Null, json!({}))
            }
            _ => {
                warn!("CVM: received non-request/non-notification message");
                make_error_response(Value::Null, -32600, "Invalid request")
            }
        }
    }

    /// Handle a tools/call request. Public for testing.
    pub async fn handle_tool_call(&self, id: Value, params: &Value) -> JsonRpcMessage {
        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        // Map underscore tool name to dot action name
        let action = match nomen_api::mcp_tool_to_action(tool_name) {
            Some(a) => a,
            None => {
                warn!(tool = %tool_name, "CVM: unknown tool in tools/call");
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

        debug!(tool = %tool_name, action = %action, "CVM: tool_call → action dispatch");

        let api_resp =
            nomen_api::dispatch(&*self.nomen, &self.default_channel, &action, &arguments).await;

        if api_resp.ok {
            debug!(tool = %tool_name, action = %action, "CVM: tool call succeeded");
        } else {
            warn!(tool = %tool_name, action = %action, "CVM: tool call returned error");
        }

        let result_json = serde_json::to_value(&api_resp).unwrap_or_else(|_| json!({"ok": false}));

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

/// Standalone handler for CVM requests, decoupled from the gateway transport.
/// Used for testing and can be used by CvmServer internally.
pub struct CvmHandler {
    pub(crate) nomen: Box<dyn NomenBackend>,
    pub(crate) allowed_npubs: HashSet<String>,
    pub(crate) rate_limiter: RateLimiter,
    pub(crate) default_channel: String,
}

impl CvmHandler {
    /// Create a handler for testing — no relay or transport needed.
    pub fn new(
        nomen: Box<dyn NomenBackend>,
        allowed_npubs: Vec<String>,
        rate_limit: u32,
        default_channel: String,
    ) -> Self {
        Self {
            nomen,
            allowed_npubs: allowed_npubs.into_iter().collect(),
            rate_limiter: RateLimiter::new(rate_limit),
            default_channel,
        }
    }

    /// Handle a JSON-RPC message and return the response.
    pub async fn handle_message(&self, message: &JsonRpcMessage) -> JsonRpcMessage {
        match message {
            JsonRpcMessage::Request(req) => {
                let method = &req.method;
                let params = req.params.clone().unwrap_or(Value::Null);
                let id = req.id.clone();

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
                        let tools = crate::mcp::v2_tools_list_value();
                        make_success_response(id, tools)
                    }
                    "tools/call" => self.handle_tool_call(id, &params).await,
                    "ping" => make_success_response(id, json!({})),
                    _ => {
                        let api_resp = nomen_api::dispatch(
                            &*self.nomen,
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
            JsonRpcMessage::Notification(_) => make_success_response(Value::Null, json!({})),
            _ => make_error_response(Value::Null, -32600, "Invalid request"),
        }
    }

    async fn handle_tool_call(&self, id: Value, params: &Value) -> JsonRpcMessage {
        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let action = match nomen_api::mcp_tool_to_action(tool_name) {
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

        let api_resp =
            nomen_api::dispatch(&*self.nomen, &self.default_channel, &action, &arguments).await;

        let result_json = serde_json::to_value(&api_resp).unwrap_or_else(|_| json!({"ok": false}));

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

    /// Check if a client npub is allowed (empty allowlist = allow all).
    pub fn check_acl(&self, client_pubkey: &str) -> bool {
        self.allowed_npubs.is_empty() || self.allowed_npubs.contains(client_pubkey)
    }

    /// Check rate limit for a client.
    pub fn check_rate_limit(&self, client_pubkey: &str) -> bool {
        let now = nostr_sdk::Timestamp::now().as_u64();
        self.rate_limiter.check(client_pubkey, now)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

pub fn extract_request_id(message: &JsonRpcMessage) -> Value {
    match message {
        JsonRpcMessage::Request(req) => req.id.clone(),
        _ => Value::Null,
    }
}

pub fn make_success_response(id: Value, result: Value) -> JsonRpcMessage {
    JsonRpcMessage::Response(contextvm_sdk::JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result,
    })
}

pub fn make_error_response(id: Value, code: i64, message: &str) -> JsonRpcMessage {
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
