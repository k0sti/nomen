//! Pure memory parsing functions — no nostr_sdk Event/Tags dependencies.

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

/// Extract the first line from content (for display as title).
pub fn first_line(content: &str) -> &str {
    content.lines().next().unwrap_or(content)
}

/// Parse a d-tag into a normalized string.
pub fn parse_d_tag(d_tag: &str) -> String {
    d_tag.to_string()
}

/// Known tier prefixes for d-tag parsing.
const TIER_PREFIXES: &[&str] = &[
    "public", "group", "circle", "personal", "private",
];

/// Check if a d-tag uses the v0.3 format: `{tier}/{topic}` or `{tier}/{scope}/{topic}`.
pub fn is_v3_dtag(d_tag: &str) -> bool {
    let prefix = d_tag.split('/').next().unwrap_or("");
    TIER_PREFIXES.contains(&prefix) && d_tag.contains('/')
}

/// Extract the topic component from a d-tag (v0.3 format only).
///
/// v0.3 `public/rust-error-handling` → `rust-error-handling`
/// v0.3 `personal/{pubkey}/ssh-config` → `ssh-config`
/// v0.3 `public/rust/error-handling` → `rust/error-handling`
pub fn dtag_topic(d_tag: &str) -> Option<&str> {
    if is_v3_dtag(d_tag) {
        v3_dtag_topic(d_tag)
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
            if topic.is_empty() {
                None
            } else {
                Some(topic)
            }
        }
        // Unscoped tiers: topic is everything after tier/
        "public" | "private" => {
            if after_tier.is_empty() {
                None
            } else {
                Some(after_tier)
            }
        }
        _ => None,
    }
}

/// Extract visibility and scope from a v0.3 d-tag.
///
/// Returns `(visibility, scope)`.
///
/// v0.3 `public/rust-error-handling` → `("public", "")`
/// v0.3 `personal/{pubkey}/ssh-config` → `("personal", "{pubkey}")`
/// v0.3 `private/agent-reasoning` → `("private", "")`
pub fn extract_visibility_scope(d_tag: &str) -> (String, String) {
    if is_v3_dtag(d_tag) {
        let mut parts = d_tag.splitn(3, '/');
        let tier = parts.next().unwrap_or("public");
        let visibility = tier.to_string();

        let scope = match visibility.as_str() {
            "personal" | "group" | "circle" => parts.next().unwrap_or("").to_string(),
            _ => String::new(),
        };
        (visibility, scope)
    } else {
        ("public".to_string(), String::new())
    }
}

/// Build a v0.3 d-tag: `{tier}/{topic}` or `{tier}/{scope}/{topic}`.
pub fn build_dtag(visibility: &str, scope: &str, topic: &str) -> String {
    match visibility {
        "public" | "private" => format!("{visibility}/{topic}"),
        "personal" | "group" | "circle" => {
            if scope.is_empty() {
                format!("{visibility}/{topic}")
            } else {
                format!("{visibility}/{scope}/{topic}")
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
/// - `"private"` → `private/{topic}`
pub fn build_dtag_from_tier(tier: &str, author_pubkey_hex: &str, topic: &str) -> String {
    if tier == "public" {
        build_dtag("public", "", topic)
    } else if let Some(group_id) = tier.strip_prefix("group:") {
        build_dtag("group", group_id, topic)
    } else if tier == "group" {
        build_dtag("group", "", topic)
    } else if tier == "personal" {
        build_dtag("personal", author_pubkey_hex, topic)
    } else if tier == "private" {
        build_dtag("private", "", topic)
    } else if let Some(circle_id) = tier.strip_prefix("circle:") {
        build_dtag("circle", circle_id, topic)
    } else {
        build_dtag("public", "", topic)
    }
}

/// Extract the base tier (public/group/personal/private) without scope suffix.
pub fn base_tier(tier: &str) -> &str {
    if tier.starts_with("group") {
        "group"
    } else {
        tier
    }
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
        assert!(!is_v3_dtag("unknown/topic"));
    }

    #[test]
    fn test_dtag_topic_v3() {
        assert_eq!(
            dtag_topic("public/rust-error-handling"),
            Some("rust-error-handling")
        );
        assert_eq!(
            dtag_topic("public/rust/error-handling"),
            Some("rust/error-handling")
        );
        assert_eq!(
            dtag_topic("private/agent-reasoning"),
            Some("agent-reasoning")
        );
        assert_eq!(dtag_topic("personal/abc123/ssh-config"), Some("ssh-config"));
        assert_eq!(dtag_topic("group/techteam/deployment"), Some("deployment"));
        assert_eq!(dtag_topic("circle/abc123/notes"), Some("notes"));
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
    fn test_build_dtag() {
        assert_eq!(
            build_dtag("public", "", "rust-error-handling"),
            "public/rust-error-handling"
        );
        assert_eq!(
            build_dtag("private", "", "agent-reasoning"),
            "private/agent-reasoning"
        );
        assert_eq!(
            build_dtag("personal", "abc123", "ssh-config"),
            "personal/abc123/ssh-config"
        );
        assert_eq!(
            build_dtag("group", "techteam", "deployment"),
            "group/techteam/deployment"
        );
    }

    #[test]
    fn test_build_dtag_from_tier() {
        assert_eq!(build_dtag_from_tier("public", "", "topic"), "public/topic");
        assert_eq!(
            build_dtag_from_tier("private", "", "topic"),
            "private/topic"
        );
        assert_eq!(
            build_dtag_from_tier("personal", "abc123", "topic"),
            "personal/abc123/topic"
        );
        assert_eq!(
            build_dtag_from_tier("group:techteam", "", "topic"),
            "group/techteam/topic"
        );
    }

    #[test]
    fn test_base_tier() {
        assert_eq!(base_tier("public"), "public");
        assert_eq!(base_tier("personal"), "personal");
        assert_eq!(base_tier("private"), "private");
        assert_eq!(base_tier("group"), "group");
        assert_eq!(base_tier("group:techteam"), "group");
    }
}
