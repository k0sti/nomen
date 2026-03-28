//! impl Nomen — admin operations: prune, group management, entities.

use anyhow::Result;

use crate::db;
use crate::Nomen;

impl Nomen {
    /// Prune old/unused memories and consolidated raw messages.
    pub async fn prune(&self, days: u64, dry_run: bool) -> Result<db::PruneReport> {
        db::prune_memories(&self.db, days, dry_run).await
    }

    /// List entity memories (type=entity:*), optionally filtered by type.
    pub async fn entity_memories(
        &self,
        type_filter: Option<&str>,
    ) -> Result<Vec<db::MemoryRecord>> {
        let records: Vec<db::MemoryRecord> = if let Some(exact) = type_filter {
            self.db
                .query("SELECT * FROM memory WHERE type = $filter ORDER BY topic ASC")
                .bind(("filter", exact.to_string()))
        } else {
            self.db
                .query("SELECT * FROM memory WHERE type IS NOT NONE AND string::starts_with(type, 'entity:') ORDER BY topic ASC")
        }
            .await?
            .check()?
            .take(0)?;
        Ok(records)
    }

    /// List references edges for an entity memory (by d_tag).
    pub async fn entity_relationships(
        &self,
        d_tag: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        use serde_json::json;
        let results: Vec<serde_json::Value> = if let Some(dt) = d_tag {
            // Get edges from/to this memory
            let neighbors = db::get_graph_neighbors_simple(&self.db, dt).await?;
            neighbors
                .iter()
                .map(|n| {
                    json!({
                        "relation": n.edge_type,
                        "topic": n.topic,
                        "d_tag": n.d_tag,
                        "content": n.content,
                    })
                })
                .collect()
        } else {
            vec![]
        };
        Ok(results)
    }

    /// Create a new group.
    pub async fn group_create(
        &self,
        id: &str,
        name: &str,
        members: &[String],
        nostr_group: Option<&str>,
        relay: Option<&str>,
    ) -> Result<()> {
        let parent = nomen_core::groups::derive_parent(id);
        crate::groups::create_group(&self.db, id, name, members, nostr_group, relay).await?;

        // Publish to relay
        if let Some(ref relay_mgr) = self.relay {
            if let Err(e) = relay_mgr
                .publish_group(id, name, members, relay, parent.as_deref())
                .await
            {
                tracing::warn!("Failed to publish group to relay: {e}");
            }
        }

        Ok(())
    }

    /// List all groups.
    pub async fn group_list(&self) -> Result<Vec<crate::groups::Group>> {
        crate::groups::list_groups(&self.db).await
    }

    /// Get members of a group.
    pub async fn group_members(&self, group_id: &str) -> Result<Vec<String>> {
        crate::groups::get_members(&self.db, group_id).await
    }

    /// Add a member to a group.
    pub async fn group_add_member(&self, group_id: &str, npub: &str) -> Result<()> {
        crate::groups::add_member(&self.db, group_id, npub).await
    }

    /// Remove a member from a group.
    pub async fn group_remove_member(&self, group_id: &str, npub: &str) -> Result<()> {
        crate::groups::remove_member(&self.db, group_id, npub).await
    }
}
