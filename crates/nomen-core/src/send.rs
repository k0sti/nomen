//! Send types: target, options, result, recipient parsing.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Target for a sent message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SendTarget {
    /// Private DM to an npub (NIP-17 gift-wrapped, NIP-44 encrypted).
    Npub(String),
    /// Group message (NIP-29 kind 9 with h-tag).
    Group(String),
    /// Public broadcast (kind 1 note).
    Public,
}

/// Options for sending a message.
#[derive(Debug, Clone)]
pub struct SendOptions {
    pub target: SendTarget,
    pub content: String,
    /// Delivery channel (e.g. "nostr", "telegram"). Defaults to "nostr".
    pub channel: Option<String>,
    /// Optional metadata (JSON object).
    pub metadata: Option<Value>,
}

/// Result of a send operation.
#[derive(Debug, Clone, Serialize)]
pub struct SendResult {
    pub event_id: String,
    pub accepted: Vec<String>,
    pub rejected: Vec<(String, String)>,
}

impl SendResult {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.accepted.is_empty() {
            parts.push(format!("accepted by {} relay(s)", self.accepted.len()));
        }
        if !self.rejected.is_empty() {
            parts.push(format!("rejected by {} relay(s)", self.rejected.len()));
        }
        if parts.is_empty() {
            "no relay responses".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// Parse a recipient string into a SendTarget.
///
/// - `"npub1..."` -> Npub
/// - `"group:<id>"` -> Group
/// - `"public"` -> Public
pub fn parse_recipient(recipient: &str) -> Result<SendTarget> {
    if recipient == "public" {
        Ok(SendTarget::Public)
    } else if recipient.starts_with("npub1") {
        Ok(SendTarget::Npub(recipient.to_string()))
    } else if let Some(group_id) = recipient.strip_prefix("group:") {
        if group_id.is_empty() {
            bail!("Group ID cannot be empty");
        }
        Ok(SendTarget::Group(group_id.to_string()))
    } else {
        bail!("Invalid recipient: {recipient}. Use npub1..., group:<id>, or 'public'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_recipient() {
        assert!(matches!(
            parse_recipient("public").unwrap(),
            SendTarget::Public
        ));

        assert!(matches!(
            parse_recipient("npub1abc123").unwrap(),
            SendTarget::Npub(ref s) if s == "npub1abc123"
        ));

        assert!(matches!(
            parse_recipient("group:techteam").unwrap(),
            SendTarget::Group(ref s) if s == "techteam"
        ));

        assert!(parse_recipient("group:").is_err());
        assert!(parse_recipient("invalid").is_err());
    }
}
