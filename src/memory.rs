use nostr_sdk::prelude::*;
use serde::Deserialize;

use crate::signer::NomenSigner;

#[derive(Deserialize)]
pub struct MemoryContent {
    pub summary: String,
    #[allow(dead_code)]
    pub detail: String,
    #[allow(dead_code)]
    pub context: Option<String>,
}

pub struct ParsedMemory {
    pub tier: String,
    pub topic: String,
    pub version: String,
    pub confidence: String,
    pub model: String,
    pub summary: String,
    pub created_at: Timestamp,
    pub d_tag: String,
    pub source: String,
    pub content_raw: String,
    pub detail: String,
}

/// Parse a d-tag into a normalized string.
///
/// v0.2 format only: `{visibility}:{scope}:{topic}` — returned as-is.
/// Unrecognized formats are returned verbatim.
pub fn parse_d_tag(d_tag: &str) -> String {
    d_tag.to_string()
}

/// Check if a d-tag uses the v0.2 format: `{visibility}:{scope}:{topic}`.
pub fn is_v2_dtag(d_tag: &str) -> bool {
    let prefix = d_tag.split(':').next().unwrap_or("");
    matches!(
        prefix,
        "public" | "group" | "circle" | "personal" | "internal"
    )
}

/// Extract the topic component from a v0.2 d-tag.
/// For `public::rust-error-handling` returns `rust-error-handling`.
/// For `group:techteam:deployment` returns `deployment`.
/// For `group:telegram:-1003821690204:room/8485` returns `room/8485`.
pub fn v2_dtag_topic(d_tag: &str) -> Option<&str> {
    if !is_v2_dtag(d_tag) {
        return None;
    }
    let first_colon = d_tag.find(':')?;
    let last_colon = d_tag.rfind(':')?;
    if last_colon <= first_colon {
        return None;
    }
    Some(&d_tag[last_colon + 1..])
}

/// Extract visibility and scope from a v0.2 d-tag.
/// Returns `(visibility, scope)` — e.g. `("group", "techteam")` from `"group:techteam:deploy"`.
/// Supports scopes containing colons, e.g. `("group", "telegram:-1003821690204")`
/// from `"group:telegram:-1003821690204:room/8485"`.
/// For non-v0.2 d-tags, returns `("public", "")`.
pub fn extract_visibility_scope(d_tag: &str) -> (String, String) {
    if !is_v2_dtag(d_tag) {
        return ("public".to_string(), String::new());
    }
    let first_colon = match d_tag.find(':') {
        Some(i) => i,
        None => return ("public".to_string(), String::new()),
    };
    let last_colon = match d_tag.rfind(':') {
        Some(i) if i > first_colon => i,
        _ => return ("public".to_string(), String::new()),
    };
    let visibility = d_tag[..first_colon].to_string();
    let scope = d_tag[first_colon + 1..last_colon].to_string();
    (visibility, scope)
}

/// Build a v0.2 d-tag from visibility, context, and topic.
pub fn build_v2_dtag(visibility: &str, context: &str, topic: &str) -> String {
    format!("{visibility}:{context}:{topic}")
}

/// Build a v0.2 d-tag from tier string and author pubkey hex.
///
/// Derives visibility and context from the tier:
/// - `"public"` → `public::topic`
/// - `"group:techteam"` → `group:techteam:topic`
/// - `"group"` → `group::topic`
/// - `"personal"` / `"private"` → `personal:{pubkey_hex}:topic`
/// - `"internal"` → `internal:{pubkey_hex}:topic`
pub fn build_dtag_from_tier(tier: &str, author_pubkey_hex: &str, topic: &str) -> String {
    if tier == "public" {
        build_v2_dtag("public", "", topic)
    } else if let Some(group_id) = tier.strip_prefix("group:") {
        build_v2_dtag("group", group_id, topic)
    } else if tier == "group" {
        build_v2_dtag("group", "", topic)
    } else if let Some(circle_id) = tier.strip_prefix("circle:") {
        build_v2_dtag("circle", circle_id, topic)
    } else if tier == "circle" {
        build_v2_dtag("circle", "", topic)
    } else if let Some(scope) = tier.strip_prefix("personal:") {
        build_v2_dtag("personal", scope, topic)
    } else if tier == "personal" || tier == "private" {
        build_v2_dtag("personal", author_pubkey_hex, topic)
    } else if let Some(scope) = tier.strip_prefix("internal:") {
        build_v2_dtag("internal", scope, topic)
    } else if tier == "internal" {
        build_v2_dtag("internal", author_pubkey_hex, topic)
    } else {
        build_v2_dtag("public", "", topic)
    }
}


/// Parse tier from event tags.
///
/// Reads `visibility` tag first, then falls back to d-tag prefix.
/// Normalizes "private" → "personal".
pub fn parse_tier(tags: &Tags) -> String {
    // Try visibility tag first (canonical v0.2)
    let tier_val = if let Some(vis) = get_tag_value(tags, "visibility") {
        vis
    } else {
        // Fall back to d-tag prefix
        if let Some(d) = get_tag_value(tags, "d") {
            if let Some(vis) = d.split(':').next() {
                match vis {
                    "public" | "group" | "personal" | "internal" | "circle" => {
                        vis.to_string()
                    }
                    "private" => "personal".to_string(),
                    _ => "unknown".to_string(),
                }
            } else {
                "unknown".to_string()
            }
        } else {
            "unknown".to_string()
        }
    };

    // Normalize legacy "private" → "personal"
    let tier_val = if tier_val == "private" {
        "personal".to_string()
    } else {
        tier_val
    };

    if tier_val == "group" {
        if let Some(h) = get_tag_value(tags, "h") {
            return format!("group:{h}");
        }
    }
    tier_val
}

/// Extract the base tier (public/group/personal/internal) without scope suffix.
/// Normalizes legacy "private" to "personal".
pub fn base_tier(tier: &str) -> &str {
    if tier.starts_with("group") {
        "group"
    } else if tier.starts_with("circle") {
        "circle"
    } else if tier == "private" || tier.starts_with("personal") {
        "personal"
    } else if tier.starts_with("internal") {
        "internal"
    } else {
        tier
    }
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
    let version = get_tag_value(tags, "version").unwrap_or_else(|| "?".to_string());
    let confidence = get_tag_value(tags, "confidence").unwrap_or_else(|| "?".to_string());
    let model = get_tag_value(tags, "model").unwrap_or_else(|| "unknown".to_string());

    let content_str = if tier == "personal" || tier == "internal" {
        match try_decrypt_content(event, signer) {
            Some(decrypted) => decrypted,
            None => event.content.to_string(),
        }
    } else {
        event.content.to_string()
    };

    let (summary, detail) = match serde_json::from_str::<MemoryContent>(&content_str) {
        Ok(content) => (content.summary.clone(), content.detail),
        Err(_) => {
            let s = if content_str.len() > 80 {
                format!("{}...", &content_str[..80])
            } else {
                content_str.clone()
            };
            (s.clone(), s)
        }
    };

    // Always normalize d_tag to clean topic for SurrealDB storage
    ParsedMemory {
        tier,
        topic: topic.clone(),
        version,
        confidence,
        model,
        summary,
        created_at: event.created_at,
        d_tag: topic,
        source: event.pubkey.to_hex(),
        content_raw: content_str,
        detail,
    }
}
