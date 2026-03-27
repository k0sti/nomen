# Nomen — System Overview

Nomen is a Rust CLI and library for **Nostr-native agent memory**. It stores memories as custom Nostr events on relays, caches them locally in SurrealDB with hybrid vector + BM25 search, and consolidates high-volume conversation messages into durable knowledge.

## Core Ideas

1. **Nostr relay is source of truth.** All persistent data lives as Nostr events. Local DB is a cache/index. If local state is lost, everything recovers from the relay.

2. **Messages in, memories out.** Conversation messages (from any platform) are collected as kind 30100 events, then consolidated by an LLM into kind 31234 named memories. The analogy is sleep: replay raw experience, extract durable knowledge.

3. **Five-tier visibility.** Memories have visibility levels (`public`, `group`, `circle`, `personal`, `private`) that control access and encryption. Scope identifies the boundary (group id, pubkey, circle hash).

4. **Transport-independent API.** A single canonical dispatch layer serves all transports: CLI, MCP (stdio), HTTP, ContextVM (Nostr-native), and socket.

5. **Canonical message hierarchy.** Normalized messages use `platform → community? → chat → thread? → message`. This is separate from the memory model's `visibility/scope/topic`.

## Data Flow

```
Nostr Relay (source of truth, kind 31234 memories)
    │ sync (bidirectional)
    ▼
SurrealDB (embedded local cache)
    │ search / store / ingest / consolidate
    ▼
Canonical dispatch layer
    │
┌────────┬──────────┬──────────┬──────────┬──────────┐
│  CLI   │  MCP     │  HTTP    │ContextVM │  Socket  │
└────────┴──────────┴──────────┴──────────┴──────────┘
```

## What Gets Published to Relay

Nostr events are the source of truth for all persistent data. SurrealDB is a local cache/index.

| Data | Kind | Relay | Description |
|---|---|---|---|
| Named memories | 31234 | ✅ | Addressable/replaceable. D-tag keyed. Core knowledge store. |
| Collected messages | 30100 | ✅ | Parameterized replaceable. Bridged from any platform. Input to consolidation. |
| Entities | TBD | 🔜 planned | Extracted entities (person, project, concept) with typed relationships. Currently local-only; needs a relay-publishable event kind. |
| Sessions | — | ❌ ephemeral | Connection-scoped runtime state. Not persisted beyond the connection lifetime. No relay representation needed. |

### Already implemented
- **Memories (31234)** — full bidirectional sync: publish on write, fetch on sync.
- **Collected messages (30100)** — produced by message collectors (e.g. Nocelium), stored and indexed by Nomen. Upsert by d-tag.

### Planned
- **Entities** — currently extracted during consolidation and stored only in local DB. Should be published as relay events so they survive DB loss and can be shared across instances. Event kind and tag schema TBD.

### Not published
- **Sessions** — ephemeral runtime state (active connections, identity bindings). These are connection-scoped and do not need relay persistence.

## Crate Structure

```
nomen/                  # main binary + lib
├── nomen-core/         # shared types, ops, collected event model
├── nomen-db/           # SurrealDB storage layer
├── nomen-llm/          # consolidation pipeline, LLM providers, grouping
├── nomen-api/          # canonical dispatch + operations
├── nomen-transport/    # MCP, HTTP, socket adapters
├── nomen-relay/        # Nostr relay sync + event publishing
├── nomen-media/        # media storage (Blossom content-addressing)
└── nomen-wire/         # wire protocol types for socket transport
```

## Storage

Single embedded SurrealDB database (`~/.nomen/db/`, SurrealKV engine).

### Core Tables

| Table | Purpose |
|---|---|
| `memory` | Named memories with content, tier, scope, topic, embedding |
| `collected_message` | Ingested messages (kind 30100 events) before/after consolidation |
| `entity` | Extracted entities (person, project, concept) |
| `session` | Active session tracking |
| `nomen_group` | Group definitions and membership |

### Indexes

- HNSW vector index on `memory.embedding` (1536 dims, cosine)
- BM25 full-text on `memory.content`
- Unique index on `memory.d_tag`
- Compound indexes on collected messages: `(platform, chat_id)`, `(chat_id, thread_id)`

### Graph Edges

| Edge | Purpose |
|---|---|
| `mentions` | memory → entity |
| `references` | memory → memory (supports, contradicts, supersedes, summarizes) |
| `consolidated_from` | memory → source collected messages |
| `related_to` | entity → entity (typed: works_on, collaborates_with, etc.) |

## Dependencies

```
nostr-sdk 0.39, surrealdb 2, clap 4, tokio 1, reqwest 0.12,
serde/serde_json, anyhow, tracing, chacha20poly1305, hkdf, sha2
```
