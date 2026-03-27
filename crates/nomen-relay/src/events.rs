//! Event parsing: extract Nomen memory data from Nostr events.

use nostr_sdk::prelude::*;

use nomen_core::memory::{base_tier, normalize_content, parse_d_tag, ParsedMemory};
use nomen_core::signer::NomenSigner;

/// Parse tier from event tags.
///
/// Reads `visibility` tag first, then falls back to d-tag prefix.
/// Normalizes legacy "internal" → "private".
/// Supports both v0.2 (`:` separator) and v0.3 (`/` separator) d-tag formats.
pub fn parse_tier(tags: &Tags) -> String {
    // Try visibility tag first (canonical)
    let tier_val = if let Some(vis) = get_tag_value(tags, "visibility") {
        nomen_core::memory::normalize_tier_name(&vis)
    } else {
        // Fall back to d-tag prefix (supports both : and / separators)
        if let Some(d) = get_tag_value(tags, "d") {
            let (vis, _scope) = nomen_core::memory::extract_visibility_scope(&d);
            vis
        } else {
            "unknown".to_string()
        }
    };

    if tier_val == "group" {
        if let Some(h) = get_tag_value(tags, "h") {
            return format!("group:{h}");
        }
    }
    tier_val
}

pub fn get_tag_value(tags: &Tags, name: &str) -> Option<String> {
    for tag in tags.iter() {
        let vec = tag.as_slice();
        if vec.len() >= 2 && vec[0] == name {
            return Some(vec[1].to_string());
        }
    }
    None
}

/// Try to decrypt NIP-44 encrypted content using the provided signer.
pub fn try_decrypt_content(event: &Event, signer: &dyn NomenSigner) -> Option<String> {
    let content = event.content.as_str();

    if content.starts_with('{') || content.starts_with('[') || content.starts_with('"') {
        return None;
    }

    // Try self-decrypt first
    if let Ok(decrypted) = signer.decrypt(content) {
        return Some(decrypted);
    }

    // Try decrypting with each p-tag recipient
    for tag in event.tags.iter() {
        let vec = tag.as_slice();
        if vec.len() >= 2 && vec[0] == "p" {
            if let Ok(sender_pk) = PublicKey::from_hex(&vec[1]) {
                if let Ok(decrypted) = signer.decrypt_from(content, &sender_pk) {
                    return Some(decrypted);
                }
            }
        }
    }

    None
}

pub fn parse_event(event: &Event, signer: &dyn NomenSigner) -> ParsedMemory {
    let tags = &event.tags;
    let d_tag_raw = get_tag_value(tags, "d").unwrap_or_default();
    let topic = parse_d_tag(&d_tag_raw);
    let tier = parse_tier(tags);
    let visibility =
        get_tag_value(tags, "visibility").unwrap_or_else(|| base_tier(&tier).to_string());
    let model = get_tag_value(tags, "model").unwrap_or_else(|| "unknown".to_string());
    let importance = get_tag_value(tags, "importance").and_then(|v| v.parse::<i32>().ok());

    let content_str = if tier == "personal" || tier == "private" {
        match try_decrypt_content(event, signer) {
            Some(decrypted) => decrypted,
            None => event.content.to_string(),
        }
    } else {
        event.content.to_string()
    };

    // Normalize: if legacy JSON content, merge summary+detail into plain text
    let content = normalize_content(&content_str);

    // Always normalize d_tag to clean topic for SurrealDB storage
    ParsedMemory {
        tier,
        visibility,
        topic: topic.clone(),
        model,
        content,
        created_at: event.created_at,
        d_tag: topic,
        source: event.pubkey.to_hex(),
        importance,
    }
}
