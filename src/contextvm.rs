//! Context-VM: Nostr-native request/response interface for agents.
//!
//! Protocol:
//!   Request:  kind 21900 (ephemeral) — NIP-44 encrypted JSON
//!   Response: kind 21901 (ephemeral) — NIP-44 encrypted JSON
//!
//! Request tags:  ["p", nomen_npub], ["t", "nomen-request"], ["expiration", unix+60]
//! Response tags: ["p", requester_npub], ["e", request_event_id], ["t", "nomen-response"]

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use anyhow::Result;
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, error, info, warn};

use crate::entities;
use crate::ingest;
use crate::search;
use crate::send;
use crate::Nomen;

const REQUEST_KIND: u16 = 21900;
const RESPONSE_KIND: u16 = 21901;

// ── Protocol types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ContextVmRequest {
    action: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct ContextVmResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl ContextVmResponse {
    fn ok(result: Value) -> Self {
        Self {
            result: Some(result),
            error: None,
        }
    }

    fn err(msg: impl Into<String>) -> Self {
        Self {
            result: None,
            error: Some(msg.into()),
        }
    }
}

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

// ── Server ──────────────────────────────────────────────────────────

pub struct ContextVmServer {
    nomen: Nomen,
    allowed_npubs: HashSet<String>,
    rate_limiter: RateLimiter,
    default_channel: String,
}

impl ContextVmServer {
    pub fn new(
        nomen: Nomen,
        allowed_npubs: Vec<String>,
        default_channel: String,
    ) -> Self {
        Self::with_rate_limit(nomen, allowed_npubs, 30, default_channel)
    }

    pub fn with_rate_limit(
        nomen: Nomen,
        allowed_npubs: Vec<String>,
        max_requests_per_minute: u32,
        default_channel: String,
    ) -> Self {
        Self {
            nomen,
            allowed_npubs: allowed_npubs.into_iter().collect(),
            rate_limiter: RateLimiter::new(max_requests_per_minute),
            default_channel,
        }
    }

    /// Subscribe to kind 21900 requests tagged with our npub and process them.
    pub async fn run(&self) -> Result<()> {
        let relay = self.nomen.relay()
            .ok_or_else(|| anyhow::anyhow!("No relay configured for Context-VM"))?;
        let our_pubkey = relay.public_key();
        info!(
            pubkey = %our_pubkey.to_bech32().unwrap_or_default(),
            "Context-VM listening for requests"
        );

        let filter = Filter::new()
            .kind(Kind::Custom(REQUEST_KIND))
            .custom_tag(SingleLetterTag::lowercase(Alphabet::P), our_pubkey.to_hex())
            .custom_tag(SingleLetterTag::lowercase(Alphabet::T), "nomen-request");

        relay.client().subscribe(filter, None).await?;

        relay
            .client()
            .handle_notifications(|notification| async {
                if let RelayPoolNotification::Event { event, .. } = notification {
                    if event.kind == Kind::Custom(REQUEST_KIND) {
                        if let Err(e) = self.handle_event(&event).await {
                            error!(event_id = %event.id, "Failed to handle request: {e}");
                        }
                    }
                }
                Ok(false)
            })
            .await?;

        Ok(())
    }

    async fn handle_event(&self, event: &Event) -> Result<()> {
        // Expiration check
        if let Some(expiration) = crate::memory::get_tag_value(&event.tags, "expiration") {
            if let Ok(exp_ts) = expiration.parse::<u64>() {
                if exp_ts < Timestamp::now().as_u64() {
                    warn!(event_id = %event.id, "Skipping expired Context-VM request");
                    return Ok(());
                }
            }
        }

        let requester = event.pubkey.to_hex();

        // Authorization check
        if !self.allowed_npubs.contains(&requester) {
            let requester_bech32 = event.pubkey.to_bech32().unwrap_or_default();
            if !self.allowed_npubs.contains(&requester_bech32) {
                warn!(requester = %requester, "Rejecting request from unauthorized npub");
                return Ok(());
            }
        }

        // Rate limit check
        if !self.rate_limiter.check(&requester, Timestamp::now().as_u64()) {
            warn!(requester = %requester, "Rate-limited Context-VM request");
            return Ok(());
        }

        debug!(requester = %requester, event_id = %event.id, "Processing Context-VM request");

        let relay = self.nomen.relay()
            .ok_or_else(|| anyhow::anyhow!("No relay for Context-VM"))?;

        // Decrypt content (NIP-44)
        let plaintext = relay
            .signer()
            .decrypt_from(&event.content, &event.pubkey)
            .map_err(|e| anyhow::anyhow!("NIP-44 decryption failed: {e}"))?;

        let request: ContextVmRequest = serde_json::from_str(&plaintext)
            .map_err(|e| anyhow::anyhow!("Invalid request JSON: {e}"))?;

        info!(action = %request.action, requester = %requester, "Dispatching action");

        let response = match request.action.as_str() {
            "search" => self.handle_search(&request.params).await,
            "store" => self.handle_store(&request.params).await,
            "ingest" => self.handle_ingest(&request.params).await,
            "entities" => self.handle_entities(&request.params).await,
            "consolidate" => self.handle_consolidate(&request.params).await,
            "messages" => self.handle_messages(&request.params).await,
            "groups" => self.handle_groups(&request.params).await,
            "send" => self.handle_send(&request.params).await,
            "delete" => self.handle_delete(&request.params).await,
            "list" => self.handle_list(&request.params).await,
            "sync" => self.handle_sync(&request.params).await,
            "embed" => self.handle_embed(&request.params).await,
            "prune" => self.handle_prune(&request.params).await,
            _ => Ok(ContextVmResponse::err(format!(
                "Unknown action: {}",
                request.action
            ))),
        }
        .unwrap_or_else(|e| ContextVmResponse::err(format!("Internal error: {e}")));

        self.send_response(event, &response).await?;
        Ok(())
    }

