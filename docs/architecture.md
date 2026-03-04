# Nomen Architecture

**Version:** v0.2
**Date:** 2026-03-04
**Status:** Design

## Overview

Nomen is a Rust CLI and library for managing agent memory backed by Nostr events (NIP-78) and SurrealDB. It provides unified storage, semantic search, graph-based entity linking, memory consolidation, and tier-based privacy scoping.

## Data Flow

```
                    ┌─────────────────┐
                    │   Nostr Relay    │
                    │  (NIP-78 k:30078)│
                    └────────┬────────┘
                             │ sync (bidirectional)
                             │
                    ┌────────▼────────┐
                    │    Ingestor     │
                    │  parse events   │
                    │  generate embeds│
                    └────────┬────────┘
                             │
                    ┌────────▼────────┐
                    │   SurrealDB     │
                    │   (embedded)    │
                    │                 │
                    │  documents      │
                    │  vectors (HNSW) │
                    │  graph edges    │
                    │  full-text      │
                    └────────┬────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
        ┌─────▼─────┐ ┌─────▼─────┐ ┌─────▼─────┐
        │  CLI       │ │  Library  │ │  Snowclaw  │
        │  nomen     │ │  crate    │ │  bridge    │
        └───────────┘ └───────────┘ └───────────┘
```

## Storage: SurrealDB (Embedded)

Single database engine replaces the need for separate SQLite + vector DB + graph DB.

### Why SurrealDB

- **Multi-model:** Documents + graphs + vectors + full-text in one engine
- **Rust-native:** Embeds directly in process via `surrealdb` crate
- **ACID:** Full transactions, no consistency issues
- **Graph first-class:** `RELATE` edges with data, bidirectional traversal
- **Vector search:** HNSW indexes, cosine/euclidean similarity
- **Full-text:** BM25 with analyzers, combinable with vector search
- **Persistent:** SurrealKV (pure Rust) or RocksDB backends
- **Schemaful or schemaless:** Can enforce types or stay flexible

### Crate Configuration

```toml
[dependencies]
surrealdb = { version = "2", features = ["kv-surrealkv"] }
```

Use `kv-surrealkv` for pure-Rust persistent storage (no C deps). Alternative: `kv-rocksdb` for proven performance, but adds C compilation requirement.

### Storage Path

```
~/.nomen/db/          # SurrealDB data directory
~/.nomen/config.toml  # Nomen configuration
```

## Schema Design

### Tables

```surql
-- Core memory record
DEFINE TABLE memory SCHEMAFULL;
DEFINE FIELD content    ON memory TYPE string;          -- the memory text
DEFINE FIELD summary    ON memory TYPE option<string>;  -- L0 abstract
DEFINE FIELD embedding  ON memory TYPE option<array<float>>;
DEFINE FIELD tier       ON memory TYPE string;          -- public | group | private
DEFINE FIELD scope      ON memory TYPE string;          -- "" | group_id | npub
DEFINE FIELD topic      ON memory TYPE string;          -- namespace/category
DEFINE FIELD confidence ON memory TYPE option<float>;
DEFINE FIELD source     ON memory TYPE string;          -- agent npub that created it
DEFINE FIELD model      ON memory TYPE option<string>;  -- LLM model used
DEFINE FIELD version    ON memory TYPE int DEFAULT 1;
DEFINE FIELD nostr_id   ON memory TYPE option<string>;  -- relay event id
DEFINE FIELD d_tag      ON memory TYPE option<string>;  -- NIP-78 d-tag (replaceable)
DEFINE FIELD created_at ON memory TYPE datetime;
DEFINE FIELD updated_at ON memory TYPE datetime;
DEFINE FIELD ephemeral  ON memory TYPE bool DEFAULT false;  -- pre-consolidation

-- Entity extracted from memories
DEFINE TABLE entity SCHEMAFULL;
DEFINE FIELD name       ON entity TYPE string;
DEFINE FIELD kind       ON entity TYPE string;  -- person | project | concept | place
DEFINE FIELD attributes ON entity TYPE option<object>;
DEFINE FIELD created_at ON entity TYPE datetime;

-- Group/scope hierarchy
DEFINE TABLE scope SCHEMAFULL;
DEFINE FIELD name       ON scope TYPE string;
DEFINE FIELD parent     ON scope TYPE option<record<scope>>;
DEFINE FIELD tier       ON scope TYPE string;   -- group | private
DEFINE FIELD created_at ON scope TYPE datetime;
```

