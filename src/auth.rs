//! NIP-98 HTTP Auth — verify `Authorization: Nostr <base64>` headers.
//!
//! Implements the server-side of NIP-98: parse the header, decode the base64 event,
//! verify the signature, check kind/url/method/timestamp, and extract the caller pubkey.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use nostr_sdk::prelude::*;
use serde_json::json;
use tracing::{debug, warn};

use crate::groups::GroupStore;

/// The caller's authenticated identity attached to each request.
#[derive(Debug, Clone)]
pub struct CallerContext {
    pub role: CallerRole,
    /// Hex pubkey of the authenticated caller (None for anonymous).
    pub pubkey: Option<String>,
}

/// Caller role determined from auth + config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallerRole {
    /// Nomen instance owner (full access).
    Owner,
    /// Authenticated group member (read public + own groups).
    Member,
    /// No auth or unrecognized pubkey (public only).
    Anonymous,
}

impl CallerContext {
    pub fn owner(pubkey: String) -> Self {
        Self {
            role: CallerRole::Owner,
            pubkey: Some(pubkey),
        }
    }

    pub fn member(pubkey: String) -> Self {
        Self {
            role: CallerRole::Member,
            pubkey: Some(pubkey),
        }
    }

    pub fn anonymous() -> Self {
        Self {
            role: CallerRole::Anonymous,
            pubkey: None,
        }
    }

    pub fn is_owner(&self) -> bool {
        self.role == CallerRole::Owner
    }

    pub fn is_anonymous(&self) -> bool {
        self.role == CallerRole::Anonymous
    }

    /// Check if this caller can perform a write action.
    pub fn can_write(&self) -> bool {
        self.role == CallerRole::Owner
    }

    /// Get the allowed visibility tiers for read operations.
    pub fn allowed_visibilities(&self) -> Vec<&'static str> {
        match self.role {
            CallerRole::Owner => vec!["public", "group", "personal", "internal", "private"],
            CallerRole::Member => vec!["public", "group"],
            CallerRole::Anonymous => vec!["public"],
        }
    }
}

/// Config for NIP-98 auth (maps to `[auth]` in config.toml).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AuthConfig {
    /// Hex or npub of the Nomen instance owner.
    #[serde(default)]
    pub owner_pubkey: Option<String>,
    /// Skip auth for localhost connections (default: true).
    #[serde(default = "default_true")]
    pub local_bypass: bool,
    /// Max age of NIP-98 event in seconds (default: 60).
    #[serde(default = "default_auth_window")]
    pub auth_window_secs: u64,
}

fn default_true() -> bool {
    true
}

fn default_auth_window() -> u64 {
    60
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            owner_pubkey: None,
            local_bypass: true,
            auth_window_secs: 60,
        }
    }
}

impl AuthConfig {
    /// Resolve owner_pubkey to hex, handling npub/hex input.
    pub fn owner_pubkey_hex(&self) -> Option<String> {
        let pk = self.owner_pubkey.as_deref()?;
        if pk.starts_with("npub1") {
            PublicKey::from_bech32(pk).ok().map(|pk| pk.to_hex())
        } else {
            Some(pk.to_string())
        }
    }
}

/// Axum middleware: extract NIP-98 auth from request, attach CallerContext.
pub async fn nip98_middleware(
    State(state): State<Arc<crate::http::AppState>>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let config = state.config.read().await;
    let mut auth_config = config.auth.clone().unwrap_or_default();
    // Fall back to top-level `owner` field if auth.owner_pubkey not set
    if auth_config.owner_pubkey.is_none() {
        auth_config.owner_pubkey = config.owner.clone();
    }
    drop(config);

    // Local bypass: localhost connections get owner access,
    // but NOT when traffic arrives via reverse proxy (Cloudflare tunnel etc.)
    if auth_config.local_bypass {
        let is_local = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip().is_loopback())
            .unwrap_or(false);

        // If a proxy header is present, the request is forwarded — not truly local
        let is_proxied = req.headers().contains_key("cf-connecting-ip")
            || req.headers().contains_key("x-forwarded-for");

        if is_local && !is_proxied {
            let owner_pk = auth_config
                .owner_pubkey_hex()
                .or_else(|| state.nomen.signer().map(|s| s.public_key().to_hex()))
                .unwrap_or_default();
            debug!("Local bypass: granting owner access");
            req.extensions_mut()
                .insert(CallerContext::owner(owner_pk));
            return next.run(req).await;
        }
    }

    // Extract Authorization header
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let caller = match auth_header.as_deref() {
        Some(header) if header.starts_with("Nostr ") => {
            let base64_event = &header[6..];
            match verify_nip98(base64_event, &req, &auth_config, state.nomen.groups()) {
                Ok(ctx) => ctx,
                Err(msg) => {
                    warn!("NIP-98 auth failed: {}", msg);
                    return unauthorized_response(&msg);
                }
            }
        }
        _ => CallerContext::anonymous(),
    };

    debug!("Caller: {:?}", caller.role);
    req.extensions_mut().insert(caller);
    next.run(req).await
}