    async fn send_response(
        &self,
        request_event: &Event,
        response: &ContextVmResponse,
    ) -> Result<()> {
        let relay = self.nomen.relay()
            .ok_or_else(|| anyhow::anyhow!("No relay for Context-VM response"))?;

        let response_json = serde_json::to_string(response)?;

        let encrypted = relay
            .signer()
            .encrypt_to(&response_json, &request_event.pubkey)
            .map_err(|e| anyhow::anyhow!("NIP-44 encryption failed: {e}"))?;

        let expiration = Timestamp::from(Timestamp::now().as_u64() + 60);
        let tags = vec![
            Tag::public_key(request_event.pubkey),
            Tag::event(request_event.id),
            Tag::custom(
                TagKind::Custom("t".into()),
                vec!["nomen-response".to_string()],
            ),
            Tag::expiration(expiration),
        ];

        let builder = EventBuilder::new(Kind::Custom(RESPONSE_KIND), encrypted).tags(tags);

        let output = relay.publish(builder).await?;
        debug!(event_id = %output.event_id, "Published Context-VM response");

        Ok(())
    }

    // ── Action handlers ─────────────────────────────────────────

    async fn handle_search(&self, params: &Value) -> Result<ContextVmResponse> {
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if query.is_empty() {
            return Ok(ContextVmResponse::err("query parameter is required"));
        }

        let mut tier = params.get("tier").and_then(|v| v.as_str()).map(String::from);
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
        let scope = params.get("scope").and_then(|v| v.as_str()).map(String::from);
        let vector_weight = params.get("vector_weight").and_then(|v| v.as_f64()).unwrap_or(0.7) as f32;
        let text_weight = params.get("text_weight").and_then(|v| v.as_f64()).unwrap_or(0.3) as f32;
        let aggregate = params.get("aggregate").and_then(|v| v.as_bool()).unwrap_or(false);

        // Session ID support
        if let Some(sid) = params.get("session_id").and_then(|v| v.as_str()) {
            if tier.is_none() {
                if let Ok(resolved) = self.nomen.resolve_session(sid, &self.default_channel) {
                    tier = Some(resolved.tier);
                }
            }
        }

        let opts = search::SearchOptions {
            query,
            tier,
            allowed_scopes: scope.map(|s| vec![s]),
            limit,
            vector_weight,
            text_weight,
            aggregate,
            ..Default::default()
        };

        let results = self.nomen.search(opts).await?;

        let items: Vec<Value> = results
            .iter()
            .map(|r| {
                json!({
                    "tier": r.tier,
                    "topic": r.topic,
                    "confidence": r.confidence,
                    "summary": r.summary,
                    "score": r.score,
                    "match_type": format!("{:?}", r.match_type),
                })
            })
            .collect();

        Ok(ContextVmResponse::ok(json!({
            "count": items.len(),
            "results": items,
        })))
    }

