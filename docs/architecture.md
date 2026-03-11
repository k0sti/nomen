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
├── main.rs          (2084)  CLI entry, clap commands
├── lib.rs            (560)  Nomen struct, public API (incl. ClusterOptions)
├── db.rs            (1700)  SurrealDB schema, queries, CRUD, graph traversal
├── search.rs         (340)  Hybrid vector + BM25 search + graph expansion
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
├── entities.rs       (380)  Entity extraction (heuristic + LLM) + typed relationships
├── cluster.rs        (420)  Cluster fusion — namespace-grouped memory synthesis
├── groups.rs         (373)  Group management
├── config.rs         (280)  TOML config (~/.config/nomen/config.toml)
├── access.rs         (132)  Access control
├── display.rs         (77)  Formatted output
└── migrate.rs        (136)  SQLite → SurrealDB migration
```

Total: ~8200 LOC Rust.

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
- `references` — memory → memory (supports, contradicts, supersedes, summarizes)
- `consolidated_from` — consolidated memory → source messages
- `related_to` — entity → entity (typed: works_on, collaborates_with, decided, depends_on, member_of, etc.)

Graph edges are used for both provenance tracking and **retrieval expansion**. See Graph-Aware Retrieval below.

### Graph-Aware Retrieval

Search supports a `graph_expand` post-processing step that traverses edges from direct search hits to discover related memories:

```
hybrid_search(query)
  → top-K results
  → for each result with a d_tag, traverse 1-hop graph edges
  → score expanded results: parent_score × edge_type_weight
  → merge into final ranked list (dedup by d_tag)
```

Edge type weights control how strongly different connections influence retrieval:

| Edge Type | Weight | Rationale |
|-----------|--------|-----------|
| `contradicts` | 0.8 | Conflicts are critical context |
| `mentions` (shared entity) | 0.7 | Entity co-occurrence is strong signal |
| `references` | 0.6 | Supporting/related evidence |
| `references` (supersedes) | 0.5 | Older version, lower priority |
| `consolidated_from` | 0.3 | Provenance, lower relevance |

Graph-expanded results carry `MatchType::Graph` and include the edge type that connected them. Contradictions are flagged with `contradicts: true` for downstream handling.

Enabled via `--graph` CLI flag, `graph_expand` in MCP/API, with configurable `max_hops` (default 1).

### Cluster Fusion

Periodic pipeline that groups memories by topic namespace prefix and synthesizes coherent summaries:

```
Named memories (user/k0/preferences, user/k0/timezone, user/k0/projects)
  → Group by prefix at depth N (e.g. "user/k0" at depth 2)
  → Filter: clusters with ≥ min_members
  → LLM synthesis → coherent cluster summary
  → Store as "cluster/user/k0" with references edges (relation: "summarizes")
```

Cluster memories are replaceable by d-tag (refreshed on next run). Tier is derived from the most restrictive member tier (internal > personal > group > public).

Configured via `[memory.cluster]` in config.toml. CLI: `nomen cluster [--dry-run] [--prefix]`.

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

## Scope + Channel Model

Nomen now uses a simplified model:

- **scope** = durable Nostr-native boundary used by memories and access control
- **channel** = concrete message container where raw events were observed

This replaces the earlier idea of making `session_id` the main integration handle.

### Scope

Scope is defined the same way everywhere memory uses it:

| Visibility | Scope value |
|---|---|
| `public` | empty |
| `group` | NIP-29 group id |
| `circle` | deterministic participant-set hash |
| `personal` | hex pubkey |
| `internal` | hex pubkey |

### Channel

Channel is provider-specific transport/container identity. Examples:

- `nostr-group:wss://zooid.atlantislabs.space:techteam`
- `nostr-dm:<peer-pubkey-hex>`
- `telegram:-1003821690204:694`
- `discord:<guild_id>:<channel_id>:<thread_id>`

### Rule

- durable memories attach to **scope**
- raw messages attach to **channel** and resolve to a scope
- channel metadata may vary across providers; scope semantics remain stable

### Integration

Host integrations should provide or resolve a `scope` for memory operations. They may also pass a `channel` when ingesting or querying raw message history, but channel/provider details should not be embedded into durable memory d-tags.

## Zeroclaw/OpenClaw Compatibility

Nomen is designed to interoperate with existing agent runtimes that expose a simpler memory API built around `key`, `category`, and optional `session_id` fields.

Compatibility rules:

- `category` is a host organizational bucket, not a Nomen visibility/scope primitive
- `key` is a host identifier or topic hint, not always a canonical Nomen topic
- `session_id` is a host compatibility hint for partitioning/filtering, not a canonical scope model
- memories attach to `scope`; raw messages attach to `channel` and resolve to scope

See `docs/zeroclaw-integration-spec.md` for the full adapter contract.

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