/// Verify a NIP-98 base64-encoded event against the request.
fn verify_nip98(
    base64_event: &str,
    req: &Request<Body>,
    auth_config: &AuthConfig,
    groups: &GroupStore,
) -> Result<CallerContext, String> {
    // 1. Decode base64
    let json_bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_event)
        .map_err(|e| format!("Invalid base64: {e}"))?;

    let json_str =
        std::str::from_utf8(&json_bytes).map_err(|e| format!("Invalid UTF-8: {e}"))?;

    // 2. Parse as Nostr event and verify signature
    let event: Event =
        Event::from_json(json_str).map_err(|e| format!("Invalid Nostr event: {e}"))?;

    event
        .verify()
        .map_err(|e| format!("Invalid event signature: {e}"))?;

    // 3. Check kind == 27235
    if event.kind != Kind::HttpAuth {
        return Err(format!("Expected kind 27235, got {}", event.kind.as_u16()));
    }

    // 4. Check created_at within window
    let now = Timestamp::now().as_u64();
    let event_ts = event.created_at.as_u64();
    let diff = if now > event_ts {
        now - event_ts
    } else {
        event_ts - now
    };
    if diff > auth_config.auth_window_secs {
        return Err(format!(
            "Event timestamp too old: {}s difference (max {}s)",
            diff, auth_config.auth_window_secs
        ));
    }

    // 5. Check `u` tag matches request URL
    let event_url = event
        .tags
        .iter()
        .find(|t| t.as_slice().first().map(|s| s.as_str()) == Some("u"))
        .and_then(|t| t.as_slice().get(1).map(|s| s.to_string()))
        .ok_or("Missing 'u' tag")?;

    let request_url = reconstruct_url(req);
    if event_url != request_url {
        return Err(format!(
            "URL mismatch: event has '{}', request is '{}'",
            event_url, request_url
        ));
    }

    // 6. Check `method` tag matches
    let event_method = event
        .tags
        .iter()
        .find(|t| t.as_slice().first().map(|s| s.as_str()) == Some("method"))
        .and_then(|t| t.as_slice().get(1).map(|s| s.to_string()))
        .ok_or("Missing 'method' tag")?;

    let request_method = req.method().as_str();
    if !event_method.eq_ignore_ascii_case(request_method) {
        return Err(format!(
            "Method mismatch: event has '{}', request is '{}'",
            event_method, request_method
        ));
    }

    // 7. Determine role from pubkey
    let pubkey_hex = event.pubkey.to_hex();
    let caller = determine_role(&pubkey_hex, auth_config, groups);

    Ok(caller)
}

/// Determine the caller's role based on their pubkey.
fn determine_role(
    pubkey_hex: &str,
    auth_config: &AuthConfig,
    groups: &GroupStore,
) -> CallerContext {
    // Check if owner
    if let Some(ref owner_hex) = auth_config.owner_pubkey_hex() {
        if pubkey_hex == owner_hex {
            return CallerContext::owner(pubkey_hex.to_string());
        }
    }

    // Check if group member (using npub format for GroupStore compatibility)
    let npub = PublicKey::from_hex(pubkey_hex)
        .ok()
        .and_then(|pk| pk.to_bech32().ok())
        .unwrap_or_default();

    if !npub.is_empty() {
        let scopes = groups.expand_scopes(&npub);
        if !scopes.is_empty() {
            return CallerContext::member(pubkey_hex.to_string());
        }
    }

    // Authenticated but not owner or member — treat as anonymous
    CallerContext::anonymous()
}