    async fn handle_store(&self, params: &Value) -> Result<ContextVmResponse> {
        let topic = params
            .get("topic")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let summary = params
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if topic.is_empty() || summary.is_empty() {
            return Ok(ContextVmResponse::err("topic and summary are required"));
        }

        let detail = params
            .get("detail")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut tier = params
            .get("tier")
            .and_then(|v| v.as_str())
            .unwrap_or("public")
            .to_string();
        let confidence = params
            .get("confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.8);

        // Session ID support
        if let Some(sid) = params.get("session_id").and_then(|v| v.as_str()) {
            if tier == "public" {
                if let Ok(resolved) = self.nomen.resolve_session(sid, &self.default_channel) {
                    tier = resolved.tier;
                }
            }
        }

        let mem = crate::NewMemory {
            topic: topic.clone(),
            summary,
            detail,
            tier: tier.clone(),
            confidence,
            source: Some("contextvm".to_string()),
            model: Some("contextvm/agent".to_string()),
        };

        self.nomen.store(mem).await?;

        Ok(ContextVmResponse::ok(json!({
            "stored": true,
            "topic": topic,
            "tier": tier,
        })))
    }

    async fn handle_ingest(&self, params: &Value) -> Result<ContextVmResponse> {
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if content.is_empty() {
            return Ok(ContextVmResponse::err("content is required"));
        }

        let source = params
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("nostr")
            .to_string();
        let sender = params
            .get("sender")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let channel = params
            .get("channel")
            .and_then(|v| v.as_str())
            .map(String::from);

        let msg = ingest::RawMessage {
            source: source.clone(),
            source_id: None,
            sender: sender.clone(),
            channel,
            content,
            metadata: None,
            created_at: None,
        };

        let id = self.nomen.ingest_message(msg).await?;

        Ok(ContextVmResponse::ok(json!({
            "ingested": true,
            "id": id,
            "source": source,
            "sender": sender,
        })))
    }

    async fn handle_entities(&self, params: &Value) -> Result<ContextVmResponse> {
        let kind_filter = params.get("kind").and_then(|v| v.as_str());
        let kind = kind_filter.and_then(entities::EntityKind::from_str);

        if kind_filter.is_some() && kind.is_none() {
            return Ok(ContextVmResponse::err(
                "Unknown entity kind. Valid: person, project, concept, place, organization",
            ));
        }

        let entity_list = self.nomen.entities(kind_filter).await?;

        let items: Vec<Value> = entity_list
            .iter()
            .map(|e| {
                json!({
                    "name": e.name,
                    "kind": e.kind,
                    "created_at": e.created_at,
                })
            })
            .collect();

        Ok(ContextVmResponse::ok(json!({
            "count": items.len(),
            "entities": items,
        })))
    }

    async fn handle_consolidate(&self, _params: &Value) -> Result<ContextVmResponse> {
        let opts = crate::ConsolidateOptions::default();
        let report = self.nomen.consolidate(opts).await?;

        Ok(ContextVmResponse::ok(json!({
            "messages_processed": report.messages_processed,
            "memories_created": report.memories_created,
            "channels": report.channels,
        })))
    }

    async fn handle_messages(&self, params: &Value) -> Result<ContextVmResponse> {
        let opts = ingest::MessageQuery {
            source: params
                .get("source")
                .and_then(|v| v.as_str())
                .map(String::from),
            channel: params
                .get("channel")
                .and_then(|v| v.as_str())
                .map(String::from),
            sender: params
                .get("sender")
                .and_then(|v| v.as_str())
                .map(String::from),
            since: params
                .get("since")
                .and_then(|v| v.as_str())
                .map(String::from),
            limit: Some(params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize),
            consolidated_only: false,
        };

        let messages = self.nomen.get_messages(opts).await?;

        let items: Vec<Value> = messages
            .iter()
            .map(|m| {
                json!({
                    "source": m.source,
                    "sender": m.sender,
                    "channel": m.channel,
                    "content": m.content,
                    "created_at": m.created_at,
                    "consolidated": m.consolidated,
                })
            })
            .collect();

        Ok(ContextVmResponse::ok(json!({
            "count": items.len(),
            "messages": items,
        })))
    }

    async fn handle_groups(&self, params: &Value) -> Result<ContextVmResponse> {
        let action = params.get("action").and_then(|v| v.as_str()).unwrap_or("list");

        match action {
            "list" => {
                let group_list = self.nomen.group_list().await?;
                let items: Vec<Value> = group_list
                    .iter()
                    .map(|g| {
                        json!({
                            "id": g.id,
                            "name": g.name,
                            "parent": g.parent,
                            "members": g.members,
                            "nostr_group": g.nostr_group,
                        })
                    })
                    .collect();

                Ok(ContextVmResponse::ok(json!({
                    "count": items.len(),
                    "groups": items,
                })))
            }
            "create" => {
                let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() || name.is_empty() {
                    return Ok(ContextVmResponse::err("id and name are required for create"));
                }
                let members: Vec<String> = params
                    .get("members")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let nostr_group = params.get("nostr_group").and_then(|v| v.as_str());
                let relay = params.get("relay").and_then(|v| v.as_str());

                self.nomen.group_create(id, name, &members, nostr_group, relay).await?;
                Ok(ContextVmResponse::ok(json!({ "created": true, "id": id })))
            }
            "members" => {
                let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    return Ok(ContextVmResponse::err("id is required for members"));
                }
                let members = self.nomen.group_members(id).await?;
                Ok(ContextVmResponse::ok(json!({
                    "group_id": id,
                    "count": members.len(),
                    "members": members,
                })))
            }
            "add_member" => {
                let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let npub = params.get("npub").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() || npub.is_empty() {
                    return Ok(ContextVmResponse::err("id and npub are required"));
                }
                self.nomen.group_add_member(id, npub).await?;
                Ok(ContextVmResponse::ok(json!({ "added": true, "group_id": id, "npub": npub })))
            }
            "remove_member" => {
                let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let npub = params.get("npub").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() || npub.is_empty() {
                    return Ok(ContextVmResponse::err("id and npub are required"));
                }
                self.nomen.group_remove_member(id, npub).await?;
                Ok(ContextVmResponse::ok(json!({ "removed": true, "group_id": id, "npub": npub })))
            }
            _ => Ok(ContextVmResponse::err(format!(
                "Unknown groups action: {action}. Valid: list, create, members, add_member, remove_member"
            ))),
        }
    }

