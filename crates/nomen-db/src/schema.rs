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
DEFINE FIELD IF NOT EXISTS summary    ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS embedding  ON memory TYPE option<array<float>>;
DEFINE FIELD IF NOT EXISTS tier       ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS scope      ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS topic      ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS confidence ON memory TYPE option<float>;
DEFINE FIELD IF NOT EXISTS source     ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS model      ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS version    ON memory TYPE int DEFAULT 1;
DEFINE FIELD IF NOT EXISTS nostr_id   ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS d_tag      ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON memory TYPE string;
DEFINE FIELD IF NOT EXISTS ephemeral  ON memory TYPE bool DEFAULT false;
DEFINE FIELD IF NOT EXISTS consolidated_from ON memory TYPE option<string>;
DEFINE FIELD IF NOT EXISTS consolidated_at   ON memory TYPE option<string>;
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

DEFINE TABLE IF NOT EXISTS raw_message SCHEMALESS;
DEFINE FIELD IF NOT EXISTS source       ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS source_id    ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS sender       ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS channel      ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS content      ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS metadata     ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at   ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS consolidated ON raw_message TYPE bool DEFAULT false;
DEFINE INDEX IF NOT EXISTS raw_msg_time    ON raw_message FIELDS created_at;
DEFINE INDEX IF NOT EXISTS raw_msg_channel ON raw_message FIELDS channel;

DEFINE TABLE IF NOT EXISTS entity SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS name       ON entity TYPE string;
DEFINE FIELD IF NOT EXISTS kind       ON entity TYPE string;
DEFINE FIELD IF NOT EXISTS attributes ON entity TYPE option<object>;
DEFINE FIELD IF NOT EXISTS created_at ON entity TYPE string;
DEFINE INDEX IF NOT EXISTS entity_name ON entity FIELDS name UNIQUE;

DEFINE TABLE IF NOT EXISTS mentions SCHEMALESS;
DEFINE TABLE IF NOT EXISTS consolidated_from SCHEMALESS;
DEFINE TABLE IF NOT EXISTS references SCHEMALESS;
DEFINE TABLE IF NOT EXISTS related_to SCHEMALESS;

DEFINE INDEX IF NOT EXISTS raw_msg_source ON raw_message FIELDS source, source_id UNIQUE;
DEFINE INDEX IF NOT EXISTS raw_msg_sender ON raw_message FIELDS sender;

DEFINE TABLE IF NOT EXISTS meta SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS key        ON meta TYPE string;
DEFINE FIELD IF NOT EXISTS value      ON meta TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON meta TYPE string;
DEFINE INDEX IF NOT EXISTS meta_key   ON meta FIELDS key UNIQUE;

DEFINE TABLE IF NOT EXISTS session SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS session_id    ON session TYPE string;
DEFINE FIELD IF NOT EXISTS tier          ON session TYPE string;
DEFINE FIELD IF NOT EXISTS scope         ON session TYPE string;
DEFINE FIELD IF NOT EXISTS channel       ON session TYPE string;
DEFINE FIELD IF NOT EXISTS group_id      ON session TYPE string;
DEFINE FIELD IF NOT EXISTS participants  ON session TYPE array;
DEFINE FIELD IF NOT EXISTS participants.* ON session TYPE string;
DEFINE FIELD IF NOT EXISTS created_at    ON session TYPE string;
DEFINE FIELD IF NOT EXISTS last_active   ON session TYPE string;
DEFINE INDEX IF NOT EXISTS session_sid   ON session FIELDS session_id UNIQUE;

-- Consolidation sessions (two-phase agent mode)
DEFINE TABLE IF NOT EXISTS consolidation_session SCHEMALESS;
DEFINE INDEX IF NOT EXISTS cons_session_sid ON consolidation_session FIELDS session_id UNIQUE;
"#;
