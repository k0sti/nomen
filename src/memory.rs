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
    pub visibility: String,
    pub topic: String,
    pub version: String,
    pub model: String,
    pub created_at: Timestamp,
    pub d_tag: String,
    pub source: String,
    pub content_raw: String,
    pub detail: String,
}

/// Return the first non-empty line of `detail` for display purposes.
pub fn first_line(detail: &str) -> &str {
    detail
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(detail)
}

/// Parse a d-tag into a normalized string.
///
/// v0.2 format only: `{visibility}:{scope}:{topic}` — returned as-is.
/// Unrecognized formats are returned verbatim.
pub fn parse_d_tag(d_tag: &str) -> String {
    d_tag.to_string()
}

/// Check if a d-tag uses the v0.2+ format.
///
/// Accepts both colon (`{vis}:{scope}:{topic}`) and slash (`{vis}/{scope}/{topic}`) separators.
pub fn is_v2_dtag(d_tag: &str) -> bool {
    let prefix = d_tag
        .split(|c: char| c == ':' || c == '/')
        .next()
        .unwrap_or("");
    matches!(
        prefix,
        "public" | "group" | "circle" | "personal" | "internal"
    )
}

/// Extract the topic component from a d-tag.
///
/// Supports both colon and slash formats:
/// - `public::rust-error-handling` → `rust-error-handling`
/// - `group:telegram:-1003821690204:room/8485` → `room/8485`
/// - `public/rust-error-handling` → `rust-error-handling`
/// - `group/telegram:-1003821690204/room/8485` → `room/8485`
pub fn v2_dtag_topic(d_tag: &str) -> Option<&str> {
    if !is_v2_dtag(d_tag) {
        return None;
    }

    let vis_end = d_tag.find(|c: char| c == ':' || c == '/')?;
    let sep = d_tag.as_bytes()[vis_end];
    let visibility = &d_tag[..vis_end];

    if sep == b'/' {
        // Slash format
        let rest = &d_tag[vis_end + 1..];
        if visibility == "public" {
            // public/{topic}
            if rest.is_empty() { return None; }
            return Some(rest);
        }
        // {vis}/{scope}/{topic} — scope is up to the next '/'
        match rest.find('/') {
            Some(i) if i + 1 < rest.len() => Some(&rest[i + 1..]),
            _ => None,
        }
    } else {
        // Colon format: topic is after the last ':'
        let last_colon = d_tag.rfind(':')?;
        if last_colon <= vis_end {
            return None;
        }
        Some(&d_tag[last_colon + 1..])
    }
}

/// Extract visibility and scope from a d-tag.
///
/// Returns `(visibility, scope)`. Supports both colon and slash formats:
/// - `"group:techteam:deploy"` → `("group", "techteam")`
/// - `"group/techteam/deploy"` → `("group", "techteam")`
/// - `"public/my-topic"` → `("public", "")`
/// - `"personal/d29fe7c1/projects/nomen"` → `("personal", "d29fe7c1")`
///
/// For non-v0.2 d-tags, returns `("public", "")`.
pub fn extract_visibility_scope(d_tag: &str) -> (String, String) {
    if !is_v2_dtag(d_tag) {
        return ("public".to_string(), String::new());
    }

    let vis_end = match d_tag.find(|c: char| c == ':' || c == '/') {
        Some(i) => i,
        None => return ("public".to_string(), String::new()),
    };
    let visibility = &d_tag[..vis_end];
    let sep = d_tag.as_bytes()[vis_end];

    if sep == b'/' {
        // Slash format
        let rest = &d_tag[vis_end + 1..];
        if visibility == "public" {
            return (visibility.to_string(), String::new());
        }
        // Scope is up to the next '/' (scopes never contain slashes)
        match rest.find('/') {
            Some(i) => (visibility.to_string(), rest[..i].to_string()),
            None => (visibility.to_string(), rest.to_string()),
        }
    } else {
        // Colon format: scope is between first and last ':'
        let last_colon = match d_tag.rfind(':') {
            Some(i) if i > vis_end => i,
            _ => return (visibility.to_string(), String::new()),
        };
        let scope = d_tag[vis_end + 1..last_colon].to_string();
        (visibility.to_string(), scope)
    }
}

