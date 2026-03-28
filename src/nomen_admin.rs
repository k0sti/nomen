//! impl Nomen — admin operations: prune, group management, entities.

use anyhow::Result;

use crate::db;
use crate::entities::{EntityKind, EntityRecord, RelationshipRecord};
use crate::Nomen;

impl Nomen {
    /// Prune old/unused memories and consolidated raw messages.
    pub async fn prune(&self, days: u64, dry_run: bool) -> Result<db::PruneReport> {
        db::prune_memories(&self.db, days, dry_run).await
    }

    /// List extracted entities, optionally filtered by kind.
    pub async fn entities(&self, kind: Option<&str>) -> Result<Vec<EntityRecord>> {
        let kind = kind.and_then(EntityKind::from_str);
        db::list_entities(&self.db, kind.as_ref()).await
    }

    /// List entity relationships, optionally filtered by entity name.
    pub async fn entity_relationships(
        &self,
        entity_name: Option<&str>,
    ) -> Result<Vec<RelationshipRecord>> {
        db::list_entity_relationships(&self.db, entity_name).await
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
