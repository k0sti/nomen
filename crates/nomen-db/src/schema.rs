/// Base schema (without HNSW index — that's applied dynamically based on config).
///
/// Also exported as `SCHEMA` for integration tests.
pub const SCHEMA: &str = SCHEMA_BASE;
pub(crate) const SCHEMA_BASE: &str = r#"
DEFINE TABLE IF NOT EXISTS memory SCHEMAFULL;
-- Remove stale fields from previous schema versions
REMOVE FIELD IF EXISTS search_text ON memory;
REMOVE FIELD IF EXISTS detail ON memory;
REMOVE FIELD IF EXISTS pinned ON memory;
REMOVE FIELD IF EXISTS visibility ON memory;
DEFINE FIELD IF NOT EXISTS content    ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS type       ON memory TYPE option<string>;
REMOVE FIELD IF EXISTS summary ON memory;
DEFINE FIELD IF NOT EXISTS embedding  ON memory TYPE option<array<float>>;
DEFINE FIELD IF NOT EXISTS tier       ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS scope      ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS topic      ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS source     ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS model      ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS version    ON memory TYPE int DEFAULT 1;
DEFINE FIELD IF NOT EXISTS nostr_id   ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS d_tag      ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON memory TYPE string;
REMOVE FIELD IF EXISTS ephemeral ON memory;
REMOVE FIELD IF EXISTS consolidated_from ON memory;
REMOVE FIELD IF EXISTS consolidated_at ON memory;
DEFINE FIELD IF NOT EXISTS last_accessed     ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS access_count      ON memory TYPE int DEFAULT 0;
DEFINE FIELD IF NOT EXISTS importance        ON memory TYPE option<int>;
-- Note: created_at/updated_at remain TYPE string (not datetime) because SurrealDB
-- datetime serialization requires special handling in Rust serde. RFC3339 strings
-- still support lexicographic ordering which is sufficient for our queries.

DEFINE ANALYZER IF NOT EXISTS memory_analyzer TOKENIZERS class FILTERS ascii, lowercase, snowball(english);
DEFINE INDEX IF NOT EXISTS memory_fulltext ON memory FIELDS content FULLTEXT ANALYZER memory_analyzer BM25;
DEFINE INDEX IF NOT EXISTS memory_d_tag  ON memory FIELDS d_tag UNIQUE;
DEFINE INDEX IF NOT EXISTS memory_tier   ON memory FIELDS tier;
DEFINE INDEX IF NOT EXISTS memory_scope  ON memory FIELDS scope;
DEFINE INDEX IF NOT EXISTS memory_topic  ON memory FIELDS topic;

DEFINE TABLE IF NOT EXISTS nomen_group SCHEMALESS;
DEFINE FIELD IF NOT EXISTS name       ON nomen_group TYPE string;
DEFINE FIELD IF NOT EXISTS parent     ON nomen_group TYPE option<string>;
DEFINE FIELD IF NOT EXISTS members    ON nomen_group TYPE option<array>;
DEFINE FIELD IF NOT EXISTS relay      ON nomen_group TYPE option<string>;
DEFINE FIELD IF NOT EXISTS nostr_group ON nomen_group TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON nomen_group TYPE string;
DEFINE INDEX IF NOT EXISTS group_id   ON nomen_group FIELDS id UNIQUE;

-- Migration: drop old entity/mentions/related_to tables (entities are now memories with type=entity:*)
REMOVE TABLE IF EXISTS entity;
REMOVE TABLE IF EXISTS mentions;
REMOVE TABLE IF EXISTS related_to;

DEFINE TABLE IF NOT EXISTS consolidated_from SCHEMALESS;
DEFINE TABLE IF NOT EXISTS references SCHEMALESS;

DEFINE TABLE IF NOT EXISTS meta SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS key        ON meta TYPE string;
DEFINE FIELD IF NOT EXISTS value      ON meta TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON meta TYPE string;
DEFINE INDEX IF NOT EXISTS meta_key   ON meta FIELDS key UNIQUE;

-- Key-value meta store (used by consolidation pipeline, migrations, etc.)
DEFINE TABLE IF NOT EXISTS kv_meta SCHEMALESS;
DEFINE INDEX IF NOT EXISTS kv_meta_key ON kv_meta FIELDS key UNIQUE;

DEFINE INDEX IF NOT EXISTS session_sid   ON session FIELDS session_id UNIQUE;

-- Consolidation sessions (two-phase agent mode)
DEFINE TABLE IF NOT EXISTS consolidation_session SCHEMALESS;
DEFINE INDEX IF NOT EXISTS cons_session_sid ON consolidation_session FIELDS session_id UNIQUE;

-- Collected messages (kind 30100 events)
DEFINE TABLE IF NOT EXISTS message SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS event_json ON message TYPE string;
DEFINE FIELD IF NOT EXISTS d_tag ON message TYPE string;
DEFINE FIELD IF NOT EXISTS kind ON message TYPE int;
DEFINE FIELD IF NOT EXISTS pubkey ON message TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON message TYPE int;
DEFINE FIELD IF NOT EXISTS content ON message TYPE string;
DEFINE FIELD IF NOT EXISTS platform ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS community_id ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS community_type ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS chat_id ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS sender_id ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS thread_id ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS chat_type ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS chat_name ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS thread_type ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS message_id ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS consolidated ON message TYPE bool DEFAULT false;
DEFINE INDEX IF NOT EXISTS msg_d_tag ON message FIELDS d_tag UNIQUE;
DEFINE INDEX IF NOT EXISTS msg_platform ON message FIELDS platform;
DEFINE INDEX IF NOT EXISTS msg_community_id ON message FIELDS community_id;
DEFINE INDEX IF NOT EXISTS msg_chat_id ON message FIELDS chat_id;
DEFINE INDEX IF NOT EXISTS msg_sender_id ON message FIELDS sender_id;
DEFINE INDEX IF NOT EXISTS msg_thread_id ON message FIELDS thread_id;
DEFINE INDEX IF NOT EXISTS msg_chat_type ON message FIELDS chat_type;
DEFINE INDEX IF NOT EXISTS msg_message_id ON message FIELDS message_id;
DEFINE INDEX IF NOT EXISTS msg_created_at ON message FIELDS created_at;
DEFINE INDEX IF NOT EXISTS msg_platform_chat ON message FIELDS platform, chat_id;
DEFINE INDEX IF NOT EXISTS msg_platform_community_chat ON message FIELDS platform, community_id, chat_id;
DEFINE INDEX IF NOT EXISTS msg_chat_thread ON message FIELDS chat_id, thread_id;
DEFINE ANALYZER IF NOT EXISTS message_analyzer TOKENIZERS class FILTERS ascii, lowercase, snowball(english);
DEFINE INDEX IF NOT EXISTS msg_fulltext ON message FIELDS content FULLTEXT ANALYZER message_analyzer BM25;
"#;
