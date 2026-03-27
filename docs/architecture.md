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
Canonical dispatch (src/api/dispatch.rs)
    │
┌────────┬──────────┬──────────┬──────────┬──────────┐
│  CLI   │  MCP     │  HTTP    │ ContextVM│  Socket  │
│ nomen  │  stdio   │ dispatch │  Nostr   │  TCP/Unix│
└────────┴──────────┴──────────┴──────────┴──────────┘
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
├── cvm.rs            (500)  ContextVM server (Nostr MCP gateway + CvmHandler)
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
- `raw_message` — ingested messages before consolidation (local only, never published to relay)
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

## Memory Tiers (5-level visibility model)

| Visibility | Scope | Encryption | Access |
|------------|-------|-----------|--------|
| Public | `""` (empty) | None | All agents on relay |
| Group | NIP-29 group id | None (relay auth) | Group members |
| Circle | Deterministic participant-set hash | NIP-44 to participant set | Circle participants (unimplemented) |
| Personal | Hex pubkey | NIP-44 self-encrypt | Owning agent only |
| Internal | Hex pubkey | NIP-44 self-encrypt | Agent-only reasoning |

Legacy `private` is accepted on read and normalized to `personal`.

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
- **conversation container** = canonical `platform/community/chat/thread` hierarchy for normalized messages (legacy: `channel`)

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

`channel` is a legacy/raw-message term for provider-specific container identity.

For normalized collected-message data, the canonical hierarchy is:

**platform → community → chat → thread → message**

Examples:

- Telegram group chat: `platform=telegram`, `chat_id=-1003821690204`
- Telegram forum topic: same `chat_id`, plus `thread_id=<topic_id>`
- Discord threaded message: `platform=discord`, `community_id=<guild_id>`, `chat_id=<channel_id>`, `thread_id=<thread_id>`

### Rule

- durable memories attach to **scope**
- normalized messages attach to **platform/community/chat/thread/message**
- legacy raw-message `channel` metadata may still exist for compatibility and ingestion flows
- provider/container details must not be embedded into durable memory d-tags

### Integration

Host integrations should provide or resolve a `scope` for memory operations. For messaging data, prefer structured fields/tags (`platform`, optional `community`, `chat`, optional `thread`) over overloading `channel` as a universal canonical term.

## Interfaces

### Canonical API (v2)

All external operations are defined in the **canonical API layer** (`src/api/dispatch.rs`). The canonical operation model is transport-independent. ContextVM, MCP, HTTP, and socket are transport adapters. All transports (HTTP, MCP, ContextVM, socket) route through the same `api::dispatch()` function.

**21 operations** across 5 domains:

| Domain | Operations |
|--------|-----------|
| Memory | `memory.search`, `memory.put`, `memory.get`, `memory.list`, `memory.delete` |
| Message | `message.ingest`, `message.query`, `message.context`, `message.send` |
| Entity | `entity.list`, `entity.relationships` |
| Maintenance | `memory.consolidate`, `memory.cluster`, `memory.sync`, `memory.embed`, `memory.prune` |
| Group | `group.list`, `group.members`, `group.create`, `group.add_member`, `group.remove_member` |

All operations use canonical fields: `visibility`, `scope`, `topic`, and for messages: `platform`, optional `community`, `chat`, optional `thread`.

Responses use structured envelopes: `{ "ok": true, "result": { ... } }` or `{ "ok": false, "error": { "code": "...", "message": "..." } }`.

See `docs/api-v2-spec.md` for full specification.

### ContextVM — Nostr-Native Transport

ContextVM is the Nostr-native transport adapter, carrying canonical operations over encrypted Nostr events.

- Encrypted transport (NIP-44 / NIP-59)
- Identity via Nostr keypairs
- Server announcements and discovery
- Supports both MCP-style `tools/call` dispatch and direct action dispatch (e.g. method `"memory.search"`)
- ACL (allowed npubs) and rate limiting at application level

Implementation: `src/cvm.rs` — `CvmServer` wraps the SDK gateway; `CvmHandler` provides the testable message-handling logic.

### MCP Server — Wrapper/Projection

MCP is a **wrapper over the canonical API** for agent frameworks that speak MCP (JSON-RPC over stdio).

- Tool names use underscore format: `memory_search`, `memory_put`, etc.
- Same argument shapes and semantics as ContextVM
- Calls `api::dispatch()` internally — no separate logic

Implementation: `src/mcp.rs`.

### Socket — Local Event-Capable Transport

Socket is a **local-only transport** for efficient shared access to the Nomen runtime by local AI agents and trusted processes. It is not the preferred remote transport (use HTTP for that).

- Canonical operations use the same `action + params → ApiResponse` flow as other transports
- Transport-specific capabilities: `subscribe` and `unsubscribe` for push event management
- These are **not** canonical API actions — they are connection-scoped transport features
- Push events (e.g. `memory.updated`, `agent.connected`) are delivered via a separate event frame type

Implementation: `src/socket.rs`. Wire protocol types: `nomen-wire/src/types.rs`.

### Transport Comparison

