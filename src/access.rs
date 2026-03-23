pub use nomen_core::access::*;

// The `impl AccessCheckable for MemoryRecord` is in nomen-db (where both types are visible).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GroupConfig;
    use crate::db::MemoryRecord;
    use crate::groups::GroupStore;

    fn make_memory(tier: &str, scope: &str, source: &str) -> MemoryRecord {
        MemoryRecord {
            content: String::new(),
            summary: None,
            embedding: None,
            tier: tier.to_string(),
            scope: scope.to_string(),
            topic: "test".to_string(),
            confidence: None,
            source: source.to_string(),
            model: None,
            version: 1,
            nostr_id: None,
            d_tag: None,
            created_at: String::new(),
            updated_at: String::new(),
            ephemeral: false,
        }
    }

    #[test]
    fn test_can_access_public() {
        let store = GroupStore::from_config(&[]);
        let mem = make_memory("public", "", "npub1author");
        assert!(can_access(&mem, "npub1anyone", &store));
    }

    #[test]
    fn test_can_access_personal() {
        let store = GroupStore::from_config(&[]);
        let mem = make_memory("personal", "npub1author", "npub1author");
        assert!(can_access(&mem, "npub1author", &store));
        assert!(!can_access(&mem, "npub1other", &store));
    }

    #[test]
    fn test_can_access_private() {
        let store = GroupStore::from_config(&[]);
        let mem = make_memory("private", "npub1author", "npub1author");
        assert!(can_access(&mem, "npub1author", &store));
        assert!(!can_access(&mem, "npub1other", &store));
    }

    #[test]
    fn test_can_access_internal_legacy() {
        let store = GroupStore::from_config(&[]);
        let mem = make_memory("internal", "npub1author", "npub1author");
        assert!(can_access(&mem, "npub1author", &store));
        assert!(!can_access(&mem, "npub1other", &store));
    }

    #[test]
    fn test_can_access_group() {
        let store = GroupStore::from_config(&[GroupConfig {
            id: "atlantislabs".to_string(),
            name: "Atlantis Labs".to_string(),
            members: vec!["npub1abc".to_string()],
            nostr_group: None,
            relay: None,
        }]);

        let mem = make_memory("group", "atlantislabs", "npub1abc");
        assert!(can_access(&mem, "npub1abc", &store));
        assert!(!can_access(&mem, "npub1xyz", &store));
    }

    #[test]
    fn test_build_scope_filter() {
        let store = GroupStore::from_config(&[
            GroupConfig {
                id: "atlantislabs".to_string(),
                name: "Atlantis Labs".to_string(),
                members: vec!["npub1abc".to_string()],
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

        let scopes = build_scope_filter("npub1abc", &store);
        assert!(scopes.contains(&String::new())); // public
        assert!(scopes.contains(&"atlantislabs".to_string()));
        assert!(scopes.contains(&"atlantislabs.engineering".to_string()));
        assert!(scopes.contains(&"npub1abc".to_string())); // private
    }
}