/// Build a v0.3 d-tag from visibility, context, and topic.
///
/// Uses slash separators: `{visibility}/{context}/{topic}`.
/// Empty context collapses: `{visibility}/{topic}`.
pub fn build_v2_dtag(visibility: &str, context: &str, topic: &str) -> String {
    if context.is_empty() {
        format!("{visibility}/{topic}")
    } else {
        format!("{visibility}/{context}/{topic}")
    }
}

/// Build a d-tag from visibility, scope, and topic.
///
/// - `("public", "", "topic")` → `public/topic`
/// - `("group", "techteam", "topic")` → `group/techteam/topic`
/// - `("personal", "{pubkey}", "topic")` → `personal/{pubkey}/topic`
pub fn build_dtag(visibility: &str, scope: &str, topic: &str) -> String {
    build_v2_dtag(visibility, scope, topic)
}

/// Build a d-tag from tier string and author pubkey hex (legacy helper).
///
/// Derives visibility and context from the tier:
/// - `"public"` → `public/topic`
/// - `"group:techteam"` → `group/techteam/topic`
/// - `"group"` → `group/topic`
/// - `"personal"` / `"private"` → `personal/{pubkey_hex}/topic`
/// - `"internal"` → `internal/{pubkey_hex}/topic`
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


/// Convert a colon-format d-tag to slash format.
///
/// Returns the d-tag unchanged if it's already in slash format or unrecognized.
pub fn migrate_dtag_to_slash(d_tag: &str) -> String {
    if !is_v2_dtag(d_tag) {
        return d_tag.to_string();
    }

    // Find the separator after visibility
    let vis_end = match d_tag.find(|c: char| c == ':' || c == '/') {
        Some(i) => i,
        None => return d_tag.to_string(),
    };

    // Already slash format
    if d_tag.as_bytes()[vis_end] == b'/' {
        return d_tag.to_string();
    }

    // Parse colon format and rebuild as slash
    let (visibility, scope) = extract_visibility_scope(d_tag);
    let topic = match v2_dtag_topic(d_tag) {
        Some(t) => t.to_string(),
        None => return d_tag.to_string(),
    };

    build_v2_dtag(&visibility, &scope, &topic)
}

/// Check if a d-tag uses colon separators (legacy format needing migration).
pub fn is_colon_format(d_tag: &str) -> bool {
    if !is_v2_dtag(d_tag) {
        return false;
    }
    let vis_end = match d_tag.find(|c: char| c == ':' || c == '/') {
        Some(i) => i,
        None => return false,
    };
    d_tag.as_bytes()[vis_end] == b':'
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
        // Fall back to d-tag prefix (supports both ':' and '/' separators)
        if let Some(d) = get_tag_value(tags, "d") {
            if let Some(vis) = d.split(|c: char| c == ':' || c == '/').next() {
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
    let visibility = parse_tier(tags);
    let version = get_tag_value(tags, "version").unwrap_or_else(|| "?".to_string());
    let model = get_tag_value(tags, "model").unwrap_or_else(|| "unknown".to_string());

    let content_str = if visibility == "personal" || visibility == "internal" {
        match try_decrypt_content(event, signer) {
            Some(decrypted) => decrypted,
            None => event.content.to_string(),
        }
    } else {
        event.content.to_string()
    };

    let detail = match serde_json::from_str::<MemoryContent>(&content_str) {
        Ok(content) => {
            if content.detail.is_empty() {
                content.summary
            } else {
                content.detail
            }
        }
        Err(_) => content_str.clone(),
    };

    // Always normalize d_tag to clean topic for SurrealDB storage
    ParsedMemory {
        visibility,
        topic: topic.clone(),
        version,
        model,
        created_at: event.created_at,
        d_tag: topic,
        source: event.pubkey.to_hex(),
        content_raw: content_str,
        detail,
    }
}
