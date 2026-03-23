use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;
use tracing::debug;

use nomen_core::config::GroupConfig;
use nomen_core::groups::*;

/// DB-specific wrapper for Group with SurrealValue derive.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct DbGroup {
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

impl From<DbGroup> for Group {
    fn from(g: DbGroup) -> Self {
        Group {
            id: g.id,
            name: g.name,
            parent: g.parent,
            members: g.members,
            relay: g.relay,
            nostr_group: g.nostr_group,
            created_at: g.created_at,
        }
    }
}

impl From<&Group> for DbGroup {
    fn from(g: &Group) -> Self {
        DbGroup {
            id: g.id.clone(),
            name: g.name.clone(),
            parent: g.parent.clone(),
            members: g.members.clone(),
            relay: g.relay.clone(),
            nostr_group: g.nostr_group.clone(),
            created_at: g.created_at.clone(),
        }
    }
}

/// Extension trait for GroupStore DB operations.
pub trait GroupStoreExt {
    fn load(config_groups: &[GroupConfig], db: &Surreal<Db>) -> impl std::future::Future<Output = Result<GroupStore>> + Send;
}

impl GroupStoreExt for GroupStore {
    /// Load groups from config file entries and SurrealDB.
    async fn load(config_groups: &[GroupConfig], db: &Surreal<Db>) -> Result<GroupStore> {
        let mut store = GroupStore::from_config(config_groups);

        // Load from DB (these may overlap with config; DB wins for members)
        let db_groups: Vec<DbGroup> = db
            .query("SELECT meta::id(id) AS id, name, parent, members, relay, nostr_group, created_at FROM nomen_group")
            .await?
            .take(0)?;

        for dbg in db_groups {
            let group: Group = dbg.into();
            if let Some(existing) = store.groups_mut().iter_mut().find(|g| g.id == group.id) {
                // DB overrides config for mutable fields
                existing.members = group.members;
                if !group.nostr_group.is_empty() {
                    existing.nostr_group = group.nostr_group;
                }
                if !group.relay.is_empty() {
                    existing.relay = group.relay;
                }
            } else {
                store.groups_mut().push(group);
            }
        }

        debug!("Loaded {} groups", store.list().len());
        Ok(store)
    }
}

// -- Group CRUD (DB operations) --

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
    if !id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        bail!("Group id must be alphanumeric with dots/hyphens/underscores: {id}");
    }

    let parent = derive_parent(id).unwrap_or_default();
    let now = chrono::Utc::now().to_rfc3339();

    let group = Group {
        id: id.to_string(),
        name: name.to_string(),
        parent,
        members: members.to_vec(),
        relay: relay.unwrap_or("").to_string(),
        nostr_group: nostr_group.unwrap_or("").to_string(),
        created_at: now,
    };

    // Check if exists
    let existing: Option<DbGroup> = db
        .query("SELECT meta::id(id) AS id, name, parent, members, relay, nostr_group, created_at FROM nomen_group WHERE meta::id(id) = $id LIMIT 1")
        .bind(("id", id.to_string()))
        .await?
        .take(0)?;

    if existing.is_some() {
        bail!("Group already exists: {id}");
    }

    // Serialize members as JSON array string for SurrealDB
    let members_json = serde_json::to_string(&group.members)?;
    let record_id = format!("nomen_group:`{}`", group.id);
    let sql = format!("CREATE {record_id} SET name = $name, parent = $parent, members = {members_json}, relay = $relay, nostr_group = $nostr_group, created_at = $created_at");
    db.query(&sql)
        .bind(("name", group.name))
        .bind(("parent", group.parent))
        .bind(("relay", group.relay))
        .bind(("nostr_group", group.nostr_group))
        .bind(("created_at", group.created_at))
        .await?
        .check()?;

    debug!("Created group: {id}");
    Ok(())
}

/// List all groups from SurrealDB.
pub async fn list_groups(db: &Surreal<Db>) -> Result<Vec<Group>> {
    let db_groups: Vec<DbGroup> = db.query("SELECT meta::id(id) AS id, name, parent, members, relay, nostr_group, created_at FROM nomen_group ORDER BY id").await?.check()?.take(0)?;
    Ok(db_groups.into_iter().map(Into::into).collect())
}

/// Get members of a group.
pub async fn get_members(db: &Surreal<Db>, group_id: &str) -> Result<Vec<String>> {
    let group: Option<DbGroup> = db
        .query("SELECT meta::id(id) AS id, name, parent, members, relay, nostr_group, created_at FROM nomen_group WHERE meta::id(id) = $id LIMIT 1")
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
    let group: Option<DbGroup> = db
        .query("SELECT meta::id(id) AS id, name, parent, members, relay, nostr_group, created_at FROM nomen_group WHERE meta::id(id) = $id LIMIT 1")
        .bind(("id", group_id.to_string()))
        .await?
        .take(0)?;

    match group {
        Some(g) => {
            if g.members.contains(&npub.to_string()) {
                bail!("{npub} is already a member of {group_id}");
            }
            db.query("UPDATE nomen_group SET members = array::push(members, $npub) WHERE meta::id(id) = $id")
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
    let group: Option<DbGroup> = db
        .query("SELECT meta::id(id) AS id, name, parent, members, relay, nostr_group, created_at FROM nomen_group WHERE meta::id(id) = $id LIMIT 1")
        .bind(("id", group_id.to_string()))
        .await?
        .take(0)?;

    match group {
        Some(g) => {
            if !g.members.contains(&npub.to_string()) {
                bail!("{npub} is not a member of {group_id}");
            }
            db.query("UPDATE nomen_group SET members = array::complement(members, [$npub]) WHERE meta::id(id) = $id")
                .bind(("id", group_id.to_string()))
                .bind(("npub", npub.to_string()))
                .await?
                .check()?;
            Ok(())
        }
        None => bail!("Group not found: {group_id}"),
    }
}
