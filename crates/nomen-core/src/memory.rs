//! Pure memory parsing functions — no nostr_sdk Event/Tags dependencies.

use serde::Deserialize;

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
    pub created_at: nostr_sdk::Timestamp,
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
/// Accepts both v0.2 (`:` separated) and v0.3 (`/` separated) formats.
/// Returns the d-tag as-is.
pub fn parse_d_tag(d_tag: &str) -> String {
    d_tag.to_string()
}

/// Known tier prefixes for d-tag parsing.
const TIER_PREFIXES: &[&str] = &["public", "group", "circle", "personal", "private", "internal"];

/// Check if a d-tag uses the v0.2 format: `{visibility}:{scope}:{topic}`.
pub fn is_v2_dtag(d_tag: &str) -> bool {
    let prefix = d_tag.split(':').next().unwrap_or("");
    TIER_PREFIXES.contains(&prefix) && d_tag.contains(':')
}

/// Check if a d-tag uses the v0.3 format: `{tier}/{topic}` or `{tier}/{scope}/{topic}`.
pub fn is_v3_dtag(d_tag: &str) -> bool {
    let prefix = d_tag.split('/').next().unwrap_or("");
    TIER_PREFIXES.contains(&prefix) && d_tag.contains('/')
}

/// Check if a d-tag uses any known format (v0.2 or v0.3).
pub fn is_known_dtag(d_tag: &str) -> bool {
    is_v3_dtag(d_tag) || is_v2_dtag(d_tag)
}

/// Extract the topic component from a d-tag (supports both v0.2 and v0.3 formats).
///
/// v0.3 `public/rust-error-handling` → `rust-error-handling`
/// v0.3 `personal/{pubkey}/ssh-config` → `ssh-config`
/// v0.3 `public/rust/error-handling` → `rust/error-handling`
/// v0.2 `public::rust-error-handling` → `rust-error-handling`
/// v0.2 `group:techteam:deployment` → `deployment`
pub fn dtag_topic(d_tag: &str) -> Option<&str> {
    if is_v3_dtag(d_tag) {
        v3_dtag_topic(d_tag)
    } else if is_v2_dtag(d_tag) {
        v2_dtag_topic(d_tag)
    } else {
        None
    }
}

