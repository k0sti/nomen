# Nomen Implementation Plan

**Version:** v1.0
**Date:** 2026-03-05
**Status:** Planning

## Current State

~1k lines of Rust. Working: CLI scaffold, SurrealDB init with schema, relay connect + NIP-42 auth, list/store/delete/search (full-text only) commands, config management. Missing: embeddings, entities, graph, consolidation, message ingestion, group hierarchy, all integration interfaces.

---

## Phase 1: Core Memory System

**Goal:** Production-quality local memory with hybrid search, entity extraction, and group hierarchy.

### 1.1 Group Hierarchy & Scope Model

The `scope` field uses hierarchical dot-separated identifiers. This enables natural subgroup nesting and ancestor queries.

**Scope format:**
```
""                          → public (no scope)
"atlantislabs"              → top-level group
"atlantislabs.engineering"  → subgroup of atlantislabs
"atlantislabs.engineering.infra" → sub-subgroup
"npub1abc..."               → private (single agent)
```

**Rules:**
- Dot (`.`) is the hierarchy separator
- A query for `atlantislabs` matches `atlantislabs`, `atlantislabs.engineering`, `atlantislabs.engineering.infra`
- A query for `atlantislabs.engineering` matches itself and children, but NOT `atlantislabs` root
- Private scopes (npub) are flat — no hierarchy

**Group configuration table:**
```surql
DEFINE TABLE nomen_group SCHEMAFULL;
DEFINE FIELD id         ON nomen_group TYPE string;       -- "atlantislabs.engineering"
DEFINE FIELD name       ON nomen_group TYPE string;       -- "Engineering"
DEFINE FIELD parent     ON nomen_group TYPE option<string>; -- "atlantislabs"
DEFINE FIELD members    ON nomen_group TYPE array<string>; -- [npub1..., npub2...]
DEFINE FIELD relay      ON nomen_group TYPE option<string>; -- relay URL for this group
DEFINE FIELD nostr_group ON nomen_group TYPE option<string>; -- NIP-29 group id mapping
DEFINE FIELD created_at ON nomen_group TYPE datetime;

DEFINE INDEX group_id     ON nomen_group FIELDS id UNIQUE;
DEFINE INDEX group_parent ON nomen_group FIELDS parent;
```

**Subgroup derivation:**
- If a group has explicit member lists, subgroups are visible: members of `atlantislabs` can see that `atlantislabs.engineering` exists (but can't read its memories unless they're also members)
- Scope filtering query uses prefix matching:
```surql
-- All memories visible to a member of atlantislabs.engineering
SELECT * FROM memory WHERE
  scope = "" OR                                    -- public
  scope = "atlantislabs" OR                        -- parent group
  scope = "atlantislabs.engineering" OR            -- own group
  string::starts_with(scope, "atlantislabs.engineering.") -- child groups they're member of
```

**CLI:**
```
nomen group create atlantislabs --name "Atlantis Labs"
nomen group create atlantislabs.engineering --name "Engineering" --members npub1abc,npub1def
nomen group list
nomen group members atlantislabs.engineering
nomen group add-member atlantislabs.engineering npub1xyz
```

**Config file alternative** (`~/.config/nomen/config.toml`):
```toml
[[groups]]
id = "atlantislabs"
name = "Atlantis Labs"
members = ["npub1abc...", "npub1def...", "npub1xyz..."]

[[groups]]
id = "atlantislabs.engineering"
name = "Engineering"
members = ["npub1abc...", "npub1def..."]
nostr_group = "techteam"  # maps to NIP-29 group on relay
```

**D-tag mapping:**
```
snow:memory:group:atlantislabs.engineering:topic-name
```

**Nostr event scoping:** For NIP-29 relays, the `h` tag maps to the `nostr_group` field:
```json
["h", "techteam"]  →  scope = "atlantislabs.engineering"
```

### 1.2 Tier Enforcement

Tiers and scopes are orthogonal:

