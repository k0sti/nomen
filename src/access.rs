use crate::db::MemoryRecord;
use crate::groups::GroupStore;

/// Check if a requester can access a given memory based on tier and scope.
///
/// Rules:
/// - public: everyone can access
/// - group: requester must be a member of the memory's scope group
/// - personal: only the author (source) can access (user-auditable knowledge)
/// - internal: only the author (source) can access (agent-only reasoning)
/// - private: legacy alias for personal, treated identically
pub fn can_access(memory: &MemoryRecord, requester_npub: &str, group_store: &GroupStore) -> bool {
    match memory.tier.as_str() {
        "public" => true,
        "group" => group_store.is_member(&memory.scope, requester_npub),
        "personal" | "internal" | "private" => memory.source == requester_npub,
        _ => false,
    }
}

/// Build the list of scopes a requester is allowed to query.
/// Returns scope strings that can be used in a SurrealDB WHERE clause.
///
/// Always includes "" (public scope). Adds all group scopes the npub belongs to.
/// For private, adds the npub itself.
pub fn build_scope_filter(requester_npub: &str, group_store: &GroupStore) -> Vec<String> {
    let mut scopes = vec![String::new()]; // "" = public

    // Add all group scopes this npub is a member of
    let group_scopes = group_store.expand_scopes(requester_npub);
    scopes.extend(group_scopes);

    // Add private scope (the npub itself)
    scopes.push(requester_npub.to_string());

    scopes
}

/// Build tier+scope filter conditions for SurrealDB queries.
/// Returns (allowed_tiers, allowed_scopes) for use in WHERE clauses.
pub fn build_query_filters(
    requester_npub: &str,
    group_store: &GroupStore,
) -> (Vec<String>, Vec<String>) {
    let tiers = vec![
        "public".to_string(),
        "group".to_string(),
        "personal".to_string(),
        "internal".to_string(),
        "private".to_string(), // legacy alias for personal
    ];
    let scopes = build_scope_filter(requester_npub, group_store);
    (tiers, scopes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GroupConfig;

    fn make_memory(tier: &str, scope: &str, source: &str) -> MemoryRecord {
        MemoryRecord {
            search_text: String::new(),
            detail: None,
            embedding: None,
            visibility: tier.to_string(),
            scope: scope.to_string(),
            topic: "test".to_string(),
            source: source.to_string(),
            model: None,
            version: 1,
            nostr_id: None,
            d_tag: None,
            created_at: String::new(),
            updated_at: String::new(),
            ephemeral: false,
            consolidated_from: None,
            consolidated_at: None,
            last_accessed: None,
            access_count: 0,
            importance: None,
            embedded: false,
            pinned: false,
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
    fn test_can_access_internal() {
        let store = GroupStore::from_config(&[]);
        let mem = make_memory("internal", "npub1author", "npub1author");
        assert!(can_access(&mem, "npub1author", &store));
        assert!(!can_access(&mem, "npub1other", &store));
    }

    #[test]
    fn test_can_access_private_legacy() {
        let store = GroupStore::from_config(&[]);
        let mem = make_memory("private", "npub1author", "npub1author");
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
