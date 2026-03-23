//! Agent messaging: send messages to npubs (DM), groups, or public.
//!
//! Supports multiple delivery channels (nostr, telegram, etc.).
//! For nostr: npub->NIP-17 gift-wrapped DM, group->kind 9, public->kind 1.

pub use nomen_core::send::*;

use anyhow::{bail, Result};
use nostr_sdk::prelude::*;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use tracing::{debug, info};

use crate::db;
use crate::groups::GroupStore;
use crate::ingest::RawMessage;
use crate::relay::RelayManager;

/// Send a message via the appropriate nostr event type.
pub async fn send_message(
    relay: &RelayManager,
    db: &Surreal<Db>,
    groups: &GroupStore,
    opts: SendOptions,
) -> Result<SendResult> {
    let channel = opts.channel.as_deref().unwrap_or("nostr");

    if channel != "nostr" {
        bail!("Only 'nostr' channel is currently supported (got: {channel})");
    }

    let result = match &opts.target {
        SendTarget::Npub(npub) => send_dm(relay, npub, &opts.content).await?,
        SendTarget::Group(group_id) => send_group(relay, groups, group_id, &opts.content).await?,
        SendTarget::Public => send_public(relay, &opts.content).await?,
    };

    // Store locally as raw_message
    let (tier, scope) = match &opts.target {
        SendTarget::Npub(npub) => {
            let pk = PublicKey::from_bech32(npub)
                .map(|pk| pk.to_hex())
                .unwrap_or_else(|_| npub.clone());
            ("personal".to_string(), pk)
        }
        SendTarget::Group(group_id) => ("group".to_string(), group_id.clone()),
        SendTarget::Public => ("public".to_string(), String::new()),
    };

    let metadata = serde_json::json!({
        "tier": tier,
        "scope": scope,
        "channel": channel,
        "event_id": result.event_id,
        "direction": "outbound",
    });

    let msg = RawMessage {
        source: "nomen".to_string(),
        source_id: Some(result.event_id.clone()),
        sender: relay.public_key().to_hex(),
        channel: Some(channel.to_string()),
        content: opts.content,
        metadata: Some(metadata.to_string()),
        created_at: None,
    };

    let _ = db::store_raw_message(db, &msg).await;

    info!(
        event_id = %result.event_id,
        target = ?opts.target,
        "Message sent"
    );

    Ok(result)
}

/// Send a NIP-17 gift-wrapped DM (kind 1059) with NIP-44 encryption.
async fn send_dm(relay: &RelayManager, npub: &str, content: &str) -> Result<SendResult> {
    let recipient_pk = PublicKey::from_bech32(npub)
        .or_else(|_| PublicKey::from_hex(npub))
        .map_err(|e| anyhow::anyhow!("Invalid recipient npub: {e}"))?;

    debug!(recipient = %npub, "Sending NIP-17 gift-wrapped DM");

    // Build NIP-17 rumor (kind 14 = private direct message)
    let rumor = EventBuilder::new(Kind::Custom(14), content)
        .tags(vec![Tag::public_key(recipient_pk)])
        .build(relay.public_key());

    // Gift wrap and send (NIP-59 + NIP-44 encryption handled by nostr-sdk)
    let output = relay
        .client()
        .gift_wrap(&recipient_pk, rumor, Vec::<Tag>::new())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send gift-wrapped DM: {e}"))?;

    let event_id = output.id().to_hex();
    let accepted: Vec<String> = output.success.iter().map(|u| u.to_string()).collect();
    let rejected: Vec<(String, String)> = output
        .failed
        .iter()
        .map(|(u, r)| (u.to_string(), r.clone()))
        .collect();

    Ok(SendResult {
        event_id,
        accepted,
        rejected,
    })
}

/// Send a NIP-29 group message (kind 9) with h-tag.
async fn send_group(
    relay: &RelayManager,
    groups: &GroupStore,
    group_id: &str,
    content: &str,
) -> Result<SendResult> {
    // Resolve group to get NIP-29 h-tag value
    let h_tag = groups
        .resolve_scope_to_nostr_group(group_id)
        .unwrap_or(group_id);

    debug!(group = %group_id, h_tag = %h_tag, "Sending NIP-29 group message");

    let tags = vec![Tag::custom(
        TagKind::Custom("h".into()),
        vec![h_tag.to_string()],
    )];

    let builder = EventBuilder::new(Kind::Custom(9), content).tags(tags);
    let publish_result = relay.publish(builder).await?;

    Ok(SendResult {
        event_id: publish_result.event_id.to_hex(),
        accepted: publish_result.accepted,
        rejected: publish_result.rejected,
    })
}

/// Send a public note (kind 1).
async fn send_public(relay: &RelayManager, content: &str) -> Result<SendResult> {
    debug!("Sending public note");

    let builder = EventBuilder::new(Kind::TextNote, content);
    let publish_result = relay.publish(builder).await?;

    Ok(SendResult {
        event_id: publish_result.event_id.to_hex(),
        accepted: publish_result.accepted,
        rejected: publish_result.rejected,
    })
}

// Tests for parse_recipient are in nomen-core::send.
