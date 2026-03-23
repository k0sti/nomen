use serde::{Deserialize, Serialize};

use crate::config::GroupConfig;

/// A group record.
///
/// Note: The SurrealDB-specific `SurrealValue` derive is only present in the main
/// crate's re-export. This core version is pure data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent: String,
    pub members: Vec<String>,
    #[serde(default)]
    pub relay: String,
    #[serde(default)]
    pub nostr_group: String,
    pub created_at: String,
}

/// GroupStore loads groups from config, provides membership and scope queries.
pub struct GroupStore {
    groups: Vec<Group>,
}

impl GroupStore {
    /// Create an empty GroupStore (for testing).
    pub fn empty() -> Self {
        Self { groups: Vec::new() }
    }

    /// Load from config only (no DB).
    pub fn from_config(config_groups: &[GroupConfig]) -> Self {
        let groups = config_groups
            .iter()
            .map(|cg| Group {
                id: cg.id.clone(),
                name: cg.name.clone(),
                parent: derive_parent(&cg.id).unwrap_or_default(),
                members: cg.members.clone(),
                relay: cg.relay.clone().unwrap_or_default(),
                nostr_group: cg.nostr_group.clone().unwrap_or_default(),
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .collect();
        Self { groups }
    }

    /// Create a GroupStore from a pre-built list of groups.
    pub fn from_groups(groups: Vec<Group>) -> Self {
        Self { groups }
    }

    /// Get a mutable reference to the groups vec (for DB loading in the main crate).
    pub fn groups_mut(&mut self) -> &mut Vec<Group> {
        &mut self.groups
    }

    /// Check if npub is a member of the given scope.
    /// Does NOT walk up hierarchy — membership is explicit per level.
    pub fn is_member(&self, scope: &str, npub: &str) -> bool {
        if let Some(group) = self.groups.iter().find(|g| g.id == scope) {
            group.members.iter().any(|m| m == npub)
        } else {
            false
        }
    }

    /// Expand all scopes an npub can access.
    /// Returns: all group scopes where the npub is an explicit member,
    /// plus all child scopes of those groups where the npub is also a member.
    pub fn expand_scopes(&self, npub: &str) -> Vec<String> {
        let mut scopes = Vec::new();
        for group in &self.groups {
            if group.members.iter().any(|m| m == npub) {
                scopes.push(group.id.clone());
            }
        }
        scopes
    }

    /// Get all groups.
    pub fn list(&self) -> &[Group] {
        &self.groups
    }

    /// Get a group by id.
    pub fn get(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Resolve a NIP-29 nostr_group (h-tag value) to a hierarchical scope.
    pub fn resolve_nostr_group(&self, nostr_group: &str) -> Option<&str> {
        self.groups
            .iter()
            .find(|g| !g.nostr_group.is_empty() && g.nostr_group == nostr_group)
            .map(|g| g.id.as_str())
    }

    /// Resolve a hierarchical scope to a NIP-29 nostr_group (h-tag value).
    pub fn resolve_scope_to_nostr_group(&self, scope: &str) -> Option<&str> {
        self.groups
            .iter()
            .find(|g| g.id == scope && !g.nostr_group.is_empty())
            .map(|g| g.nostr_group.as_str())
    }
}

// -- Helpers --

/// Derive parent scope from a dot-separated id.
/// "atlantislabs.engineering.infra" -> Some("atlantislabs.engineering")
/// "atlantislabs" -> None
pub fn derive_parent(id: &str) -> Option<String> {
    id.rfind('.').map(|pos| id[..pos].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_parent() {
        assert_eq!(derive_parent("atlantislabs"), None);
        assert_eq!(
            derive_parent("atlantislabs.engineering"),
            Some("atlantislabs".to_string())
        );
        assert_eq!(
            derive_parent("atlantislabs.engineering.infra"),
            Some("atlantislabs.engineering".to_string())
        );
    }

    #[test]
    fn test_is_member() {
        let store = GroupStore::from_config(&[
            GroupConfig {
                id: "atlantislabs".to_string(),
                name: "Atlantis Labs".to_string(),
                members: vec!["npub1abc".to_string(), "npub1def".to_string()],
                nostr_group: None,
                relay: None,
            },
            GroupConfig {
                id: "atlantislabs.engineering".to_string(),
                name: "Engineering".to_string(),
                members: vec!["npub1abc".to_string()],
                nostr_group: Some("techteam".to_string()),
                relay: None,
            },
        ]);

        assert!(store.is_member("atlantislabs", "npub1abc"));
        assert!(store.is_member("atlantislabs", "npub1def"));
        assert!(!store.is_member("atlantislabs", "npub1xyz"));
        assert!(store.is_member("atlantislabs.engineering", "npub1abc"));
        assert!(!store.is_member("atlantislabs.engineering", "npub1def"));
    }

    #[test]
    fn test_expand_scopes() {
        let store = GroupStore::from_config(&[
            GroupConfig {
                id: "atlantislabs".to_string(),
                name: "Atlantis Labs".to_string(),
                members: vec!["npub1abc".to_string(), "npub1def".to_string()],
                nostr_group: None,
                relay: None,
            },
            GroupConfig {
                id: "atlantislabs.engineering".to_string(),
                name: "Engineering".to_string(),
                members: vec!["npub1abc".to_string()],
                nostr_group: None,
                relay: None,
            },
        ]);

        let scopes = store.expand_scopes("npub1abc");
        assert_eq!(scopes, vec!["atlantislabs", "atlantislabs.engineering"]);

        let scopes = store.expand_scopes("npub1def");
        assert_eq!(scopes, vec!["atlantislabs"]);

        let scopes = store.expand_scopes("npub1xyz");
        assert!(scopes.is_empty());
    }

    #[test]
    fn test_nostr_group_mapping() {
        let store = GroupStore::from_config(&[GroupConfig {
            id: "atlantislabs.engineering".to_string(),
            name: "Engineering".to_string(),
            members: vec![],
            nostr_group: Some("techteam".to_string()),
            relay: None,
        }]);

        assert_eq!(
            store.resolve_nostr_group("techteam"),
            Some("atlantislabs.engineering")
        );
        assert_eq!(store.resolve_nostr_group("unknown"), None);
        assert_eq!(
            store.resolve_scope_to_nostr_group("atlantislabs.engineering"),
            Some("techteam")
        );
    }
}
