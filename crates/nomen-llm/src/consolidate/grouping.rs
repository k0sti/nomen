use std::collections::HashMap;

use tracing::warn;

use nomen_db::RawMessageRecord;

use super::types::{GroupKey, TIME_WINDOW_SECS};

/// Derive a semantic topic name from a batch of messages.
///
/// Uses the sender/channel info to produce topics like:
/// - `user/<sender>/<channel>` for private messages
/// - `group/<channel>/conversation` for group messages
/// - `conversation/<channel>` as fallback
pub fn derive_topic_from_messages(messages: &[RawMessageRecord]) -> String {
    let mut senders: Vec<&str> = messages.iter().map(|m| m.sender.as_str()).collect();
    senders.sort();
    senders.dedup();

    let channel = messages
        .first()
        .map(|m| m.channel.as_str())
        .unwrap_or("general");
    let channel = if channel.is_empty() {
        "general"
    } else {
        channel
    };

    if senders.len() == 1 {
        let sender = senders[0];
        // Clean up sender name for use as a topic component
        let sender_clean = sanitize_topic_component(sender);
        format!("user/{sender_clean}/conversation")
    } else {
        let channel_clean = sanitize_topic_component(channel);
        format!("group/{channel_clean}/conversation")
    }
}

/// Clean a string for use in a topic path (lowercase, replace non-alphanum with dash).
pub(crate) fn sanitize_topic_component(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    // Collapse multiple dashes
    let mut result = String::new();
    let mut prev_dash = false;
    for c in cleaned.chars() {
        if c == '-' {
            if !prev_dash {
                result.push(c);
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }
    result.trim_matches('-').to_string()
}

/// Derive the memory tier from a group of source messages.
///
/// - DM messages (source "nostr" with sender npub, no group channel) -> "personal"
/// - Group messages (channel matches a group pattern) -> "group"
/// - Public/CLI/other -> "public"
pub(crate) fn derive_tier_from_messages(messages: &[RawMessageRecord]) -> String {
    // Check sources — if any message is from a DM-like source, treat as personal
    let has_dm = messages.iter().any(|m| {
        // nostr DMs have source "nostr" and either empty channel or "dm" channel
        (m.source == "nostr" && (m.channel.is_empty() || m.channel == "dm"))
            || m.source == "telegram_dm"
            || m.source == "dm"
    });

    let has_group = messages.iter().any(|m| {
        // Group messages have a non-empty channel that isn't "dm" or "general"
        !m.channel.is_empty()
            && m.channel != "dm"
            && m.channel != "general"
            && (m.source == "nostr" || m.source == "telegram" || m.source.starts_with("group"))
    });

    if has_dm {
        "personal".to_string()
    } else if has_group {
        "group".to_string()
    } else {
        "public".to_string()
    }
}

/// Enforce cross-group consolidation guard: personal/internal sources must never
/// produce group or public tier memories. Returns the tier, potentially downgraded.
pub(crate) fn enforce_tier_guard(derived_tier: &str, source_tier: &str) -> String {
    match source_tier {
        "personal" | "internal" | "private" => {
            // Personal/internal sources can only produce personal memories
            if derived_tier != "personal" && derived_tier != "internal" && derived_tier != "private"
            {
                warn!(
                    derived = derived_tier,
                    "Cross-group guard: downgrading tier to personal (source is {source_tier})"
                );
            }
            "personal".to_string()
        }
        "group" => {
            // Group sources can produce group or personal, but not public
            if derived_tier == "public" {
                warn!("Cross-group guard: downgrading tier to group (source is group)");
                "group".to_string()
            } else {
                derived_tier.to_string()
            }
        }
        _ => derived_tier.to_string(),
    }
}

/// Extract a forum topic suffix from a sender string.
///
/// Telegram forum senders look like `telegram:group:-1003821690204:topic:9225`.
/// This returns `Some("topic:9225")` for such senders, `None` otherwise.
fn extract_topic_suffix(sender: &str) -> Option<String> {
    // Match ":topic:<id>" anywhere in the sender string
    if let Some(idx) = sender.find(":topic:") {
        let suffix = &sender[idx + 1..]; // skip the leading ':'
        // Validate it looks like "topic:<digits>"
        if let Some(id) = suffix.strip_prefix("topic:") {
            if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
                return Some(suffix.to_string());
            }
        }
    }
    None
}

/// Resolve the scope for a single raw message.
///
/// Scope determines the privacy/group boundary:
/// - DM sources -> "personal"
/// - Group sources with a specific channel -> "group:{channel}"
/// - Everything else -> "public"
///
/// This is used to partition messages so that different scopes are never
/// consolidated together (cross-group guard, TODO #7).
pub(crate) fn resolve_message_scope(msg: &RawMessageRecord) -> String {
    let is_dm = msg.source == "dm"
        || msg.source == "telegram_dm"
        || (msg.source == "nostr" && (msg.channel.is_empty() || msg.channel == "dm"));

    if is_dm {
        return "personal".to_string();
    }

    let is_group = !msg.channel.is_empty()
        && msg.channel != "dm"
        && msg.channel != "general"
        && (msg.source == "nostr" || msg.source == "telegram" || msg.source.starts_with("group"));

    if is_group {
        return format!("group:{}", msg.channel);
    }

    "public".to_string()
}

/// Group messages by sender + time window (4-hour blocks) + scope.
///
/// The scope field in the group key ensures messages from different
/// visibility/group scopes are never mixed in the same consolidation
/// batch, preventing information leakage across tiers (TODO #7).
pub(crate) fn group_messages(
    messages: Vec<RawMessageRecord>,
) -> HashMap<GroupKey, Vec<RawMessageRecord>> {
    let mut groups: HashMap<GroupKey, Vec<RawMessageRecord>> = HashMap::new();

    for msg in messages {
        let timestamp = chrono::DateTime::parse_from_rfc3339(&msg.created_at)
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        let window = timestamp / TIME_WINDOW_SECS;

        // Group by sender for DMs, by channel for group messages.
        // For forum-style chats (Telegram topics), extract the topic suffix
        // from the sender field and append it to the channel identity so each
        // topic is consolidated independently.
        let identity = if msg.channel.is_empty() || msg.channel == "general" {
            msg.sender.clone()
        } else {
            let mut id = msg.channel.clone();
            if let Some(topic_suffix) = extract_topic_suffix(&msg.sender) {
                id.push_str(&format!("/{topic_suffix}"));
            }
            id
        };

        // Resolve scope to prevent cross-group consolidation
        let scope = resolve_message_scope(&msg);

        let key = GroupKey {
            identity,
            window,
            scope,
        };
        groups.entry(key).or_default().push(msg);
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_topic_suffix() {
        // Telegram forum topic
        assert_eq!(
            extract_topic_suffix("telegram:group:-1003821690204:topic:9225"),
            Some("topic:9225".to_string())
        );
        // Different topic
        assert_eq!(
            extract_topic_suffix("telegram:group:-1003821690204:topic:8485"),
            Some("topic:8485".to_string())
        );
        // No topic — regular group sender
        assert_eq!(
            extract_topic_suffix("telegram:group:-1003821690204"),
            None
        );
        // No topic — DM sender
        assert_eq!(extract_topic_suffix("telegram:60996061"), None);
        // Empty
        assert_eq!(extract_topic_suffix(""), None);
    }
}
