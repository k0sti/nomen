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

pub fn parse_tier(tags: &Tags) -> String {
    let tier_val = get_tag_value(tags, "snow:tier").unwrap_or("unknown".to_string());
    if tier_val == "group" {
        if let Some(h) = get_tag_value(tags, "h") {
            return format!("group:{h}");
        }
    }
    tier_val
}

/// Extract the base tier (public/group/private) without scope suffix
pub fn base_tier(tier: &str) -> &str {
    if tier.starts_with("group") {
        "group"
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
    let d_tag = get_tag_value(tags, "d").unwrap_or_default();
    let topic = parse_d_tag(&d_tag);
    let tier = parse_tier(tags);
    let version = get_tag_value(tags, "snow:version").unwrap_or("?".to_string());
    let confidence = get_tag_value(tags, "snow:confidence").unwrap_or("?".to_string());
    let model = get_tag_value(tags, "snow:model").unwrap_or("unknown".to_string());
    let source = get_tag_value(tags, "snow:source").unwrap_or_default();

    let content_str = if tier == "private" {
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

    ParsedMemory {
        tier,
        topic,
        version,
        confidence,
        model,
        summary,
        created_at: event.created_at,
        d_tag,
        source,
        content_raw: content_str,
        detail,
    }
}
