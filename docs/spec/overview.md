# Nomen — System Overview

Nomen is a Rust CLI and library for **Nostr-native agent memory**. It stores memories as custom Nostr events on relays, caches them locally in SurrealDB with hybrid vector + BM25 search, and consolidates high-volume conversation messages into durable knowledge.

## Core Ideas

1. **Nostr relay is source of truth.** Memories are kind 31234 replaceable events. Local DB is a cache. If local state is lost, everything recovers from the relay.

2. **Messages in, memories out.** Raw conversation messages (from any platform) are ingested as kind 30100 collected events, stored locally, then consolidated by an LLM into named memories. The analogy is sleep: replay raw experience, extract durable knowledge.

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

| Data | Local DB | Nostr Relay |
|---|---|---|
| Named memories | ✅ | ✅ kind 31234 |
| Collected messages | ✅ | ❌ local only |
| Entities | ✅ | ❌ local only |
| Sessions | ✅ | ❌ local only |

Only distilled knowledge (named memories) is durable and synced across relays. Collected messages are high-volume conversation input, consumed by consolidation.

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