| Tier | Encryption | Relay visibility | Local access |
|------|-----------|-----------------|--------------|
| `public` | None | All subscribers | Everyone |
| `group` | None (relay-auth gated) | Group members via `h` tag | Scope-matched queries |
| `private` | NIP-44 (self-encrypt) | Only author can decrypt | Author only |

**Access check function:**
```rust
fn can_access(memory: &Memory, requester: &PublicKey, groups: &GroupStore) -> bool {
    match memory.tier.as_str() {
        "public" => true,
        "group" => groups.is_member(&memory.scope, requester),
        "private" => memory.source == requester.to_hex(),
        _ => false,
    }
}
```

Group membership check walks up the hierarchy: if you're a member of `atlantislabs`, you can access `atlantislabs`-scoped memories. You can only access `atlantislabs.engineering` memories if you're explicitly a member of that subgroup.

### 1.3 Embedding Pipeline

```rust
// src/embed.rs
pub trait Embedder: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn dimensions(&self) -> usize;
}

pub struct OpenAIEmbedder { client: reqwest::Client, api_key: String, model: String }
pub struct LocalEmbedder { endpoint: String }  // vLLM / Ollama compatible
```

**Config:**
```toml
[embedding]
provider = "openai"  # or "local"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"  # read from env
dimensions = 1536
batch_size = 100

# Alternative:
# provider = "local"
# endpoint = "http://localhost:8000/v1/embeddings"
```

**When embeddings are generated:**
- `nomen store` → embed immediately
- `nomen sync` → embed any memories without embeddings
- `nomen consolidate` → embed consolidated memories
- NOT on raw message ingest (too noisy, too expensive)

### 1.4 Hybrid Search

Upgrade current full-text-only search to hybrid (vector + BM25):

```rust
// src/search.rs
pub struct SearchOptions {
    pub query: String,
    pub tier: Option<String>,
    pub scope: Option<String>,       // includes hierarchy expansion
    pub limit: usize,
    pub vector_weight: f32,          // default 0.7
    pub text_weight: f32,            // default 0.3
    pub min_confidence: Option<f64>,
    pub topic_filter: Option<String>,
}

pub struct SearchResult {
    pub memory: MemoryRecord,
    pub score: f64,
    pub match_type: MatchType,  // Vector | Text | Hybrid
}
```

**SurrealQL hybrid query:**
```surql
LET $vec = <embedding of query>;
LET $scopes = ["", "atlantislabs", "atlantislabs.engineering"];

SELECT *,
  vector::similarity::cosine(embedding, $vec) AS vec_score,
  search::score(1) AS text_score,
  (vector::similarity::cosine(embedding, $vec) * 0.7 + search::score(1) * 0.3) AS combined
FROM memory
WHERE content @1@ $query
  AND tier IN $allowed_tiers
  AND scope IN $scopes
ORDER BY combined DESC
LIMIT $limit;
```

### 1.5 Entity Extraction & Graph

Start with heuristic extraction, upgrade to LLM later:

```rust
// src/entities.rs
pub struct ExtractedEntity {
    pub name: String,
    pub kind: EntityKind,  // Person | Project | Concept | Place | Organization
    pub relevance: f64,
}

// Phase 1: regex/pattern-based
pub fn extract_entities_heuristic(text: &str, known_entities: &[String]) -> Vec<ExtractedEntity>;

// Phase 2: LLM-powered (configurable)
pub async fn extract_entities_llm(text: &str, provider: &dyn LlmProvider) -> Vec<ExtractedEntity>;
```

Entity extraction runs on:
- Memory store (always)
- Consolidation output (always)
- NOT raw messages

### 1.6 Schema Updates

Add to existing schema:

