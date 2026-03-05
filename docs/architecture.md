# Nomen Architecture

**Version:** v0.3
**Date:** 2026-03-05

## Overview

Nomen is a Rust CLI and library for Nostr-native agent memory. Memories are NIP-78 events (kind 30078) on Nostr relays, cached locally in SurrealDB with hybrid vector + BM25 search.

## Data Flow

```
Nostr Relay (source of truth)
    │ sync (bidirectional)
    ▼
SurrealDB (embedded, local cache)
    │ search / store / ingest
    ▼
┌────────┬──────────┬──────────┐
│  CLI   │  MCP     │  HTTP    │
│ nomen  │  stdio   │  server  │
└────────┴──────────┴──────────┘
```

## Module Map

```
src/
├── main.rs          (1224)  CLI entry, clap commands
├── lib.rs            (284)  Nomen struct, public API
├── db.rs             (918)  SurrealDB schema, queries, CRUD
├── search.rs         (154)  Hybrid vector + BM25 search
├── relay.rs          (223)  Nostr relay sync (NIP-78)
├── memory.rs         (141)  Memory parsing from Nostr events
├── send.rs           (249)  Send messages (DM, group, public)
├── session.rs        (232)  Session ID resolution → tier/scope
├── mcp.rs            (796)  MCP server (JSON-RPC stdio)
├── http.rs           (376)  HTTP server
├── contextvm.rs      (601)  Nostr-native request/response (NIP-44)
├── ingest.rs          (63)  Raw message ingestion
├── consolidate.rs    (354)  Raw messages → named memories
├── embed.rs          (188)  Embedding generation (OpenAI API)
├── entities.rs       (212)  Entity extraction + graph edges
├── groups.rs         (373)  Group management
├── config.rs         (197)  TOML config (~/.config/nomen/config.toml)
├── access.rs         (132)  Access control
├── display.rs         (77)  Formatted output
├── migrate.rs        (136)  SQLite → SurrealDB migration
└── snowclaw_adapter.rs (456) Snowclaw integration bridge
```

Total: ~7400 LOC Rust.

## Storage: SurrealDB (Embedded)

Single embedded database. Multi-model: documents, vectors (HNSW), full-text (BM25), graph edges.

**Path:** `~/.nomen/db/`
**Engine:** SurrealKV (pure Rust, no C deps)

### Core Tables

- `memory` — memories with content, tier, scope, topic, embedding, confidence
- `raw_message` — ingested messages before consolidation
- `entity` — extracted entities (person, project, concept)
- `session` — active session tracking
- `group` — group definitions and membership

### Indexes

- HNSW vector index on `memory.embedding` (1536 dims, cosine)
- BM25 full-text on `memory.content`
- Unique index on `memory.d_tag` (NIP-78 replaceable key)

### Graph Edges

- `mentions` — memory → entity
- `references` — memory → memory (supports, contradicts, supersedes)
- `consolidated_from` — consolidated memory → source messages
- `related_to` — entity → entity

## Memory Tiers

| Tier | Scope | Encryption | Access |
|------|-------|-----------|--------|
| Public | `""` | None | All agents on relay |
| Group | `group:<id>` | None (relay auth) | Group members |
| Private | `npub:<hex>` | NIP-44 | Owning agent only |

## Interfaces

### CLI

```
nomen list / store / delete / search / sync
nomen send --to <recipient>
nomen ingest / consolidate / prune
nomen messages / entities / group
nomen embed
nomen serve --stdio | --http <addr>
```

### MCP Server (`nomen serve --stdio`)

Tools: `nomen_search`, `nomen_store`, `nomen_messages`, `nomen_entities`, `nomen_consolidate`, `nomen_ingest`. All tools accept optional `session_id` for automatic tier/scope resolution.

### HTTP Server (`nomen serve --http :3000`)

REST API for remote agents and web UIs.

### Context-VM (Nostr-native)

NIP-44 encrypted request/response events for pure-Nostr agents. No MCP/HTTP dependency.

## Relay Sync

- **Source of truth:** Nostr relay (NIP-78, kind 30078)
- **Sync:** `nomen sync` fetches events, upserts into SurrealDB by d-tag
- **Publish:** `nomen store` creates local record + publishes to relay
- **Dedup:** d-tag uniqueness, latest timestamp wins

## Dependencies

```
nostr-sdk 0.39, surrealdb 2 (kv-surrealkv), clap 4, tokio 1,
reqwest 0.12 (embeddings API), serde/serde_json, anyhow, tracing
Optional: rusqlite (migration), snow-memory (Snowclaw adapter)
```
