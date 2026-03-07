use nostr_sdk::prelude::*;
use nostr_sdk::prelude::nip44;
use serde::Deserialize;

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

/// Parse a d-tag into a clean topic string.
///
/// Supports both v0.1 and v0.2 d-tag formats:
/// - v0.2: `{visibility}:{context}:{topic}` → returns topic as-is (full d-tag is the identifier)
/// - v0.1: `snow:memory:{topic}` → extracts topic
/// - v0.1: `snowclaw:memory:npub:{npub}` → `user:{npub_prefix}`
/// - v0.1: `snowclaw:memory:group:{id}` → `group:{id}`
/// - v0.1: `snowclaw:config:{key}` → `config:{key}`
pub fn parse_d_tag(d_tag: &str) -> String {
    // v0.2 format: starts with a known visibility prefix
    if is_v2_dtag(d_tag) {
        return d_tag.to_string();
    }

    // v0.1 formats
    if let Some(topic) = d_tag.strip_prefix("snow:memory:") {
        topic.to_string()
    } else if let Some(rest) = d_tag.strip_prefix("snowclaw:memory:npub:") {
        format!("user:{}", &rest[..12.min(rest.len())])
    } else if let Some(group) = d_tag.strip_prefix("snowclaw:memory:group:") {
        format!("group:{group}")
    } else if d_tag.starts_with("snowclaw:config:") {
        format!("config:{}", d_tag.strip_prefix("snowclaw:config:").unwrap())
    } else {
        d_tag.to_string()
    }
}

/// Check if a d-tag uses the v0.2 format: `{visibility}:{context}:{topic}`.
pub fn is_v2_dtag(d_tag: &str) -> bool {
    let prefix = d_tag.split(':').next().unwrap_or("");
    matches!(prefix, "public" | "group" | "circle" | "personal" | "internal" | "private")
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

/// Parse tier from event tags. Checks both new ("tier") and legacy ("snow:tier") tag names.
/// Also extracts visibility from the v0.2 d-tag format if no tier tag is present.
/// Normalizes "private" → "personal" (canonical 4-tier model).
pub fn parse_tier(tags: &Tags) -> String {
    // Try tier tag first (legacy/compat)
    let tier_from_tag = get_tag_value(tags, "tier")
        .or_else(|| get_tag_value(tags, "snow:tier"));

    // Try extracting from v0.2 d-tag format: {visibility}:{context}:{topic}
    let tier_val = tier_from_tag.unwrap_or_else(|| {
        if let Some(d) = get_tag_value(tags, "d") {
            if let Some(vis) = d.split(':').next() {
                match vis {
                    "public" | "group" | "personal" | "internal" | "circle" => return vis.to_string(),
                    "private" => return "personal".to_string(),
                    _ => {}
                }
            }
        }
        "unknown".to_string()
    });

    // Normalize legacy "private" → "personal"
    let tier_val = if tier_val == "private" { "personal".to_string() } else { tier_val };

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

/// Get a tag value, checking new name first, then legacy "snow:" prefixed name.
fn get_tag_compat(tags: &Tags, name: &str) -> Option<String> {
    get_tag_value(tags, name)
        .or_else(|| get_tag_value(tags, &format!("snow:{name}")))
}

/// Try to decrypt NIP-44 encrypted content using the provided keys.
pub fn try_decrypt_content(event: &Event, keys: &Keys) -> Option<String> {
    let content = event.content.as_str();

    if content.starts_with('{') || content.starts_with('[') || content.starts_with('"') {
        return None;
    }

    if let Ok(decrypted) = nip44::decrypt(keys.secret_key(), &keys.public_key(), content) {
        return Some(decrypted);
    }

    for tag in event.tags.iter() {
        let vec = tag.as_slice();
        if vec.len() >= 2 && vec[0] == "p" {
            if let Ok(recipient_pk) = PublicKey::from_hex(&vec[1]) {
                if let Ok(decrypted) = nip44::decrypt(keys.secret_key(), &recipient_pk, content) {
                    return Some(decrypted);
                }
            }
        }
    }

    None
}

pub fn parse_event(event: &Event, keys: &Keys) -> ParsedMemory {
    let tags = &event.tags;
    let d_tag_raw = get_tag_value(tags, "d").unwrap_or_default();
    let topic = parse_d_tag(&d_tag_raw);
    let tier = parse_tier(tags);
    // Check new tag names first, fall back to legacy "snow:" prefixed names
    let version = get_tag_compat(tags, "version").unwrap_or_else(|| "?".to_string());
    let confidence = get_tag_compat(tags, "confidence").unwrap_or_else(|| "?".to_string());
    let model = get_tag_compat(tags, "model").unwrap_or_else(|| "unknown".to_string());
    let source = get_tag_compat(tags, "source").unwrap_or_default();

    let content_str = if tier == "personal" || tier == "internal" || tier == "private" {
        match try_decrypt_content(event, keys) {
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
        source,
        content_raw: content_str,
        detail,
    }
}