/// Extract the topic from a v0.3 d-tag.
///
/// For scoped tiers (personal, group, circle), the topic is everything after the second `/`.
/// For unscoped tiers (public, private), the topic is everything after the first `/`.
fn v3_dtag_topic(d_tag: &str) -> Option<&str> {
    let first_slash = d_tag.find('/')?;
    let tier = &d_tag[..first_slash];
    let after_tier = &d_tag[first_slash + 1..];

    match tier {
        // Scoped tiers: skip scope segment
        "personal" | "group" | "circle" => {
            let second_slash = after_tier.find('/')?;
            let topic = &after_tier[second_slash + 1..];
            if topic.is_empty() { None } else { Some(topic) }
        }
        // Unscoped tiers: topic is everything after tier/
        "public" | "private" | "internal" => {
            if after_tier.is_empty() { None } else { Some(after_tier) }
        }
        _ => None,
    }
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

/// Extract visibility and scope from a d-tag (supports both v0.2 and v0.3 formats).
///
/// Returns `(visibility, scope)`. Normalizes legacy "internal" → "private".
///
/// v0.3 `public/rust-error-handling` → `("public", "")`
/// v0.3 `personal/{pubkey}/ssh-config` → `("personal", "{pubkey}")`
/// v0.3 `private/agent-reasoning` → `("private", "")`
/// v0.2 `internal:{pubkey}:topic` → `("private", "")`
/// v0.2 `personal:{pubkey}:topic` → `("personal", "{pubkey}")`
pub fn extract_visibility_scope(d_tag: &str) -> (String, String) {
    if is_v3_dtag(d_tag) {
        return extract_visibility_scope_v3(d_tag);
    }
    if is_v2_dtag(d_tag) {
        return extract_visibility_scope_v2(d_tag);
    }
    ("public".to_string(), String::new())
}

/// Extract visibility and scope from a v0.3 d-tag.
fn extract_visibility_scope_v3(d_tag: &str) -> (String, String) {
    let mut parts = d_tag.splitn(3, '/');
    let tier = parts.next().unwrap_or("public");
    // Normalize legacy "internal" → "private"
    let visibility = normalize_tier_name(tier);

    let scope = match visibility.as_str() {
        "personal" | "group" | "circle" => {
            parts.next().unwrap_or("").to_string()
        }
        _ => String::new(),
    };
    (visibility, scope)
}

/// Extract visibility and scope from a v0.2 d-tag.
fn extract_visibility_scope_v2(d_tag: &str) -> (String, String) {
    let mut parts = d_tag.splitn(3, ':');
    let tier = parts.next().unwrap_or("public");
    let visibility = normalize_tier_name(tier);
    let scope = match visibility.as_str() {
        // In v0.2, "internal" had a pubkey scope but in v0.3 "private" has no scope
        "private" => String::new(),
        _ => parts.next().unwrap_or("").to_string(),
    };
    (visibility, scope)
}

/// Normalize legacy tier names to v0.3 canonical names.
pub fn normalize_tier_name(tier: &str) -> String {
    match tier {
        "internal" => "private".to_string(),
        other => other.to_string(),
    }
}

/// Build a v0.3 d-tag: `{tier}/{topic}` or `{tier}/{scope}/{topic}`.
///
/// v0.3 format uses `/` separators and no pubkey in private tier.
pub fn build_dtag(visibility: &str, scope: &str, topic: &str) -> String {
    let vis = normalize_tier_name(visibility);
    match vis.as_str() {
        "public" | "private" => format!("{vis}/{topic}"),
        "personal" | "group" | "circle" => {
            if scope.is_empty() {
                format!("{vis}/{topic}")
            } else {
                format!("{vis}/{scope}/{topic}")
            }
        }
        _ => format!("public/{topic}"),
    }
}

/// Build a v0.3 d-tag from tier string and author pubkey hex.
///
/// Derives visibility and scope from the tier:
/// - `"public"` → `public/{topic}`
/// - `"group:techteam"` → `group/techteam/{topic}`
/// - `"group"` → `group/{topic}`
/// - `"personal"` → `personal/{pubkey_hex}/{topic}`
/// - `"private"` / `"internal"` → `private/{topic}` (no pubkey)
pub fn build_dtag_from_tier(tier: &str, author_pubkey_hex: &str, topic: &str) -> String {
    if tier == "public" {
        build_dtag("public", "", topic)
    } else if let Some(group_id) = tier.strip_prefix("group:") {
        build_dtag("group", group_id, topic)
    } else if tier == "group" {
        build_dtag("group", "", topic)
    } else if tier == "personal" {
        build_dtag("personal", author_pubkey_hex, topic)
    } else if tier == "private" || tier == "internal" {
        build_dtag("private", "", topic)
    } else if let Some(circle_id) = tier.strip_prefix("circle:") {
        build_dtag("circle", circle_id, topic)
    } else {
        build_dtag("public", "", topic)
    }
}

/// Extract the base tier (public/group/personal/private) without scope suffix.
/// Normalizes legacy "internal" → "private".
pub fn base_tier(tier: &str) -> &str {
    if tier.starts_with("group") {
        "group"
    } else if tier == "internal" {
        "private"
    } else {
        tier
    }
}

// -- Legacy v0.2 builder (kept for backward compat during migration) --

/// Build a v0.2 d-tag from visibility, context, and topic.
#[deprecated(note = "Use build_dtag() for v0.3 format")]
pub fn build_v2_dtag(visibility: &str, context: &str, topic: &str) -> String {
    format!("{visibility}:{context}:{topic}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_v3_dtag() {
        assert!(is_v3_dtag("public/rust-error-handling"));
        assert!(is_v3_dtag("private/agent-reasoning"));
        assert!(is_v3_dtag("personal/abc123/ssh-config"));
        assert!(is_v3_dtag("group/techteam/deployment"));
        assert!(is_v3_dtag("circle/abc123/notes"));
        assert!(!is_v3_dtag("public::rust-error-handling"));
        assert!(!is_v3_dtag("unknown/topic"));
    }

    #[test]
    fn test_is_v2_dtag() {
        assert!(is_v2_dtag("public::rust-error-handling"));
        assert!(is_v2_dtag("internal:abc123:topic"));
        assert!(is_v2_dtag("personal:abc123:ssh-config"));
        assert!(!is_v2_dtag("public/rust-error-handling"));
        assert!(!is_v2_dtag("unknown:foo:bar"));
    }

    #[test]
    fn test_dtag_topic_v3() {
        assert_eq!(dtag_topic("public/rust-error-handling"), Some("rust-error-handling"));
        assert_eq!(dtag_topic("public/rust/error-handling"), Some("rust/error-handling"));
        assert_eq!(dtag_topic("private/agent-reasoning"), Some("agent-reasoning"));
        assert_eq!(dtag_topic("personal/abc123/ssh-config"), Some("ssh-config"));
        assert_eq!(dtag_topic("group/techteam/deployment"), Some("deployment"));
        assert_eq!(dtag_topic("circle/abc123/notes"), Some("notes"));
    }

    #[test]
    fn test_dtag_topic_v2() {
        assert_eq!(dtag_topic("public::rust-error-handling"), Some("rust-error-handling"));
        assert_eq!(dtag_topic("group:techteam:deployment"), Some("deployment"));
        assert_eq!(dtag_topic("internal:abc123:reasoning"), Some("reasoning"));
    }

    #[test]
    fn test_extract_visibility_scope_v3() {
        assert_eq!(
            extract_visibility_scope("public/rust-error-handling"),
            ("public".to_string(), "".to_string())
        );
        assert_eq!(
            extract_visibility_scope("private/agent-reasoning"),
            ("private".to_string(), "".to_string())
        );
        assert_eq!(
            extract_visibility_scope("personal/abc123/ssh-config"),
            ("personal".to_string(), "abc123".to_string())
        );
        assert_eq!(
            extract_visibility_scope("group/techteam/deployment"),
            ("group".to_string(), "techteam".to_string())
        );
    }

    #[test]
    fn test_extract_visibility_scope_v2_compat() {
        // v0.2 "internal" normalizes to "private" with no scope
        assert_eq!(
            extract_visibility_scope("internal:abc123:reasoning"),
            ("private".to_string(), "".to_string())
        );
        assert_eq!(
            extract_visibility_scope("personal:abc123:ssh-config"),
            ("personal".to_string(), "abc123".to_string())
        );
        assert_eq!(
            extract_visibility_scope("public::rust-error-handling"),
            ("public".to_string(), "".to_string())
        );
    }

    #[test]
    fn test_build_dtag() {
        assert_eq!(build_dtag("public", "", "rust-error-handling"), "public/rust-error-handling");
        assert_eq!(build_dtag("private", "", "agent-reasoning"), "private/agent-reasoning");
        assert_eq!(build_dtag("personal", "abc123", "ssh-config"), "personal/abc123/ssh-config");
        assert_eq!(build_dtag("group", "techteam", "deployment"), "group/techteam/deployment");
        // "internal" normalizes to "private"
        assert_eq!(build_dtag("internal", "abc123", "reasoning"), "private/reasoning");
    }

    #[test]
    fn test_build_dtag_from_tier() {
        assert_eq!(build_dtag_from_tier("public", "", "topic"), "public/topic");
        assert_eq!(build_dtag_from_tier("private", "", "topic"), "private/topic");
        assert_eq!(build_dtag_from_tier("internal", "abc123", "topic"), "private/topic");
        assert_eq!(build_dtag_from_tier("personal", "abc123", "topic"), "personal/abc123/topic");
        assert_eq!(build_dtag_from_tier("group:techteam", "", "topic"), "group/techteam/topic");
    }

    #[test]
    fn test_base_tier() {
        assert_eq!(base_tier("public"), "public");
        assert_eq!(base_tier("personal"), "personal");
        assert_eq!(base_tier("private"), "private");
        assert_eq!(base_tier("internal"), "private");
        assert_eq!(base_tier("group"), "group");
        assert_eq!(base_tier("group:techteam"), "group");
    }
}
