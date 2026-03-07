# Nomen Architecture

**Version:** v0.3
**Date:** 2026-03-05

## Overview

Nomen is a Rust CLI and library for Nostr-native agent memory. Memories are custom kind 31234 events on Nostr relays, cached locally in SurrealDB with hybrid vector + BM25 search.

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
├── relay.rs          (223)  Nostr relay sync
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
└── migrate.rs        (136)  SQLite → SurrealDB migration
```

Total: ~6900 LOC Rust.

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
- Unique index on `memory.d_tag` (replaceable key (kind 31234))

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

## Groups

Two kinds of groups serve different purposes:

### Named Groups

Pre-defined groups with an ID, name, and explicit member list. Configured in `config.toml` or created via CLI. Can map to a NIP-29 relay group via `nostr_group` field.

```toml
[[groups]]
id = "atlantislabs.engineering"
name = "Engineering"
members = ["npub1abc...", "npub1def..."]
nostr_group = "techteam"    # maps to NIP-29 h-tag on relay
relay = "wss://zooid.atlantislabs.space"
```

- Hierarchical IDs with dot separator (`atlantislabs.engineering.infra`)
- Parent derived automatically (`atlantislabs.engineering` → parent `atlantislabs`)
- Membership is explicit per level — being in a parent doesn't grant child access
- `nostr_group` enables bidirectional mapping: scope ↔ NIP-29 h-tag
- Stored in SurrealDB `nomen_group` table, config entries merged on load

**CLI:** `nomen group create/list/members/add/remove`

### Ad-hoc npub Sets

Implicit groups formed by a set of participants — like a multi-party DM. No pre-configuration needed. The group identity is the sorted set of npubs.

- **Session ID:** `hash(sorted npubs)` — deterministic, same participants always produce the same scope
- **Tier:** private (encrypted between participants)
- **Use case:** Multi-party conversations, pair-wise agent interactions
- **No relay mapping** — these are direct NIP-17 DM conversations, not NIP-29 groups

**Status:** Designed but not yet implemented. Currently, only named groups and single-npub private sessions are supported in session resolution.

### Comparison

| | Named Groups | Ad-hoc npub Sets |
|---|---|---|
| Configuration | Explicit (config/CLI) | Implicit (from participants) |
| Identity | Human-readable ID | Hash of sorted npubs |
| Membership | Managed list | Fixed set |
| Relay mapping | NIP-29 via `nostr_group` | None (NIP-17 DMs) |
| Hierarchy | Dot-separated nesting | Flat |
| Use case | Teams, projects, communities | DMs, pair-wise interactions |

## Messaging (`nomen send`)

Agents send messages to recipients via configurable channels.

```
nomen send "hello" --to npub1abc...              # NIP-17 DM (default)
nomen send "update" --to group:techteam           # NIP-29 group message
nomen send "announcement" --to public             # Kind 1 note
nomen send "hey" --to npub1abc... --channel telegram  # Telegram DM
```

**Routing:** recipient format determines tier and delivery:

| Recipient | Channel | Delivery | Encryption |
|-----------|---------|----------|------------|
| `npub1...` | nostr (default) | Kind 1059 (NIP-17 DM) | NIP-44 |
| `npub1...` | telegram | Telegram Bot API | Platform TLS |
| `group:<id>` | nostr (default) | Kind 9 (NIP-29) | None (relay auth) |
| `group:<id>` | telegram | Telegram group | Platform TLS |
| `public` | nostr (default) | Kind 1 (note) | None |

All sent messages are stored locally as `raw_message` with `source="nomen"`.

## Session ID Model

A session ID encodes recipient + channel + tier in a single string, eliminating the need to pass tier/scope separately on every call.

**Formats:**

| Session ID | Type | Resolves to | Status |
|---|---|---|---|
| `public` | Public | tier=public, empty scope | ✅ |
| `npub1abc...` | Private DM | tier=private, scope=npub hex | ✅ |
| `telegram:npub1abc...` | Private DM (explicit channel) | tier=private, channel=telegram | ✅ |
| `techteam` | Named group | tier=group, scope=group_id | ✅ |
| `nostr:inner-circle` | Named group (via NIP-29 alias) | tier=group, resolved via `nostr_group` | ✅ |
| `hash(sorted npubs)` | Ad-hoc npub set | tier=private, scope=hash | 📋 planned |

**Resolution** (`src/session.rs`): `resolve_session(id, groups, default_channel) → ResolvedSession { tier, scope, channel, group_id, participants }`

Named groups resolve via GroupStore lookup (by ID or `nostr_group` alias). Ad-hoc npub sets will resolve by looking up the hash in the session table.

**Integration:** All MCP tools accept optional `session_id` parameter. When provided, tier and scope are derived automatically — no risk of misconfiguring visibility.

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

- **Source of truth:** Nostr relay (kind 31234)
- **Sync:** `nomen sync` fetches events, upserts into SurrealDB by d-tag
- **Publish:** `nomen store` creates local record + publishes to relay
- **Dedup:** d-tag uniqueness, latest timestamp wins

## Dependencies

```
nostr-sdk 0.39, surrealdb 2 (kv-surrealkv), clap 4, tokio 1,
reqwest 0.12 (embeddings API), serde/serde_json, anyhow, tracing
Optional: rusqlite (migration), snow-memory (Snowclaw adapter)
```