### Indexes

```surql
-- Vector similarity search (HNSW)
DEFINE INDEX memory_embedding ON memory FIELDS embedding
  HNSW DIMENSION 1536 DIST COSINE EFC 150 M 12;

-- Full-text search
DEFINE ANALYZER memory_analyzer TOKENIZERS class FILTERS ascii, lowercase, snowball(english);
DEFINE INDEX memory_fulltext ON memory FIELDS content
  FULLTEXT ANALYZER memory_analyzer BM25;

-- Lookups
DEFINE INDEX memory_tier   ON memory FIELDS tier;
DEFINE INDEX memory_scope  ON memory FIELDS scope;
DEFINE INDEX memory_topic  ON memory FIELDS topic;
DEFINE INDEX memory_d_tag  ON memory FIELDS d_tag UNIQUE;
DEFINE INDEX entity_name   ON entity FIELDS name UNIQUE;
```

### Graph Edges

```surql
-- Memory mentions an entity
DEFINE TABLE mentions SCHEMAFULL;
DEFINE FIELD in  ON mentions TYPE record<memory>;
DEFINE FIELD out ON mentions TYPE record<entity>;
DEFINE FIELD relevance ON mentions TYPE option<float>;

-- Memory references another memory
DEFINE TABLE references SCHEMAFULL;
DEFINE FIELD in  ON references TYPE record<memory>;
DEFINE FIELD out ON references TYPE record<memory>;
DEFINE FIELD relation ON references TYPE string;  -- supports | contradicts | supersedes | elaborates

-- Memory was consolidated from other memories
DEFINE TABLE consolidated_from SCHEMAFULL;
DEFINE FIELD in  ON consolidated_from TYPE record<memory>;   -- new consolidated memory
DEFINE FIELD out ON consolidated_from TYPE record<memory>;   -- original ephemeral memory

-- Entity relates to entity
DEFINE TABLE related_to SCHEMAFULL;
DEFINE FIELD in  ON related_to TYPE record<entity>;
DEFINE FIELD out ON related_to TYPE record<entity>;
DEFINE FIELD relation ON related_to TYPE string;  -- works_on | knows | located_in | part_of
```

### Example Queries

```surql
-- Semantic search with tier filtering
SELECT *, vector::similarity::cosine(embedding, $query_vec) AS score
  FROM memory
  WHERE tier IN ['public', 'group']
    AND scope IN ['', 'techteam']
  ORDER BY score DESC
  LIMIT 10;

-- Hybrid search (vector + full-text)
SELECT *, 
  vector::similarity::cosine(embedding, $query_vec) AS vec_score,
  search::score(1) AS text_score
  FROM memory
  WHERE content @1@ $query_text
  ORDER BY (vec_score * 0.7 + text_score * 0.3) DESC
  LIMIT 10;

-- Graph: find all entities related to a memory
SELECT ->mentions->entity FROM memory:abc123;

-- Graph: find memories about a specific entity
SELECT <-mentions<-memory FROM entity:alhovuori;

-- Graph: what was this memory consolidated from?
SELECT ->consolidated_from->memory FROM memory:consolidated_xyz;

-- Scope hierarchy: get all ancestor scopes
SELECT parent.* FROM scope:techteam;

-- Multi-hop: entities connected to entities I know about
SELECT ->related_to->entity FROM entity:k0;
```

## Nostr Relay Sync

The relay is the **source of truth** and **sync mechanism**. SurrealDB is the local index.

### Ingest (relay → local)

1. Subscribe to `kinds: [30078]` for configured npubs
2. Parse event content, d-tag, custom tags
3. Upsert into SurrealDB `memory` table (d-tag as unique key)
4. Generate embedding via configured provider (OpenAI, local model, etc.)
5. Run entity extraction (LLM-powered or regex for known patterns)
6. Create `mentions` edges for extracted entities
7. Check similarity against existing memories for dedup alerts

### Publish (local → relay)

1. Create/update memory in SurrealDB
2. Build NIP-78 event (kind 30078, d-tag, custom tags)
3. Encrypt if private tier (NIP-44 self-to-self)
4. Sign with configured nsec
5. Publish to relay
6. Store relay event ID back in SurrealDB record

