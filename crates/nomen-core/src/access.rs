use crate::groups::GroupStore;

/// Trait for memory records that can be access-checked.
/// This avoids a dependency on the full SurrealDB MemoryRecord type.
pub trait AccessCheckable {
    fn tier(&self) -> &str;
    fn scope(&self) -> &str;
    fn source(&self) -> &str;
}

/// Check if a requester can access a given memory based on tier and scope.
///
/// Rules:
/// - public: everyone can access
/// - group: requester must be a member of the memory's scope group
/// - personal: only the author (source) can access (user-auditable knowledge)
/// - private: only the author (source) can access (agent-only reasoning)
/// - internal: legacy alias for private, treated identically
pub fn can_access(memory: &dyn AccessCheckable, requester_npub: &str, group_store: &GroupStore) -> bool {
    match memory.tier() {
        "public" => true,
        "group" => group_store.is_member(memory.scope(), requester_npub),
        "personal" | "private" | "internal" => memory.source() == requester_npub,
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
        "private".to_string(),
        "internal".to_string(), // legacy alias for private
    ];
    let scopes = build_scope_filter(requester_npub, group_store);
    (tiers, scopes)
}