```surql
-- Embedding field on memory (add to existing table)
DEFINE FIELD IF NOT EXISTS embedding ON memory TYPE option<array<float>>;
DEFINE INDEX IF NOT EXISTS memory_embedding ON memory FIELDS embedding
  HNSW DIMENSION 1536 DIST COSINE EFC 150 M 12;

-- Scope index for hierarchy queries
DEFINE FIELD IF NOT EXISTS scope ON memory TYPE string DEFAULT "";
DEFINE INDEX IF NOT EXISTS memory_scope ON memory FIELDS scope;

-- Entity table
DEFINE TABLE IF NOT EXISTS entity SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS name       ON entity TYPE string;
DEFINE FIELD IF NOT EXISTS kind       ON entity TYPE string;
DEFINE FIELD IF NOT EXISTS attributes ON entity TYPE option<object>;
DEFINE FIELD IF NOT EXISTS created_at ON entity TYPE string;
DEFINE INDEX IF NOT EXISTS entity_name ON entity FIELDS name UNIQUE;

-- Graph edges
DEFINE TABLE IF NOT EXISTS mentions SCHEMALESS;
DEFINE TABLE IF NOT EXISTS references SCHEMALESS;
DEFINE TABLE IF NOT EXISTS consolidated_from SCHEMALESS;
DEFINE TABLE IF NOT EXISTS related_to SCHEMALESS;

-- Group config
DEFINE TABLE IF NOT EXISTS nomen_group SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS id         ON nomen_group TYPE string;
DEFINE FIELD IF NOT EXISTS name       ON nomen_group TYPE string;
DEFINE FIELD IF NOT EXISTS parent     ON nomen_group TYPE option<string>;
DEFINE FIELD IF NOT EXISTS members    ON nomen_group TYPE array;
DEFINE FIELD IF NOT EXISTS relay      ON nomen_group TYPE option<string>;
DEFINE FIELD IF NOT EXISTS nostr_group ON nomen_group TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON nomen_group TYPE string;
DEFINE INDEX IF NOT EXISTS group_id   ON nomen_group FIELDS id UNIQUE;

-- Raw messages
DEFINE TABLE IF NOT EXISTS raw_message SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS source      ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS source_id   ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS sender      ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS channel     ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS content     ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS metadata    ON raw_message TYPE option<object>;
DEFINE FIELD IF NOT EXISTS created_at  ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS consolidated ON raw_message TYPE bool DEFAULT false;
DEFINE INDEX IF NOT EXISTS raw_msg_time ON raw_message FIELDS created_at;
DEFINE INDEX IF NOT EXISTS raw_msg_channel ON raw_message FIELDS channel;
```

### 1.7 Deliverables

| Item | File(s) | Estimate |
|------|---------|----------|
| Group hierarchy model + config | `src/groups.rs`, `config.toml` | 1 day |
| Tier enforcement + access checks | `src/access.rs` | 0.5 day |
| Embedding trait + OpenAI impl | `src/embed.rs` | 1 day |
| Hybrid search (vector + BM25) | `src/search.rs`, `src/db.rs` | 1 day |
| Entity extraction (heuristic) | `src/entities.rs` | 1 day |
| Graph edges (mentions, references) | `src/db.rs` | 0.5 day |
| Raw message table + ingest | `src/ingest.rs` | 0.5 day |
| Schema migration | `src/db.rs` | 0.5 day |
| **Total Phase 1** | | **~6 days** |

---

## Phase 2: Message Ingestion & Consolidation

**Goal:** Ingest raw messages from all sources, consolidate into searchable memories.

### 2.1 Message Ingestion

```rust
// src/ingest.rs
pub struct RawMessage {
    pub source: String,      // "telegram" | "nostr" | "webhook"
    pub source_id: Option<String>,
    pub sender: String,      // npub, telegram user id, etc.
    pub channel: Option<String>,
    pub content: String,
    pub metadata: Option<serde_json::Value>,
}

pub async fn ingest_message(db: &Surreal<Db>, msg: RawMessage) -> Result<()>;
pub async fn get_messages(db: &Surreal<Db>, opts: MessageQuery) -> Result<Vec<RawMessage>>;
```