### Conflict Resolution

- NIP-78 replaceable events: latest timestamp wins on relay
- Local: compare `updated_at`, keep newest
- Contradictions: mark with `references` edge (relation: "contradicts"), let consolidation resolve

## Memory Tiers

| Tier | Scope | Encryption | Visibility |
|------|-------|-----------|------------|
| Public | `""` (empty) | None | All agents on relay |
| Group | `group:<id>` | None (relay auth gates access) | Group members |
| Private | `npub:<hex>` | NIP-44 | Only the owning agent |

Scope prefix in d-tag determines tier:
- `snow:memory:core:*` → Public
- `snow:memory:group:<id>:*` → Group  
- `snow:memory:npub:<hex>:*` → Private
- `snow:memory:lesson:*` → Public

## Memory Consolidation

Ephemeral memories (raw autosaves) get consolidated into durable named memories:

1. **Group by topic/scope** — cluster ephemeral memories by semantic similarity + shared scope
2. **Summarize** — LLM generates consolidated summary from cluster
3. **Create new memory** — named, with proper topic, higher confidence
4. **Link provenance** — `consolidated_from` edges to originals
5. **Clean up** — mark originals as consolidated (or delete via NIP-09)

Consolidation can run:
- On-demand via CLI (`nomen consolidate`)
- Periodically via cron/heartbeat
- Triggered when ephemeral count exceeds threshold

## Embedding Strategy

- **Default model:** OpenAI `text-embedding-3-small` (1536 dims, cheap)
- **Alternative:** Local model via vLLM/Ollama (for privacy)
- **Dimension:** 1536 (configurable, must match HNSW index)
- Embeddings generated at ingest time, stored alongside content

## CLI Commands (Planned)

```
nomen list                    # list memories (current: implemented)
nomen search <query>          # hybrid semantic + full-text search
nomen store <text>            # store a new memory
nomen consolidate             # run consolidation pass
nomen sync                    # force relay sync
nomen entities                # list extracted entities
nomen graph <entity>          # show entity relationships
nomen delete <id>             # delete memory (+ NIP-09)
nomen import                  # import from external sources
nomen config                  # show/edit configuration
```

## Crate Structure (Future)

```
nomen/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── db.rs                # SurrealDB connection & schema
│   ├── sync.rs              # Nostr relay sync
│   ├── ingest.rs            # Event parsing, embedding, entity extraction
│   ├── search.rs            # Hybrid search (vector + FTS + graph)
│   ├── consolidate.rs       # Memory consolidation pipeline
│   ├── entities.rs          # Entity extraction & graph management
│   └── config.rs            # Configuration
├── docs/
│   ├── architecture.md      # This file
│   └── nostr-memory-spec.md # NIP-78 event format
└── Cargo.toml
```

## Dependencies

```toml
[dependencies]
surrealdb = { version = "2", features = ["kv-surrealkv"] }
nostr-sdk = "0.37"
clap = { version = "4", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
chrono = { version = "0.4", features = ["serde"] }
```

## Snowclaw Integration

Nomen is standalone but designed to serve as Snowclaw's memory backend:

1. **Library crate** — Snowclaw imports `nomen` as a dependency
2. **Shared DB** — Snowclaw points to same SurrealDB data dir
3. **Recall API** — `nomen::search(query, tier, scope)` replaces `CollectiveMemory::recall()`
4. **Write API** — `nomen::store(content, tier, scope, topic)` replaces direct relay publish
5. **Migration** — import existing `memories.db` SQLite data into SurrealDB

This decouples memory from Snowclaw's upstream, so memory evolution doesn't require zeroclaw rebases.

## Open Questions

1. **Embedding provider config** — use OpenRouter? Local model? Configurable per-instance?
2. **Entity extraction** — LLM-powered (expensive) vs regex/heuristic (cheap, limited)?
3. **SurrealKV vs RocksDB** — SurrealKV is pure Rust (simpler build), RocksDB is battle-tested. Start with SurrealKV?
4. **Multi-agent** — if multiple agents share a relay, do they share a local DB or each maintain their own?
5. **Consolidation trigger** — time-based, count-based, or both?
