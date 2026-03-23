pub use nomen_core::session::*;

use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

/// SurrealDB session record.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
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
