//! Session ID model for automatic tier/scope resolution.
//!
//! A session ID encodes the target + channel + tier in a single identifier,
//! simplifying the API by eliminating the need to specify tier and scope separately.
//!
//! Formats:
//!   - `"public"` → public tier, empty scope
//!   - `"npub1..."` → private tier, scope = npub hex
//!   - `"<channel>:npub1..."` → private tier with explicit channel
//!   - `"<group_name>"` → group tier (resolved via GroupStore)
//!   - `"<channel>:<group_name>"` → group tier with explicit channel

use anyhow::{Result, bail};
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};

use crate::groups::GroupStore;

/// A resolved session with tier, scope, channel, and participant info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedSession {
    /// Original session ID string.
    pub session_id: String,
    /// Visibility tier: "public", "group", or "private".
    pub tier: String,
    /// Scope: empty for public, group_id for group, npub hex for private.
    pub scope: String,
    /// Delivery channel (e.g. "nostr", "telegram").
    pub channel: String,
    /// Optional group ID (if this is a group session).
    pub group_id: String,
    /// Participants in this session (npub hex strings).
    pub participants: Vec<String>,
}

/// SurrealDB session record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    #[serde(default, deserialize_with = "crate::db::deserialize_thing_as_string")]
    pub id: String,
    pub session_id: String,
    pub tier: String,
    pub scope: String,
    pub channel: String,
    pub group_id: String,
    pub participants: Vec<String>,
    pub created_at: String,
    pub last_active: String,
}

/// Resolve a session ID to its tier, scope, channel, and participants.
///
/// Resolution logic:
/// 1. `"public"` → public tier
/// 2. `"npub1..."` → private DM (scope = npub hex)
/// 3. `"<channel>:npub1..."` → private DM with explicit channel
/// 4. `"<name>"` where name matches a group → group tier
/// 5. `"<channel>:<name>"` where name matches a group → group tier with channel
pub fn resolve_session(
    session_id: &str,
    groups: &GroupStore,
    default_channel: &str,
) -> Result<ResolvedSession> {
    // Case 1: Public
    if session_id == "public" {
        return Ok(ResolvedSession {
            session_id: session_id.to_string(),
            tier: "public".to_string(),
            scope: String::new(),
            channel: default_channel.to_string(),
            group_id: String::new(),
            participants: Vec::new(),
        });
    }

    // Case 2: Plain npub (no channel prefix)
    if session_id.starts_with("npub1") {
        let hex = PublicKey::from_bech32(session_id)
            .map(|pk| pk.to_hex())
            .unwrap_or_else(|_| session_id.to_string());
        return Ok(ResolvedSession {
            session_id: session_id.to_string(),
            tier: "private".to_string(),
            scope: hex.clone(),
            channel: default_channel.to_string(),
            group_id: String::new(),
            participants: vec![hex],
        });
    }

    // Check for channel prefix: "channel:target"
    if let Some(colon_pos) = session_id.find(':') {
        let prefix = &session_id[..colon_pos];
        let target = &session_id[colon_pos + 1..];

        if target.is_empty() {
            bail!("Session ID has empty target after channel prefix: {session_id}");
        }

        // Case 3: "<channel>:npub1..."
        if target.starts_with("npub1") {
            let hex = PublicKey::from_bech32(target)
                .map(|pk| pk.to_hex())
                .unwrap_or_else(|_| target.to_string());
            return Ok(ResolvedSession {
                session_id: session_id.to_string(),
                tier: "private".to_string(),
                scope: hex.clone(),
                channel: prefix.to_string(),
                group_id: String::new(),
                participants: vec![hex],
            });
        }

        // Case 5: "<channel>:<group_name>" — resolve group
        if let Some(group) = groups.get(target) {
            return Ok(ResolvedSession {
                session_id: session_id.to_string(),
                tier: "group".to_string(),
                scope: group.id.clone(),
                channel: prefix.to_string(),
                group_id: group.id.clone(),
                participants: group.members.clone(),
            });
        }

        // Also check if target is a nostr_group h-tag value
        if let Some(scope) = groups.resolve_nostr_group(target) {
            let group = groups.get(scope);
            return Ok(ResolvedSession {
                session_id: session_id.to_string(),
                tier: "group".to_string(),
                scope: scope.to_string(),
                channel: prefix.to_string(),
                group_id: scope.to_string(),
                participants: group.map(|g| g.members.clone()).unwrap_or_default(),
            });
        }

        bail!("Cannot resolve session: unknown group '{target}' in session ID '{session_id}'");
    }

    // Case 4: Plain group name (no channel prefix)
    if let Some(group) = groups.get(session_id) {
        return Ok(ResolvedSession {
            session_id: session_id.to_string(),
            tier: "group".to_string(),
            scope: group.id.clone(),
            channel: default_channel.to_string(),
            group_id: group.id.clone(),
            participants: group.members.clone(),
        });
    }

    // Also check nostr_group h-tag mapping
    if let Some(scope) = groups.resolve_nostr_group(session_id) {
        let group = groups.get(scope);
        return Ok(ResolvedSession {
            session_id: session_id.to_string(),
            tier: "group".to_string(),
            scope: scope.to_string(),
            channel: default_channel.to_string(),
            group_id: scope.to_string(),
            participants: group.map(|g| g.members.clone()).unwrap_or_default(),
        });
    }

    bail!(
        "Cannot resolve session ID: '{session_id}'. \
         Expected: 'public', 'npub1...', '<group_name>', or '<channel>:<target>'"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GroupConfig;

    fn test_groups() -> GroupStore {
        GroupStore::from_config(&[
            GroupConfig {
                id: "techteam".to_string(),
                name: "Tech Team".to_string(),
                members: vec!["npub1abc".to_string()],
                nostr_group: Some("inner-circle".to_string()),
                relay: None,
            },
        ])
    }

    #[test]
    fn test_resolve_public() {
        let groups = test_groups();
        let s = resolve_session("public", &groups, "nostr").unwrap();
        assert_eq!(s.tier, "public");
        assert_eq!(s.scope, "");
        assert_eq!(s.channel, "nostr");
    }

    #[test]
    fn test_resolve_group() {
        let groups = test_groups();
        let s = resolve_session("techteam", &groups, "nostr").unwrap();
        assert_eq!(s.tier, "group");
        assert_eq!(s.scope, "techteam");
        assert_eq!(s.group_id, "techteam");
        assert_eq!(s.channel, "nostr");
    }

    #[test]
    fn test_resolve_group_with_channel() {
        let groups = test_groups();
        let s = resolve_session("telegram:techteam", &groups, "nostr").unwrap();
        assert_eq!(s.tier, "group");
        assert_eq!(s.scope, "techteam");
        assert_eq!(s.channel, "telegram");
    }

    #[test]
    fn test_resolve_nostr_group_alias() {
        let groups = test_groups();
        let s = resolve_session("inner-circle", &groups, "nostr").unwrap();
        assert_eq!(s.tier, "group");
        assert_eq!(s.scope, "techteam");
    }

    #[test]
    fn test_resolve_unknown_fails() {
        let groups = test_groups();
        assert!(resolve_session("unknown-thing", &groups, "nostr").is_err());
    }
}