CLI:
```
nomen ingest --source telegram --sender 60996061 --channel techteam "message text"
nomen messages --source telegram --channel techteam --since 2h --limit 50
```

### 2.2 Consolidation Pipeline

```rust
// src/consolidate.rs
pub struct ConsolidationConfig {
    pub batch_size: usize,           // messages per batch (default 50)
    pub time_window: Duration,       // group messages within this window
    pub min_messages: usize,         // minimum to trigger consolidation
    pub llm_provider: Box<dyn LlmProvider>,
}

pub async fn consolidate(db: &Surreal<Db>, embedder: &dyn Embedder, config: &ConsolidationConfig) -> Result<ConsolidationReport>;
```

**Consolidation steps:**
1. Query `raw_message WHERE consolidated = false` grouped by channel + time window
2. Send batch to LLM: "Extract significant facts, decisions, context from these messages"
3. LLM returns structured output: `[{summary, detail, topic, entities, confidence}]`
4. For each extracted memory:
   - Store in `memory` table with proper scope/tier
   - Generate embedding
   - Extract and link entities
   - Create `consolidated_from` edges to source messages
5. Mark raw messages as `consolidated = true`
6. Retention: delete consolidated raw messages after N days (configurable)

**LLM provider trait:**
```rust
pub trait LlmProvider: Send + Sync {
    async fn consolidate(&self, messages: &[RawMessage]) -> Result<Vec<ExtractedMemory>>;
    async fn extract_entities(&self, text: &str) -> Result<Vec<ExtractedEntity>>;
}

pub struct OpenRouterProvider { api_key: String, model: String }
```

### 2.3 Deliverables

| Item | File(s) | Estimate |
|------|---------|----------|
| Raw message ingest + query | `src/ingest.rs` | 1 day |
| LLM provider trait + OpenRouter impl | `src/llm.rs` | 1 day |
| Consolidation pipeline | `src/consolidate.rs` | 2 days |
| Retention/pruning | `src/ingest.rs` | 0.5 day |
| CLI commands (consolidate, messages) | `src/main.rs` | 0.5 day |
| **Total Phase 2** | | **~5 days** |

---

## Phase 3: Nostr Relay Sync

**Goal:** Bidirectional sync with Nostr relays, proper NIP-42 auth, NIP-44 encryption.

### 3.1 Improved Relay Sync

Current relay code is minimal. Needs:

```rust
// src/relay.rs (rewrite)
pub struct RelayManager {
    client: Client,
    keys: Keys,
    config: RelayConfig,
}

impl RelayManager {
    /// Connect with NIP-42 auth, verify connection
    pub async fn connect(&mut self) -> Result<()>;
    
    /// Fetch all memory events, return per-relay status
    pub async fn fetch_memories(&self, pubkeys: &[PublicKey]) -> Result<Vec<Event>>;
    
    /// Publish and verify acceptance (inspect Output, don't discard)
    pub async fn publish(&self, event: EventBuilder) -> Result<PublishResult>;
    
    /// Subscribe to live updates
    pub async fn subscribe(&self, pubkeys: &[PublicKey]) -> Result<()>;
    
    /// Encrypt content for private tier (NIP-44)
    pub fn encrypt_private(&self, content: &str) -> Result<String>;
    pub fn decrypt_private(&self, encrypted: &str) -> Result<String>;
}

pub struct PublishResult {
    pub event_id: EventId,
    pub accepted: Vec<String>,    // relay URLs that accepted
    pub rejected: Vec<(String, String)>,  // (relay_url, reason)
}
```

### 3.2 D-Tag Mapping for Groups

```
snow:memory:group:{scope}:{topic}
```

Where `{scope}` uses dots: `snow:memory:group:atlantislabs.engineering:standup-notes`

The relay `h` tag uses the NIP-29 group id (mapped via `nomen_group.nostr_group`):
```json
["h", "techteam"]
```

