use nostr_sdk::prelude::*;
use serde::Deserialize;

use crate::signer::NomenSigner;

/// Legacy JSON content format (for backward-compat reads).
#[derive(Deserialize)]
struct LegacyContent {
    summary: Option<String>,
    detail: Option<String>,
}

pub struct ParsedMemory {
    pub tier: String,
    pub topic: String,
    pub visibility: String,
    pub model: String,
    /// Plain-text content (the full memory body).
    pub content: String,
    pub created_at: Timestamp,
    pub d_tag: String,
    pub source: String,
    /// Importance score (1-10), if present in tags.
    pub importance: Option<i32>,
}

/// Normalize content from a Nostr event: if it's legacy JSON with summary/detail,
/// merge them into plain text. Otherwise return as-is.
pub fn normalize_content(raw: &str) -> String {
    if let Ok(legacy) = serde_json::from_str::<LegacyContent>(raw) {
        let summary = legacy.summary.unwrap_or_default();
        let detail = legacy.detail.unwrap_or_default();
        if !summary.is_empty() && !detail.is_empty() && summary != detail {
            format!("{summary}\n\n{detail}")
        } else if !detail.is_empty() {
            detail
        } else {
            summary
        }
    } else {
        raw.to_string()
    }
}

/// Extract the first line from content (for display as title).
pub fn first_line(content: &str) -> &str {
    content.lines().next().unwrap_or(content)
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
pub fn v2_dtag_topic(d_tag: &str) -> Option<&str> {
    if !is_v2_dtag(d_tag) {
        return None;
    }
    // Find the second colon
    let first_colon = d_tag.find(':')?;
    let rest = &d_tag[first_colon + 1..];
    let second_colon = rest.find(':')?;
    Some(&rest[second_colon + 1..])
}

/// Extract visibility and scope from a v0.2 d-tag.
/// Returns `(visibility, scope)` — e.g. `("group", "techteam")` from `"group:techteam:deploy"`.
/// For non-v0.2 d-tags, returns `("public", "")`.
pub fn extract_visibility_scope(d_tag: &str) -> (String, String) {
    if !is_v2_dtag(d_tag) {
        return ("public".to_string(), String::new());
    }
    let mut parts = d_tag.splitn(3, ':');
    let visibility = parts.next().unwrap_or("public").to_string();
    let scope = parts.next().unwrap_or("").to_string();
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
    } else if tier == "personal" || tier == "private" {
        build_v2_dtag("personal", author_pubkey_hex, topic)
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
    } else if tier == "private" {
        "personal"
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
    let visibility = get_tag_value(tags, "visibility").unwrap_or_else(|| base_tier(&tier).to_string());
    let model = get_tag_value(tags, "model").unwrap_or_else(|| "unknown".to_string());
    let importance = get_tag_value(tags, "importance").and_then(|v| v.parse::<i32>().ok());

    let content_str = if tier == "personal" || tier == "internal" {
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