    async fn handle_send(&self, params: &Value) -> Result<ContextVmResponse> {
        let recipient = params
            .get("recipient")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");

        if recipient.is_empty() || content.is_empty() {
            return Ok(ContextVmResponse::err("recipient and content are required"));
        }

        let channel = params
            .get("channel")
            .and_then(|v| v.as_str())
            .map(String::from);
        let metadata = params.get("metadata").cloned();

        let target = send::parse_recipient(recipient).map_err(|e| anyhow::anyhow!("{e}"))?;

        let opts = send::SendOptions {
            target,
            content: content.to_string(),
            channel,
            metadata,
        };

        let result = self.nomen.send(opts).await?;

        Ok(ContextVmResponse::ok(json!({
            "sent": true,
            "event_id": result.event_id,
            "accepted": result.accepted.len(),
            "rejected": result.rejected.len(),
        })))
    }

    async fn handle_delete(&self, params: &Value) -> Result<ContextVmResponse> {
        let topic = params.get("topic").and_then(|v| v.as_str());
        let id = params.get("id").and_then(|v| v.as_str());

        if topic.is_none() && id.is_none() {
            return Ok(ContextVmResponse::err("Provide either topic or id"));
        }

        self.nomen.delete(topic, id).await?;

        Ok(ContextVmResponse::ok(json!({
            "deleted": true,
            "topic": topic,
            "id": id,
        })))
    }

    async fn handle_list(&self, params: &Value) -> Result<ContextVmResponse> {
        let tier = params.get("tier").and_then(|v| v.as_str()).map(String::from);
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
        let include_stats = params.get("stats").and_then(|v| v.as_bool()).unwrap_or(false);

        let report = self.nomen.list(crate::ListOptions { tier, limit, include_stats }).await?;

        let items: Vec<Value> = report.memories.iter().map(|m| {
            json!({
                "tier": m.tier,
                "topic": m.topic,
                "summary": m.summary,
                "confidence": m.confidence,
                "version": m.version,
                "created_at": m.created_at,
            })
        }).collect();

        let mut result = json!({
            "count": items.len(),
            "memories": items,
        });

        if let Some(ref stats) = report.stats {
            result["stats"] = json!({
                "total": stats.total,
                "named": stats.named,
                "pending": stats.pending,
            });
        }

        Ok(ContextVmResponse::ok(result))
    }

    async fn handle_sync(&self, _params: &Value) -> Result<ContextVmResponse> {
        let report = self.nomen.sync().await?;

        Ok(ContextVmResponse::ok(json!({
            "stored": report.stored,
            "skipped": report.skipped,
            "errors": report.errors,
        })))
    }

    async fn handle_embed(&self, params: &Value) -> Result<ContextVmResponse> {
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
        let report = self.nomen.embed(limit).await?;

        Ok(ContextVmResponse::ok(json!({
            "embedded": report.embedded,
            "total": report.total,
        })))
    }

    async fn handle_prune(&self, params: &Value) -> Result<ContextVmResponse> {
        let days = params.get("days").and_then(|v| v.as_u64()).unwrap_or(90);
        let dry_run = params.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

        let report = self.nomen.prune(days, dry_run).await?;

        Ok(ContextVmResponse::ok(json!({
            "memories_pruned": report.memories_pruned,
            "raw_messages_pruned": report.raw_messages_pruned,
            "dry_run": report.dry_run,
        })))
    }
}