### 3.3 Deliverables

| Item | Estimate |
|------|----------|
| RelayManager rewrite with proper auth | 1 day |
| Publish with Output inspection | 0.5 day |
| NIP-44 encrypt/decrypt for private tier | 0.5 day |
| Live subscription + incremental sync | 1 day |
| D-tag ↔ scope mapping | 0.5 day |
| **Total Phase 3** | **~3.5 days** |

---

## Phase 4: MCP Server

**Goal:** Expose Nomen as an MCP server for any MCP-compatible agent.

### 4.1 Server Implementation

```rust
// src/mcp.rs
pub struct NomenMcpServer {
    db: Surreal<Db>,
    embedder: Box<dyn Embedder>,
    relay: Option<RelayManager>,
}
```

**Transport:**
- `nomen serve --stdio` → stdio transport (for local agents, OpenClaw plugin)
- `nomen serve --http :3848` → HTTP+SSE transport (for remote agents)

**Tools:**

| Tool | Parameters | Description |
|------|-----------|-------------|
| `nomen_search` | `query`, `tier?`, `scope?`, `limit?` | Hybrid semantic + full-text search |
| `nomen_store` | `topic`, `summary`, `detail?`, `tier?`, `scope?`, `confidence?` | Store a new memory |
| `nomen_ingest` | `source`, `sender`, `channel?`, `content`, `metadata?` | Ingest a raw message |
| `nomen_messages` | `source?`, `channel?`, `sender?`, `since?`, `limit?` | Query raw messages |
| `nomen_entities` | `kind?`, `query?` | List/search entities |
| `nomen_consolidate` | `channel?`, `since?` | Trigger consolidation |
| `nomen_groups` | `action` (`list`\|`members`\|`create`) | Manage groups |
| `nomen_delete` | `topic?`, `id?` | Delete a memory |

**MCP crate:** Use `mcp-server` or `rmcp` crate (check ecosystem maturity).

### 4.2 OpenClaw Plugin

Thin TS wrapper in `~/work/openclaw-nomen/`:

```
openclaw-nomen/
├── openclaw.plugin.json    # plugin manifest
├── package.json
├── src/
│   ├── plugin.ts           # main plugin: message hook + MCP bridge
│   └── config.ts
└── tsconfig.json
```

**Plugin responsibilities:**
1. On load: spawn `nomen serve --stdio` as child process
2. Hook all inbound messages → call `nomen_ingest` tool
3. Register Nomen MCP tools in OpenClaw's tool registry
4. On periodic timer or agent request → call `nomen_consolidate`

### 4.3 Deliverables

| Item | Estimate |
|------|----------|
| MCP server core (tool dispatch, schema) | 2 days |
| stdio transport | 0.5 day |
| HTTP+SSE transport | 1 day |
| OpenClaw plugin scaffold | 1 day |
| OpenClaw message hook → ingest | 0.5 day |
| Integration testing | 1 day |
| **Total Phase 4** | **~6 days** |

---

## Phase 5: Context-VM (Nostr-Native Interface)

**Goal:** Pure Nostr request/response interface for agents that skip MCP.

### 5.1 Event Protocol

```
Request:  kind 21900 (ephemeral)
Response: kind 21901 (ephemeral)
```

Using ephemeral range (20000-29999) so relays don't persist them.

**Request event (kind 21900):**
```json
{
  "kind": 21900,
  "content": "<NIP-44 encrypted JSON>",
  "tags": [
    ["p", "<nomen-service-npub>"],
    ["t", "nomen-request"],
    ["expiration", "<unix+60>"]
  ]
}
```

**Decrypted content:**
```json
{
  "action": "search",
  "params": {
    "query": "alhovuori plans",
    "scope": "atlantislabs.engineering",
    "limit": 10
  }
}
```

