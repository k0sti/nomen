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

pub fn parse_d_tag(d_tag: &str) -> String {
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