/// Reconstruct the full URL from the request for NIP-98 `u` tag comparison.
fn reconstruct_url(req: &Request<Body>) -> String {
    let scheme = req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");

    let host = req
        .headers()
        .get("x-forwarded-host")
        .or_else(|| req.headers().get("host"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");

    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(req.uri().path());

    format!("{scheme}://{host}{path_and_query}")
}

fn unauthorized_response(msg: &str) -> Response {
    let body = json!({
        "ok": false,
        "error": {
            "code": "unauthorized",
            "message": msg,
        },
        "meta": { "version": "v2" }
    });
    (
        StatusCode::UNAUTHORIZED,
        [("content-type", "application/json")],
        serde_json::to_string(&body).unwrap_or_default(),
    )
        .into_response()
}

// ── Action-level permission checks ──────────────────────────────────

/// Actions that require owner access (write/admin operations).
const OWNER_ONLY_ACTIONS: &[&str] = &[
    "memory.put",
    "memory.delete",
    "memory.pin",
    "memory.unpin",
    "memory.sync",
    "memory.consolidate",
    "memory.consolidate_prepare",
    "memory.consolidate_commit",
    "memory.cluster",
    "memory.embed",
    "memory.publish",
    "memory.migrate_dtags",
    "memory.prune",
    "message.ingest",
    "message.send",
    "group.create",
    "group.add_member",
    "group.remove_member",
];

/// Check if a caller is allowed to execute the given action.
pub fn check_action_permission(action: &str, caller: &CallerContext) -> Result<(), String> {
    if caller.is_owner() {
        return Ok(());
    }

    if OWNER_ONLY_ACTIONS.contains(&action) {
        return Err(format!("Action '{action}' requires owner access"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_caller_context_roles() {
        let owner = CallerContext::owner("abc123".to_string());
        assert!(owner.is_owner());
        assert!(owner.can_write());
        assert_eq!(owner.allowed_visibilities().len(), 5);

        let member = CallerContext::member("def456".to_string());
        assert!(!member.is_owner());
        assert!(!member.can_write());
        assert_eq!(member.allowed_visibilities(), vec!["public", "group"]);

        let anon = CallerContext::anonymous();
        assert!(anon.is_anonymous());
        assert!(!anon.can_write());
        assert_eq!(anon.allowed_visibilities(), vec!["public"]);
    }

    #[test]
    fn test_action_permissions() {
        let owner = CallerContext::owner("abc".to_string());
        assert!(check_action_permission("memory.put", &owner).is_ok());
        assert!(check_action_permission("memory.search", &owner).is_ok());

        let anon = CallerContext::anonymous();
        assert!(check_action_permission("memory.search", &anon).is_ok());
        assert!(check_action_permission("memory.put", &anon).is_err());
        assert!(check_action_permission("memory.delete", &anon).is_err());
        assert!(check_action_permission("memory.sync", &anon).is_err());

        let member = CallerContext::member("def".to_string());
        assert!(check_action_permission("memory.search", &member).is_ok());
        assert!(check_action_permission("memory.list", &member).is_ok());
        assert!(check_action_permission("memory.put", &member).is_err());
    }

    #[test]
    fn test_auth_config_owner_hex() {
        let cfg = AuthConfig {
            owner_pubkey: Some(
                "63fe6318dc58583cfe16810f86dd09e18bfd76aabc24a0081ce2856f330504ed".to_string(),
            ),
            ..Default::default()
        };
        assert_eq!(
            cfg.owner_pubkey_hex().unwrap(),
            "63fe6318dc58583cfe16810f86dd09e18bfd76aabc24a0081ce2856f330504ed"
        );
    }

    #[test]
    fn test_auth_config_defaults() {
        let cfg = AuthConfig::default();
        assert!(cfg.local_bypass);
        assert_eq!(cfg.auth_window_secs, 60);
        assert!(cfg.owner_pubkey.is_none());
    }
}