**Response event (kind 21901):**
```json
{
  "kind": 21901,
  "content": "<NIP-44 encrypted JSON>",
  "tags": [
    ["p", "<requesting-agent-npub>"],
    ["e", "<request-event-id>"],
    ["t", "nomen-response"]
  ]
}
```

**Supported actions:** `search`, `store`, `ingest`, `entities`, `consolidate`, `messages`, `groups`

### 5.2 Daemon Mode

`nomen daemon` runs both MCP server and Nostr listener:

```rust
// src/daemon.rs
pub async fn run_daemon(config: &Config) -> Result<()> {
    let db = init_db().await?;
    let relay = RelayManager::connect(&config).await?;
    
    // Subscribe to kind 21900 tagged with our npub
    relay.subscribe_requests().await?;
    
    // Also optionally listen on HTTP for MCP
    if let Some(port) = config.mcp_port {
        tokio::spawn(mcp::serve_http(port, db.clone()));
    }
    
    // Event loop: process incoming requests
    loop {
        let event = relay.next_request().await?;
        let response = handle_request(&db, &event).await?;
        relay.publish_response(response).await?;
    }
}
```

### 5.3 Authorization

- Only process requests from known npubs (configured allowlist or group members)
- All request/response content NIP-44 encrypted
- Scope access enforced: requesting agent only gets memories they're authorized for
- Rate limiting per npub

### 5.4 Deliverables

| Item | Estimate |
|------|----------|
| Request/response event protocol | 1 day |
| Nostr subscription + dispatch | 1 day |
| NIP-44 encryption for all payloads | 0.5 day |
| Authorization + rate limiting | 0.5 day |
| Daemon mode (combined MCP + Nostr) | 1 day |
| **Total Phase 5** | **~4 days** |

---

## Phase 6: Snowclaw Integration

**Goal:** Replace Snowclaw's `CollectiveMemory` with Nomen library crate.

### 6.1 Library API

```rust
// nomen as a library crate
pub struct Nomen {
    db: Surreal<Db>,
    embedder: Box<dyn Embedder>,
    relay: Option<RelayManager>,
    groups: GroupStore,
}

impl Nomen {
    pub async fn open(config: &Config) -> Result<Self>;
    pub async fn search(&self, opts: SearchOptions) -> Result<Vec<SearchResult>>;
    pub async fn store(&self, memory: NewMemory) -> Result<MemoryId>;
    pub async fn ingest_message(&self, msg: RawMessage) -> Result<()>;
    pub async fn consolidate(&self, opts: ConsolidateOptions) -> Result<ConsolidationReport>;
    pub async fn get_messages(&self, opts: MessageQuery) -> Result<Vec<RawMessage>>;
    pub async fn entities(&self, opts: EntityQuery) -> Result<Vec<Entity>>;
    pub async fn delete(&self, id: &MemoryId) -> Result<()>;
}
```

### 6.2 Migration

```rust
// src/migrate.rs
/// Import from Snowclaw's memories.db (SQLite) into Nomen (SurrealDB)
pub async fn migrate_from_sqlite(sqlite_path: &Path, nomen: &Nomen) -> Result<MigrationReport>;
```

CLI: `nomen import --sqlite ~/.snowclaw/memories.db`

### 6.3 Snowclaw Changes

In `~/work/snowclaw/Cargo.toml`:
```toml
nomen = { path = "../nomen" }
```

Replace:
- `CollectiveMemory::recall()` → `nomen.search()`
- `CollectiveMemory::store()` → `nomen.store()`
- Direct relay publish for memory → `nomen.store()` (handles relay sync)
- Add `nomen.ingest_message()` calls in Telegram + Nostr channel handlers

### 6.4 Deliverables

| Item | Estimate |
|------|----------|
| Library crate API (pub interface) | 1 day |
| SQLite → SurrealDB migration | 1 day |
| Snowclaw CollectiveMemory replacement | 1.5 days |
| Channel handler message ingestion | 0.5 day |
| Testing + validation | 1 day |
| **Total Phase 6** | **~5 days** |

---

