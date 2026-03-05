//! Context-VM: Nostr-native request/response interface for agents.
//!
//! Protocol:
//!   Request:  kind 21900 (ephemeral) — NIP-44 encrypted JSON
//!   Response: kind 21901 (ephemeral) — NIP-44 encrypted JSON
//!
//! Request tags:  ["p", nomen_npub], ["t", "nomen-request"], ["expiration", unix+60]
//! Response tags: ["p", requester_npub], ["e", request_event_id], ["t", "nomen-response"]

use std::collections::HashSet;

use anyhow::Result;
use nostr_sdk::prelude::*;
use nostr_sdk::prelude::nip44;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tracing::{debug, error, info, warn};

use crate::consolidate;
use crate::db;
use crate::embed::Embedder;
use crate::entities;
use crate::ingest;
use crate::relay::RelayManager;
use crate::search;

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

// ── Server ──────────────────────────────────────────────────────────

pub struct ContextVmServer {
    db: Surreal<Db>,
    embedder: Box<dyn Embedder>,
    relay: RelayManager,
    /// Allowed requester npubs (hex pubkeys). Empty = deny all.
    allowed_npubs: HashSet<String>,
}

impl ContextVmServer {
    pub fn new(
        db: Surreal<Db>,
        embedder: Box<dyn Embedder>,
        relay: RelayManager,
        allowed_npubs: Vec<String>,
    ) -> Self {
        Self {
            db,
            embedder,
            relay,
            allowed_npubs: allowed_npubs.into_iter().collect(),
        }
    }

    /// Subscribe to kind 21900 requests tagged with our npub and process them.
    pub async fn run(&self) -> Result<()> {
        let our_pubkey = self.relay.keys().public_key();
        info!(
            pubkey = %our_pubkey.to_bech32().unwrap_or_default(),
            "Context-VM listening for requests"
        );

        let filter = Filter::new()
            .kind(Kind::Custom(REQUEST_KIND))
            .pubkey(our_pubkey)
            .custom_tag(SingleLetterTag::lowercase(Alphabet::T), "nomen-request");

        self.relay.client().subscribe(filter, None).await?;

        // Event loop
        self.relay
            .client()
            .handle_notifications(|notification| async {
                if let RelayPoolNotification::Event { event, .. } = notification {
                    if event.kind == Kind::Custom(REQUEST_KIND) {
                        if let Err(e) = self.handle_event(&event).await {
                            error!(event_id = %event.id, "Failed to handle request: {e}");
                        }
                    }
                }
                Ok(false) // don't stop
            })
            .await?;

        Ok(())
    }

    async fn handle_event(&self, event: &Event) -> Result<()> {
        let requester = event.pubkey.to_hex();

        // Authorization check
        if !self.allowed_npubs.contains(&requester) {
            // Also check bech32 format
            let requester_bech32 = event.pubkey.to_bech32().unwrap_or_default();
            if !self.allowed_npubs.contains(&requester_bech32) {
                warn!(
                    requester = %requester,
                    "Rejecting request from unauthorized npub"
                );
                return Ok(());
            }
        }

        debug!(
            requester = %requester,
            event_id = %event.id,
            "Processing Context-VM request"
        );

        // Decrypt content (NIP-44, encrypted to our pubkey)
        let plaintext = nip44::decrypt(
            self.relay.keys().secret_key(),
            &event.pubkey,
            &event.content,
        )
        .map_err(|e| anyhow::anyhow!("NIP-44 decryption failed: {e}"))?;

        // Parse request
        let request: ContextVmRequest = serde_json::from_str(&plaintext)
            .map_err(|e| anyhow::anyhow!("Invalid request JSON: {e}"))?;

        info!(
            action = %request.action,
            requester = %requester,
            "Dispatching action"
        );

        // Dispatch to handler
        let response = match request.action.as_str() {
            "search" => self.handle_search(&request.params).await,
            "store" => self.handle_store(&request.params).await,
            "ingest" => self.handle_ingest(&request.params).await,
            "entities" => self.handle_entities(&request.params).await,
            "consolidate" => self.handle_consolidate(&request.params).await,
            "messages" => self.handle_messages(&request.params).await,
            "groups" => self.handle_groups(&request.params).await,
            _ => Ok(ContextVmResponse::err(format!(
                "Unknown action: {}",
                request.action
            ))),
        }
        .unwrap_or_else(|e| ContextVmResponse::err(format!("Internal error: {e}")));

        // Encrypt response and publish
        self.send_response(event, &response).await?;

        Ok(())
    }

    async fn send_response(&self, request_event: &Event, response: &ContextVmResponse) -> Result<()> {
        let response_json = serde_json::to_string(response)?;

        // Encrypt to requester's pubkey
        let encrypted = nip44::encrypt(
            self.relay.keys().secret_key(),
            &request_event.pubkey,
            &response_json,
            nip44::Version::default(),
        )
        .map_err(|e| anyhow::anyhow!("NIP-44 encryption failed: {e}"))?;

        let tags = vec![
            Tag::public_key(request_event.pubkey),
            Tag::event(request_event.id),
            Tag::custom(
                TagKind::Custom("t".into()),
                vec!["nomen-response".to_string()],
            ),
        ];

        let builder = EventBuilder::new(Kind::Custom(RESPONSE_KIND), encrypted).tags(tags);

        let output = self.relay.publish(builder).await?;
        debug!(
            event_id = %output.event_id,
            "Published Context-VM response"
        );

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

        let tier = params.get("tier").and_then(|v| v.as_str()).map(String::from);
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;
        let scope = params.get("scope").and_then(|v| v.as_str()).map(String::from);

        let opts = search::SearchOptions {
            query,
            tier,
            allowed_scopes: scope.map(|s| vec![s]),
            limit,
            vector_weight: 0.7,
            text_weight: 0.3,
            min_confidence: None,
        };

        let results = search::search(&self.db, self.embedder.as_ref(), &opts).await?;

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
        let tier = params
            .get("tier")
            .and_then(|v| v.as_str())
            .unwrap_or("public")
            .to_string();
        let confidence = params
            .get("confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.8);

        let content = json!({
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
            model: "contextvm/agent".to_string(),
            summary: summary.clone(),
            created_at: Timestamp::now(),
            d_tag: d_tag.clone(),
            source: "contextvm".to_string(),
            content_raw: content.to_string(),
            detail: if detail.is_empty() { summary.clone() } else { detail },
        };

        db::store_memory_direct(&self.db, &parsed, "contextvm").await?;

        // Generate embedding if available
        if self.embedder.dimensions() > 0 {
            let text = format!("{} {}", parsed.summary, parsed.detail);
            if let Ok(embeddings) = self.embedder.embed(&[text]).await {
                if let Some(embedding) = embeddings.into_iter().next() {
                    let _ = db::store_embedding(&self.db, &d_tag, embedding).await;
                }
            }
        }

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

        let id = ingest::ingest_message(&self.db, &msg).await?;

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

        let entity_list = db::list_entities(&self.db, kind.as_ref()).await?;

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
        let config = consolidate::ConsolidationConfig::default();
        let report =
            consolidate::consolidate(&self.db, self.embedder.as_ref(), &config).await?;

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
            limit: Some(
                params
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50) as usize,
            ),
            consolidated_only: false,
        };

        let messages = ingest::get_messages(&self.db, &opts).await?;

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

    async fn handle_groups(&self, _params: &Value) -> Result<ContextVmResponse> {
        let groups = crate::groups::list_groups(&self.db).await?;

        let items: Vec<Value> = groups
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
}