| | HTTP | MCP | ContextVM | Socket |
|---|---|---|---|---|
| **Primary use** | Remote generic | Local agent compat | Nostr-native remote | Local shared access |
| **Canonical dispatch** | Yes | Yes (via tool mapping) | Yes (direct + tools/call) | Yes |
| **Framing** | HTTP POST | JSON-RPC stdio | Nostr events (NIP-44/59) | Length-prefixed JSON frames |
| **Transport-specific features** | Health, stats, config endpoints | Tool listing, initialize | Encryption, allowlist, rate limit | Subscribe/unsubscribe, push events |
| **Auth** | None (planned) | N/A (local) | Nostr keypairs + ACL | Unix permissions |
| **Implementation** | `http.rs` | `mcp.rs` | `cvm.rs` | `socket.rs` |

All transports share the same canonical operation semantics. Transport-specific features are clearly separated from the canonical API.

### CLI

```
nomen list / store / delete / search / sync
nomen send --to <recipient>
nomen ingest / consolidate / prune
nomen messages / entities / group
nomen embed / cluster
nomen serve --stdio | --http <addr> [--context-vm]
```

### Serve Mode Combinations

The `nomen serve` command supports running multiple interfaces concurrently:

| Mode | Command | Description |
|------|---------|-------------|
| stdio MCP only | `nomen serve` | Default. MCP JSON-RPC over stdin/stdout. |
| HTTP only | `nomen serve --http :3000` | HTTP dispatch + web UI. |
| CVM only | `nomen serve --context-vm` | ContextVM listener only. |
| Socket only | `nomen serve --socket /tmp/nomen.sock` | Unix/TCP socket transport (canonical dispatch + transport-specific subscriptions). |
| CVM + stdio MCP | `nomen serve --context-vm --stdio` | ContextVM + stdio MCP (both run). |
| HTTP + CVM | `nomen serve --http :3000 --context-vm` | Both HTTP and ContextVM run concurrently. |

CVM requires nsec keys (via config or `--nsec`). The `[contextvm]` config section controls relay, encryption, allowlist, and rate limiting.

### HTTP Server (`nomen serve --http :3000`)

First-class remote transport for agents and web UIs. Exposes the canonical dispatch endpoint at `POST /memory/api/dispatch` accepting `{ action, params }` envelopes. Additional utility endpoints for health, stats, and config are served alongside.

### CVM Transport Notes

ContextVM uses Nostr kind 25910 (ephemeral) for unencrypted messages and kind 1059 (NIP-59 gift wrap) for encrypted messages. The server subscribes to both kinds filtered by `p` tag matching its own pubkey.

**Encryption modes:**
- `disabled` — plaintext kind 25910 events. Tested and working with Zooid relay.
- `optional` (default) — defaults to gift-wrap encryption. Gift-wrap delivery depends on relay support for NIP-59 kind 1059 events with `p` tag filtering. Some relays may not deliver these reliably.
- `required` — always gift-wrap encrypted.

For initial setup, use `encryption = "disabled"` in `[contextvm]` config and verify basic round-trip before enabling encryption.

### CVM Smoke Test

A reusable smoke-test client lives in `examples/cvm_smoke_test.rs`:

```bash
# Verify a running Nomen CVM server over a relay
NOMEN_SERVER_PUBKEY=<hex> NOMEN_NSEC=<nsec> ./scripts/cvm-smoke-test.sh

# Test with encryption disabled (recommended for initial verification)
NOMEN_SERVER_PUBKEY=<hex> NOMEN_NSEC=<nsec> NOMEN_ENCRYPTION=disabled ./scripts/cvm-smoke-test.sh
```

This sends `tools/list` and `memory.list` requests and verifies responses.

## Relay Sync

- **Source of truth:** Nostr relay (kind 31234)
- **Sync:** `nomen sync` fetches events, upserts into SurrealDB by d-tag
- **Publish:** `nomen store` creates local record + publishes to relay
- **Dedup:** d-tag uniqueness, latest timestamp wins

### What Gets Published to Relay

Only **named memories** are published to the Nostr relay as kind 31234 replaceable events. These are created by `memory.put` (direct API) or `memory.consolidate` (LLM extraction from collected messages / legacy raw-message compatibility inputs).

**Raw messages** (`message.ingest`) are stored **locally in SurrealDB only** — they are never published to the relay. They are ephemeral input to the consolidation pipeline, consumed and marked `consolidated = true` once processed. This is intentional: raw messages are high-volume conversation noise; only the distilled knowledge (named memories) is durable and worth syncing across relays.

| Data | SurrealDB | Nostr Relay |
|---|---|---|
| Named memories (`memory.put`, consolidation output) | ✅ `memory` table | ✅ kind 31234 |
| Raw messages (`message.ingest`) | ✅ `raw_message` table | ❌ local only |
| Entities (extracted) | ✅ `entity` table | ❌ local only |
| Sessions | ✅ `session` table | ❌ local only |

## Dependencies

```
nostr-sdk 0.39, surrealdb 2 (kv-surrealkv), clap 4, tokio 1,
reqwest 0.12 (embeddings API), serde/serde_json, anyhow, tracing
Optional: rusqlite (migration), snow-memory (Snowclaw adapter)
```