## Phase 7: UI (Optional)

**Goal:** Web interface for browsing, searching, and managing memories.

### 7.1 Options

1. **Flotilla integration** — add memory views to existing Flotilla fork
2. **Standalone SPA** — small Svelte/React app served by `nomen serve --http`
3. **TUI** — terminal UI with `ratatui` (fits CLI-first approach)

### 7.2 Features

- Memory browser with search (hybrid)
- Entity graph visualization
- Group hierarchy tree
- Consolidation status + manual trigger
- Raw message viewer
- Memory edit/delete

### 7.3 Estimate

| Item | Estimate |
|------|----------|
| TUI (ratatui, covers core features) | 3 days |
| Web UI (if desired, standalone SPA) | 5 days |

---

## Summary Timeline

| Phase | Description | Estimate | Dependencies |
|-------|-------------|----------|--------------|
| 1 | Core Memory System | ~6 days | None |
| 2 | Message Ingestion & Consolidation | ~5 days | Phase 1 |
| 3 | Nostr Relay Sync | ~3.5 days | Phase 1 |
| 4 | MCP Server + OpenClaw Plugin | ~6 days | Phase 1-2 |
| 5 | Context-VM (Nostr-Native) | ~4 days | Phase 3 |
| 6 | Snowclaw Integration | ~5 days | Phase 1-3 |
| 7 | UI (optional) | ~3-5 days | Phase 1 |
| **Total** | | **~32-34 days** | |

**Recommended order:** 1 → 2 → 3 → 4 → 6 → 5 → 7

Phase 4 (MCP) provides immediate value for OpenClaw users. Phase 6 (Snowclaw) is the primary consumer. Phase 5 (Context-VM) is the long-term vision but least urgent. UI is optional — CLI + MCP covers most use cases.

---

## Crate Structure (Updated)

```
nomen/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Library crate root (pub API)
│   ├── db.rs                # SurrealDB connection, schema, migrations
│   ├── config.rs            # Configuration
│   ├── relay.rs             # Nostr relay sync (NIP-42, NIP-44, NIP-78)
│   ├── memory.rs            # Memory types and parsing
│   ├── search.rs            # Hybrid search (vector + BM25 + graph)
│   ├── embed.rs             # Embedding trait + providers
│   ├── entities.rs          # Entity extraction + graph management
│   ├── groups.rs            # Group hierarchy, membership, scope resolution
│   ├── access.rs            # Tier enforcement, access checks
│   ├── ingest.rs            # Raw message ingestion
│   ├── consolidate.rs       # Consolidation pipeline
│   ├── llm.rs               # LLM provider trait (OpenRouter, local)
│   ├── mcp.rs               # MCP server (stdio + HTTP)
│   ├── contextvm.rs         # Nostr context-VM (kind 21900/21901)
│   ├── daemon.rs            # Combined daemon mode
│   ├── migrate.rs           # Import from external sources
│   └── display.rs           # Terminal output formatting
├── docs/
│   ├── architecture.md
│   ├── nostr-memory-spec.md
│   └── implementation-plan.md  # This file
└── Cargo.toml
```

---

## Design Decisions (Resolved)

1. **Nostr SDK** — Yes, use `nostr-sdk` (already in Cargo.toml). Don't reinvent relay management.
2. **Embeddings** — OpenRouter + OpenAI APIs, same as Snowclaw. Reuse Snowclaw's implementation as reference. Use Snowclaw's config/keys/tokens for testing.
3. **LLM for consolidation** — Configurable per task. An agent can use its own LLM for consolidation. Nomen should also work standalone (without an agent) with its own configured LLM provider.
4. **SurrealDB** — Commit to it. Rethink only if real problems arise.
5. **Context-VM event kinds** — 21900/21901 (ephemeral range). Start ephemeral, formalize later.
6. **Multi-relay** — single relay for now.
7. **MCP crate** — evaluate `rmcp` vs hand-roll.
