use std::collections::HashMap;

use tracing::warn;

use nomen_db::CollectedMessageRecord;

use super::types::{ConsolidationMessage, GroupKey, TIME_WINDOW_SECS};

pub(crate) trait ConsolidationMessageLike {
    fn sender(&self) -> &str;
    fn source(&self) -> &str;
    fn thread_id(&self) -> &str;
    fn created_at_ts(&self) -> i64;
    fn primary_container_id(&self) -> String;
}

impl ConsolidationMessageLike for ConsolidationMessage {
    fn sender(&self) -> &str {
        &self.sender
    }
    fn source(&self) -> &str {
        &self.source
    }
    fn thread_id(&self) -> &str {
        &self.thread
    }
    fn created_at_ts(&self) -> i64 {
        self.created_at_ts
    }
    fn primary_container_id(&self) -> String {
        self.container.clone()
    }
}

impl ConsolidationMessageLike for CollectedMessageRecord {
    fn sender(&self) -> &str {
        self.sender_id.as_deref().unwrap_or("")
    }
    fn source(&self) -> &str {
        self.platform.as_deref().unwrap_or("")
    }
    fn thread_id(&self) -> &str {
        self.thread_id.as_deref().unwrap_or("")
    }
    fn created_at_ts(&self) -> i64 {
        self.created_at
    }
    fn primary_container_id(&self) -> String {
        if let Some(thread) = self.thread_id.as_deref().filter(|s| !s.is_empty()) {
            let chat = self.chat_id.as_deref().unwrap_or("");
            if chat.is_empty() {
                thread.to_string()
            } else {
                format!("{chat}/{thread}")
            }
        } else {
            self.chat_id.clone().unwrap_or_default()
        }
    }
}

/// Derive the primary conversation container identity from a message record.
pub(crate) fn primary_container_id<T: ConsolidationMessageLike>(msg: &T) -> String {
    msg.primary_container_id()
}

/// Derive a semantic topic name from a batch of messages.
///
/// Uses sender plus the current primary conversation-container identity to
/// produce topics using canonical conversation hierarchy.
pub fn derive_topic_from_messages<T: ConsolidationMessageLike>(messages: &[T]) -> String {
    let mut senders: Vec<&str> = messages.iter().map(|m| m.sender()).collect();
    senders.sort();
    senders.dedup();

    let channel = messages
        .first()
        .map(primary_container_id)
        .unwrap_or_else(|| "general".to_string());
    let channel = if channel.is_empty() {
        "general".to_string()
    } else {
        channel
    };

    if senders.len() == 1 {
        let sender = senders[0];
        // Clean up sender name for use as a topic component
        let sender_clean = sanitize_topic_component(sender);
        format!("user/{sender_clean}/conversation")
    } else {
        let channel_clean = sanitize_topic_component(&channel);
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
/// - DM-like messages -> `personal`
/// - Group/container messages -> `group`
/// - Public/CLI/other -> `public`
///
/// Uses canonical conversation hierarchy to derive privacy/group tier.
pub(crate) fn derive_tier_from_messages<T: ConsolidationMessageLike>(messages: &[T]) -> String {
    // Check sources — if any message is from a DM-like source, treat as personal
    let has_dm = messages.iter().any(|m| {
        let container = primary_container_id(m);
        (m.source() == "nostr" && (container.is_empty() || container == "dm"))
            || m.source() == "telegram_dm"
            || m.source() == "dm"
    });

    let has_group = messages.iter().any(|m| {
        let container = primary_container_id(m);
        !container.is_empty()
            && container != "dm"
            && container != "general"
            && (m.source() == "nostr"
                || m.source() == "telegram"
                || m.source().starts_with("group"))
    });

    if has_dm {
        "personal".to_string()
    } else if has_group {
        "group".to_string()
    } else {
        "public".to_string()
    }
}

/// Enforce cross-group consolidation guard: personal/private sources must never
/// produce group or public tier memories. Returns the tier, potentially downgraded.
pub(crate) fn enforce_tier_guard(derived_tier: &str, source_tier: &str) -> String {
    match source_tier {
        "personal" | "private" => {
            // Personal/private sources can only produce personal memories
            if derived_tier != "personal" && derived_tier != "private" {
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

/// Extract a forum topic suffix from a sender string (e.g. Telegram forum topics).
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

/// Resolve the scope for a single consolidation message.
///
/// Scope determines the privacy/group boundary:
/// - DM sources -> "personal"
/// - Group sources with a specific conversation container -> `group:{container}`
/// - Everything else -> "public"
///
/// This is used to partition messages so that different scopes are never
/// consolidated together (cross-group guard, TODO #7).
pub(crate) fn resolve_message_scope<T: ConsolidationMessageLike>(msg: &T) -> String {
    let container = primary_container_id(msg);
    let is_dm = msg.source() == "dm"
        || msg.source() == "telegram_dm"
        || (msg.source() == "nostr" && (container.is_empty() || container == "dm"));

    if is_dm {
        return "personal".to_string();
    }

    let is_group = !container.is_empty()
        && container != "dm"
        && container != "general"
        && (msg.source() == "nostr"
            || msg.source() == "telegram"
            || msg.source().starts_with("group"));

    if is_group {
        return format!("group:{container}");
    }

    "public".to_string()
}

/// Group messages by sender/container + time window (4-hour blocks) + scope.
///
/// The scope field in the group key ensures messages from different
/// visibility/group scopes are never mixed in the same consolidation
/// batch, preventing information leakage across tiers (TODO #7).
pub(crate) fn group_messages<T: ConsolidationMessageLike + Clone>(
    messages: Vec<T>,
) -> HashMap<GroupKey, Vec<T>> {
    let mut groups: HashMap<GroupKey, Vec<T>> = HashMap::new();

    for msg in messages {
        let window = msg.created_at_ts() / TIME_WINDOW_SECS;

        // Group by sender for DMs, by primary conversation container for
        // group messages. For forum-style chats (Telegram topics), extract the
        // topic suffix from the sender field and append it to the container
        // identity so each topic is consolidated independently.
        let container = primary_container_id(&msg);
        let identity = if container.is_empty() || container == "general" {
            msg.sender().to_string()
        } else if !msg.thread_id().is_empty() {
            container
        } else {
            let mut id = container;
            if let Some(topic_suffix) = extract_topic_suffix(msg.sender()) {
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
        assert_eq!(extract_topic_suffix("telegram:group:-1003821690204"), None);
        // No topic — DM sender
        assert_eq!(extract_topic_suffix("telegram:60996061"), None);
        // Empty
        assert_eq!(extract_topic_suffix(""), None);
    }
}
