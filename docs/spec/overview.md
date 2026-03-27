# Nomen — System Overview

Nomen is a Rust CLI and library for **Nostr-native agent memory**. It stores all persistent data as Nostr events on relays, caches locally in SurrealDB with hybrid vector + BM25 search, and consolidates high-volume conversation messages into durable knowledge.

## Core Ideas

1. **Relay events are the single source of truth.** Every piece of persistent data — messages, memories, entities, groups — exists as a Nostr event on the relay. SurrealDB is a local cache/index for search and fast access. If local state is lost, everything is rebuilt from the relay.

2. **Messages in, memories out.** Conversation messages (from any platform) are collected as kind 30100 events, then consolidated by an LLM into kind 31234 memories. The analogy is sleep: replay raw experience, extract durable knowledge.

3. **Five-tier visibility.** Memories have visibility levels (`public`, `group`, `circle`, `personal`, `private`) that control access and encryption. Scope identifies the boundary (group id, pubkey, circle hash).

4. **Transport-independent API.** A single canonical dispatch layer serves all transports: CLI, MCP (stdio), HTTP, ContextVM (Nostr-native), and socket.

5. **Canonical message hierarchy.** Normalized messages use `platform → community? → chat → thread? → message`. This is separate from the memory model's `visibility/scope/topic`.

## Data Flow

```
Nostr Relay (source of truth — all persistent data)
    │ bidirectional sync
    ▼
SurrealDB (local cache/index — search, embeddings, fast queries)
    │
Canonical dispatch layer
    │
┌────────┬──────────┬──────────┬──────────┬──────────┐
│  CLI   │  MCP     │  HTTP    │ContextVM │  Socket  │
└────────┴──────────┴──────────┴──────────┴──────────┘
```

## Nostr Events

All persistent data has a relay representation. The DB is always rebuildable from events.

| Data | Kind | Status | Description |
|---|---|---|---|
| Messages | 30100 | ✅ | Parameterized replaceable. Bridged from any platform. Input to consolidation. |
| Memories | 31234 | ✅ | Addressable/replaceable. Includes entities (`type=entity:*`) and clusters (`type=cluster`). |
| Groups | 30000 | 🔜 planned | Application-specific data event. Group definitions and membership. |

Entities are memories with a `type` tag — no separate event kind needed. See [data-model.md](data-model.md) for full event schemas.

Groups will use a generic Nostr application data event (kind 30000, parameterized replaceable) to store group definitions. Circle encryption key management is a separate design concern (see [design/circles.md](../design/circles.md)).

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

## Local Storage

Single embedded SurrealDB database (`~/.nomen/db/`, SurrealKV engine). This is a **cache** — all data is recoverable from the relay.

### Tables

| Table | Caches | Description |
|---|---|---|
| `message` | kind 30100 | Collected messages with platform/chat/thread indexes |
| `memory` | kind 31234 | All memories: regular, entities, clusters. Embeddings + search indexes |
| `group` | kind 30000 | Group definitions and membership |

### Indexes

- HNSW vector index on `memory.embedding` (1536 dims, cosine)
- BM25 full-text on `memory.content`
- Unique index on `memory.d_tag`
- Compound indexes on messages: `(platform, chat_id)`, `(chat_id, thread_id)`

### Graph Edges

Relationships are stored as tags on relay events (see [data-model.md](data-model.md)). The DB materializes them as graph edges for traversal:

| Edge | From → To | Source tag |
|---|---|---|
| `mentions` | memory → entity | `["mentions", "<d-tag>"]` |
| `references` | memory → memory | `["ref", "<d-tag>", "<relation>"]` |
| `related_to` | entity → entity | `["rel", "<d-tag>", "<relation>"]` |
| `consolidated_from` | memory → messages | `["source", "<d-tag>"]` |

## Dependencies

```
nostr-sdk 0.43, surrealdb 2, clap 4, tokio 1, reqwest 0.12,
serde/serde_json, anyhow, tracing, chacha20poly1305, hkdf, sha2
```
