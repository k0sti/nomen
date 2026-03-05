use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use tracing::debug;

use crate::config::GroupConfig;

/// A group record as stored in SurrealDB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub members: Vec<String>,
    pub relay: Option<String>,
    pub nostr_group: Option<String>,
    pub created_at: String,
}

/// GroupStore loads groups from config and DB, provides membership and scope queries.
pub struct GroupStore {
    groups: Vec<Group>,
}

impl GroupStore {
    /// Load groups from config file entries and SurrealDB.
    pub async fn load(config_groups: &[GroupConfig], db: &Surreal<Db>) -> Result<Self> {
        let mut groups = Vec::new();

        // Load from config
        for cg in config_groups {
            let parent = derive_parent(&cg.id);
            groups.push(Group {
                id: cg.id.clone(),
                name: cg.name.clone(),
                parent,
                members: cg.members.clone(),
                relay: cg.relay.clone(),
                nostr_group: cg.nostr_group.clone(),
                created_at: chrono::Utc::now().to_rfc3339(),
            });
        }

        // Load from DB (these may overlap with config; DB wins for members)
        let db_groups: Vec<Group> = db
            .query("SELECT * FROM nomen_group")
            .await?
            .take(0)?;

        for dbg in db_groups {
            if let Some(existing) = groups.iter_mut().find(|g| g.id == dbg.id) {
                // DB overrides config for mutable fields
                existing.members = dbg.members;
                if dbg.nostr_group.is_some() {
                    existing.nostr_group = dbg.nostr_group;
                }
                if dbg.relay.is_some() {
                    existing.relay = dbg.relay;
                }
            } else {
                groups.push(dbg);
            }
        }

        debug!("Loaded {} groups", groups.len());
        Ok(Self { groups })
    }

    /// Load from config only (no DB).
    pub fn from_config(config_groups: &[GroupConfig]) -> Self {
        let groups = config_groups
            .iter()
            .map(|cg| Group {
                id: cg.id.clone(),
                name: cg.name.clone(),
                parent: derive_parent(&cg.id),
                members: cg.members.clone(),
                relay: cg.relay.clone(),
                nostr_group: cg.nostr_group.clone(),
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .collect();
        Self { groups }
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
            .find(|g| g.nostr_group.as_deref() == Some(nostr_group))
            .map(|g| g.id.as_str())
    }

    /// Resolve a hierarchical scope to a NIP-29 nostr_group (h-tag value).
    pub fn resolve_scope_to_nostr_group(&self, scope: &str) -> Option<&str> {
        self.groups
            .iter()
            .find(|g| g.id == scope)
            .and_then(|g| g.nostr_group.as_deref())
    }
}

// ── Group CRUD (DB operations) ──────────────────────────────────────

/// Create a new group in SurrealDB.
pub async fn create_group(
    db: &Surreal<Db>,
    id: &str,
    name: &str,
    members: &[String],
    nostr_group: Option<&str>,
    relay: Option<&str>,
) -> Result<()> {
    // Validate id: alphanumeric + dots only
    if !id.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_') {
        bail!("Group id must be alphanumeric with dots/hyphens/underscores: {id}");
    }

    let parent = derive_parent(id);
    let now = chrono::Utc::now().to_rfc3339();

    let group = Group {
        id: id.to_string(),
        name: name.to_string(),
        parent,
        members: members.to_vec(),
        relay: relay.map(|s| s.to_string()),
        nostr_group: nostr_group.map(|s| s.to_string()),
        created_at: now,
    };

    // Check if exists
    let existing: Option<Group> = db
        .query("SELECT * FROM nomen_group WHERE id = $id LIMIT 1")
        .bind(("id", id.to_string()))
        .await?
        .take(0)?;

    if existing.is_some() {
        bail!("Group already exists: {id}");
    }

    db.query("CREATE nomen_group CONTENT $group")
        .bind(("group", group))
        .await?
        .check()?;

    debug!("Created group: {id}");
    Ok(())
}

/// List all groups from SurrealDB.
pub async fn list_groups(db: &Surreal<Db>) -> Result<Vec<Group>> {
    let groups: Vec<Group> = db.query("SELECT * FROM nomen_group ORDER BY id").await?.take(0)?;
    Ok(groups)
}

/// Get members of a group.
pub async fn get_members(db: &Surreal<Db>, group_id: &str) -> Result<Vec<String>> {
    let group: Option<Group> = db
        .query("SELECT * FROM nomen_group WHERE id = $id LIMIT 1")
        .bind(("id", group_id.to_string()))
        .await?
        .take(0)?;

    match group {
        Some(g) => Ok(g.members),
        None => bail!("Group not found: {group_id}"),
    }
}

/// Add a member to a group.
pub async fn add_member(db: &Surreal<Db>, group_id: &str, npub: &str) -> Result<()> {
    let group: Option<Group> = db
        .query("SELECT * FROM nomen_group WHERE id = $id LIMIT 1")
        .bind(("id", group_id.to_string()))
        .await?
        .take(0)?;

    match group {
        Some(g) => {
            if g.members.contains(&npub.to_string()) {
                bail!("{npub} is already a member of {group_id}");
            }
            db.query("UPDATE nomen_group SET members = array::push(members, $npub) WHERE id = $id")
                .bind(("id", group_id.to_string()))
                .bind(("npub", npub.to_string()))
                .await?
                .check()?;
            Ok(())
        }
        None => bail!("Group not found: {group_id}"),
    }
}

/// Remove a member from a group.
pub async fn remove_member(db: &Surreal<Db>, group_id: &str, npub: &str) -> Result<()> {
    let group: Option<Group> = db
        .query("SELECT * FROM nomen_group WHERE id = $id LIMIT 1")
        .bind(("id", group_id.to_string()))
        .await?
        .take(0)?;

    match group {
        Some(g) => {
            if !g.members.contains(&npub.to_string()) {
                bail!("{npub} is not a member of {group_id}");
            }
            db.query("UPDATE nomen_group SET members = array::remove(members, array::find_index(members, $npub)) WHERE id = $id")
                .bind(("id", group_id.to_string()))
                .bind(("npub", npub.to_string()))
                .await?
                .check()?;
            Ok(())
        }
        None => bail!("Group not found: {group_id}"),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Derive parent scope from a dot-separated id.
/// "atlantislabs.engineering.infra" -> Some("atlantislabs.engineering")
/// "atlantislabs" -> None
fn derive_parent(id: &str) -> Option<String> {
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
        let store = GroupStore::from_config(&[
            GroupConfig {
                id: "atlantislabs.engineering".to_string(),
                name: "Engineering".to_string(),
                members: vec![],
                nostr_group: Some("techteam".to_string()),
                relay: None,
            },
        ]);

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
