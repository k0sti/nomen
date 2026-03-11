# Nomen API Reference

**Version:** 0.4  
**Date:** 2026-03-11

Complete reference for all Nomen operations across CLI, MCP (Model Context Protocol), and Context-VM (Nostr-native) interfaces. All interfaces share the same underlying `Nomen` struct — identical behavior guaranteed.

---

## Interface Overview

| Interface | Transport | Use Case |
|-----------|-----------|----------|
| **CLI** | Terminal | Human operators, scripts, cron jobs |
| **MCP** | JSON-RPC over stdio | Agent frameworks (OpenClaw, Claude, etc.) |
| **Context-VM** | Nostr events (kind 21900/21901) | Pure-Nostr agents, no local process needed |

### Starting Servers

```bash
nomen serve --stdio                              # MCP over stdio
nomen serve --http 127.0.0.1:3000               # HTTP API + web dashboard
nomen serve --stdio --context-vm --allowed-npubs <hex>  # MCP + Context-VM
```

---

## Operations

### search — Search memories

Hybrid semantic (HNSW vector) + full-text (BM25) search across stored memories.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `query` | string | ✅ | positional | ✅ | ✅ | Search query |
| `tier` | string | | `--tier` | ✅ | ✅ | Filter: `public`, `group`, `private` |
| `scope` | string | | | ✅ | ✅ | Filter by scope |
| `limit` | integer | | `--limit` (default 10) | ✅ (default 10) | ✅ (default 10) | Max results |
| `session_id` | string | | | ✅ | ✅ | Auto-derive tier/scope from session |
| `vector_weight` | float | | `--vector-weight` (default 0.7) | ✅ (default 0.7) | ✅ (default 0.7) | Vector similarity weight (0.0–1.0) |
| `text_weight` | float | | `--text-weight` (default 0.3) | ✅ (default 0.3) | ✅ (default 0.3) | BM25 full-text weight (0.0–1.0) |
| `aggregate` | bool | | `--aggregate` | ✅ | ✅ | Merge similar results (>0.85 similarity) |
| `graph_expand` | bool | | `--graph` | ✅ | ✅ | Traverse graph edges to surface related memories |
| `max_hops` | integer | | `--hops` (default 1) | ✅ (default 1) | ✅ (default 1) | Max hops for graph traversal (requires graph_expand) |

**CLI:**
```bash
nomen search "nostr relay setup" --tier public --limit 5
nomen search "project decisions" --aggregate --vector-weight 0.8
nomen search "Alhovuori" --graph            # traverse graph edges
nomen search "Alhovuori" --graph --hops 2   # 2-hop traversal
```

**MCP tool:** `nomen_search`  
**Context-VM action:** `"search"`

---

### store — Store a memory

Create or update a named memory (kind 31234, replaceable by d-tag/topic). Publishes to relay and stores in local DB. Automatically finds existing memories by topic and adds supersedes tags.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `topic` | string | ✅ | positional | ✅ | ✅ | Topic/namespace (d-tag) |
| `summary` | string | ✅ | `--summary` | ✅ | ✅ | Short summary |
| `detail` | string | | `--detail` | ✅ | ✅ | Full detail text |
| `tier` | string | | `--tier` (default `public`) | ✅ (default `public`) | ✅ (default `public`) | Visibility tier |
| `scope` | string | | | ✅ | ✅ | Scope for group tier |
| `confidence` | float | | `--confidence` (default 0.8) | ✅ (default 0.8) | ✅ (default 0.8) | Confidence score 0.0–1.0 |
| `session_id` | string | | | ✅ | ✅ | Auto-derive tier/scope from session |

**Behavior:**
- Stores in local SurrealDB with embedding generation
- Publishes to Nostr relay (kind 31234)
- Encrypts content (NIP-44) for personal/internal tiers
- Finds existing memory by topic → adds `supersedes` tag + version increment

**CLI:**
```bash
nomen store "relay/config" --summary "Zooid relay on port 7777" --tier public
```

**MCP tool:** `nomen_store`  
**Context-VM action:** `"store"`

---

### delete — Delete a memory

Delete by topic (d-tag) or Nostr event ID. Removes from local DB and publishes NIP-09 deletion event to relay.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `topic` | string | one of | positional | ✅ | ✅ | Topic to delete |
| `id` | string | one of | `--id` | ✅ | ✅ | Event ID to delete |
| `ephemeral` | bool | | `--ephemeral` | ❌ | ❌ | Delete raw messages instead |
| `older_than` | string | | `--older-than` | ❌ | ❌ | Age filter (e.g. `7d`, `24h`). Requires `--ephemeral` |

**Behavior:**
- Removes from local SurrealDB
- Publishes NIP-09 deletion event (kind 5) to relay

**CLI:**
```bash
nomen delete "relay/config"
nomen delete --id abc123
nomen delete --ephemeral --older-than 7d
```

**MCP tool:** `nomen_delete`  
**Context-VM action:** `"delete"`

---

### ingest — Ingest a raw message

Store a raw message for later consolidation into named memories.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `content` | string | ✅ | positional | ✅ | ✅ | Message content |
| `source` | string | | `--source` (default `cli`) | ✅ (default `mcp`) | ✅ (default `nostr`) | Source system |
| `sender` | string | | `--sender` (default `local`) | ✅ (default `unknown`) | ✅ (default `unknown`) | Sender identifier |
| `channel` | string | | `--channel` | ✅ | ✅ | Channel/room name |
| `metadata` | object | | | ✅ | ❌ | Arbitrary metadata |
| `session_id` | string | | | ✅ | ❌ | Auto-derive tier/scope |

**CLI:**
```bash
nomen ingest "decided to use SurrealDB" --source telegram --sender k0 --channel techteam
```

**MCP tool:** `nomen_ingest`  
**Context-VM action:** `"ingest"`

---

### messages — Query raw messages

List ingested raw messages with filters.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `source` | string | | `--source` | ✅ | ✅ | Filter by source |
| `channel` | string | | `--channel` | ✅ | ✅ | Filter by channel |
| `sender` | string | | `--sender` | ✅ | ✅ | Filter by sender |
| `since` | string | | `--since` | ✅ | ✅ | RFC3339 timestamp |
| `limit` | integer | | `--limit` (default 50) | ✅ (default 50) | ✅ (default 50) | Max results |
| `around` | string | | `--around` | ❌ | ❌ | Show messages around a source_id |
| `context` | integer | | `--context` (default 5) | ❌ | ❌ | Context messages for `--around` |

**CLI:**
```bash
nomen messages --source telegram --channel techteam --since 2026-03-01T00:00:00Z
nomen messages --around msg_123 --context 10
```

**MCP tool:** `nomen_messages`  
**Context-VM action:** `"messages"`

---

### entities — List extracted entities

Query the knowledge graph for extracted entities (people, projects, concepts, places, organizations).

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `kind` | string | | `--kind` | ✅ | ✅ | Filter: `person`, `project`, `concept`, `place`, `organization`, `technology` |
| `query` | string | | | ✅ | ❌ | Name substring search |
| `relations` | bool | | `--relations` | ❌ | ❌ | Show typed relationships between entities |

**CLI:**
```bash
nomen entities --kind person
nomen entities --relations    # show typed relationships (works_on, collaborates_with, etc.)
```

**MCP tool:** `nomen_entities`  
**Context-VM action:** `"entities"`

---

### consolidate — Consolidate raw messages into memories

Trigger the sleep-inspired consolidation pipeline: group → extract → merge/dedup → store.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `min_messages` | integer | | `--min-messages` (default 3) | ❌ | ❌ | Min messages to trigger |
| `batch_size` | integer | | `--batch-size` (default 50) | ❌ | ❌ | Max messages per run |
| `dry_run` | bool | | `--dry-run` | ❌ | ❌ | Preview without publishing |
| `older_than` | string | | `--older-than` | ❌ | ❌ | Only consolidate messages older than (e.g. `30m`, `1h`) |
| `tier` | string | | `--tier` | ❌ | ❌ | Filter by tier |
| `channel` | string | | | ✅ | ❌ | Filter by channel |
| `since` | string | | | ✅ | ❌ | Only messages since (RFC3339) |

**CLI:**
```bash
nomen consolidate --dry-run --older-than 1h
nomen consolidate --min-messages 5 --batch-size 100
```

**MCP tool:** `nomen_consolidate`  
**Context-VM action:** `"consolidate"`

---

### cluster — Cluster fusion

Synthesize related memories by namespace prefix into coherent cluster summaries.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ❌ | ❌ |

**Parameters:**

| Parameter | Type | Required | CLI | Description |
|-----------|------|----------|-----|-------------|
| `dry_run` | bool | | `--dry-run` | Preview clusters without storing |
| `prefix` | string | | `--prefix` | Only fuse memories under this prefix (e.g. `user/`) |
| `min_members` | integer | | `--min-members` (default 3) | Minimum memories per cluster |
| `namespace_depth` | integer | | `--namespace-depth` (default 2) | Grouping depth (e.g. 2 → `user/k0`) |

**CLI:**
```bash
nomen cluster --dry-run                    # preview
nomen cluster                              # run synthesis
nomen cluster --prefix user/ --min-members 5
```

---

### groups — Manage groups

Create, list, and manage named groups and their members.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Sub-actions (MCP/CVM):** `list`, `members`, `create`, `add_member`, `remove_member`

**CLI subcommands:**

| Subcommand | Description |
|------------|-------------|
| `nomen group create <id> --name <name>` | Create group |
| `nomen group list` | List all groups |
| `nomen group members <id>` | Show members |
| `nomen group add-member <id> <npub>` | Add member |
| `nomen group remove-member <id> <npub>` | Remove member |

**CLI create parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `id` | string | ✅ | Dot-separated hierarchy (e.g. `atlantislabs.engineering`) |
| `--name` | string | ✅ | Human-readable name |
| `--members` | string[] | | Comma-separated initial member npubs |
| `--nostr-group` | string | | NIP-29 group id mapping |
| `--relay` | string | | Relay URL for this group |

**MCP tool:** `nomen_groups` — `action` param selects sub-action  
**Context-VM action:** `"groups"` — `action` param selects sub-action (default: `list`)

---

### send — Send a message

Send a message to a recipient via Nostr DM, group message, or public note.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `content` | string | ✅ | positional | ✅ | ✅ | Message body |
| `recipient`/`to` | string | ✅ | `--to` | `recipient` | `recipient` | `npub1...` for DM, `group:<id>` for group, `public` for broadcast |
| `channel` | string | | `--channel` | ✅ | ✅ | Delivery channel (default: `nostr`) |
| `metadata` | object | | | ✅ | ✅ | Platform-specific extras |

**CLI:**
```bash
nomen send "relay is down" --to npub1abc...
nomen send "update" --to group:techteam
nomen send "announcement" --to public
```

**MCP tool:** `nomen_send`  
**Context-VM action:** `"send"`

---

### list — List memories

List stored memories from local DB with optional filters and statistics.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `named` | bool | | `--named` | ❌ | ❌ | Only named (consolidated) memories |
| `ephemeral` | bool | | `--ephemeral` | ❌ | ❌ | Only ephemeral (pending consolidation) |
| `stats` | bool | | `--stats` | ✅ | ✅ | Include/show statistics |
| `tier` | string | | | ✅ | ✅ | Filter by tier |
| `limit` | integer | | | ✅ (default 100) | ✅ (default 100) | Max results |

**CLI:**
```bash
nomen list                    # All memories
nomen list --named            # Only consolidated memories
nomen list --ephemeral        # Only pending consolidation
nomen list --stats            # Statistics
```

**MCP tool:** `nomen_list`  
**Context-VM action:** `"list"`

---

### sync — Sync relay → local DB

Fetch memory events from Nostr relay and upsert into local SurrealDB.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:** None (uses configured relay and keys).

**CLI:**
```bash
nomen sync
```

**MCP tool:** `nomen_sync`  
**Context-VM action:** `"sync"`

---

### embed — Generate missing embeddings

Generate vector embeddings for memories that don't have them yet.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `limit` | integer | | `--limit` (default 100) | ✅ (default 100) | ✅ (default 100) | Max memories to embed |

**CLI:**
```bash
nomen embed --limit 100
```

**MCP tool:** `nomen_embed`  
**Context-VM action:** `"embed"`

---

### prune — Remove old/unused memories

Prune low-confidence and unaccessed memories, plus consolidated raw messages.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `days` | integer | | `--days` (default 90) | ✅ (default 90) | ✅ (default 90) | Delete items older than N days |
| `dry_run` | bool | | `--dry-run` | ✅ | ✅ | Preview without deleting |

**CLI:**
```bash
nomen prune --days 90 --dry-run
nomen prune --days 30
```

**MCP tool:** `nomen_prune`  
**Context-VM action:** `"prune"`

---

## CLI-Only Commands

These are interactive setup/diagnostic tools that don't need API equivalents:

### init — Interactive setup wizard

```bash
nomen init
nomen init --force --non-interactive   # requires NOMEN_NSEC env var
```

### doctor — Validate config and connectivity

```bash
nomen doctor
```

### config — Show config status

```bash
nomen config
```

---

## Context-VM Protocol

Nostr-native request/response for agents without local MCP access.

### Event Kinds

| Kind | Direction | Description |
|------|-----------|-------------|
| **21900** | Agent → Nomen | Request (ephemeral) |
| **21901** | Nomen → Agent | Response (ephemeral) |

### Request Format

Kind 21900 event with NIP-44 encrypted JSON content:

```json
{
  "action": "search",
  "params": {
    "query": "relay configuration",
    "limit": 5
  }
}
```

**Tags:**
- `["p", "<nomen_npub_hex>"]` — target Nomen instance
- `["t", "nomen-request"]` — protocol tag
- `["expiration", "<unix_timestamp>"]` — request TTL (typically now + 60s)

### Response Format

Kind 21901 event with NIP-44 encrypted JSON content:

```json
{
  "result": { "count": 3, "results": [...] }
}
```

Or on error:

```json
{
  "error": "query parameter is required"
}
```

**Tags:**
- `["p", "<requester_npub_hex>"]` — route back to requester
- `["e", "<request_event_id>"]` — correlate with request
- `["t", "nomen-response"]` — protocol tag
- `["expiration", "<unix_timestamp>"]` — response TTL

### Available Actions

`search`, `store`, `delete`, `ingest`, `messages`, `entities`, `consolidate`, `groups`, `send`, `list`, `sync`, `embed`, `prune`

### Security

- **Allowlist:** Only npubs passed via `--allowed-npubs` can issue requests
- **Rate limiting:** 30 requests/minute per npub (configurable)
- **Encryption:** All payloads NIP-44 encrypted
- **Expiration:** Expired requests are silently dropped

---

## Feature Parity Matrix

| Operation | CLI | MCP | Context-VM | Notes |
|-----------|-----|-----|------------|-------|
| search | ✅ | ✅ | ✅ | All have weight/aggregate/graph_expand/session_id |
| store | ✅ | ✅ | ✅ | All publish to relay + local DB |
| delete | ✅ | ✅ | ✅ | All publish NIP-09 to relay |
| ingest | ✅ | ✅ | ✅ | |
| messages | ✅ | ✅ | ✅ | CLI has around/context |
| entities | ✅ | ✅ | ✅ | MCP has query filter; CLI has --relations |
| cluster | ✅ | — | — | CLI-only (batch operation) |
| consolidate | ✅ | ✅ | ✅ | CLI has batch/dry-run controls |
| groups | ✅ | ✅ | ✅ | Full CRUD on all interfaces |
| send | ✅ | ✅ | ✅ | |
| list | ✅ | ✅ | ✅ | |
| sync | ✅ | ✅ | ✅ | |
| embed | ✅ | ✅ | ✅ | |
| prune | ✅ | ✅ | ✅ | |
| init | ✅ | — | — | CLI-only (interactive setup) |
| doctor | ✅ | — | — | CLI-only (diagnostics) |
| config | ✅ | — | — | CLI-only (info display) |

---

## Architecture

All interfaces use the `Nomen` struct from `lib.rs` as the single API layer:

```
┌─────────────────────────────────────────────────────┐
│  Interfaces (thin adapters — parse input, format output) │
│  CLI (main.rs) │ MCP (mcp.rs) │ CVM (contextvm.rs)      │
├─────────────────────────────────────────────────────┤
│  Nomen struct (lib.rs) — single API                      │
│  store, search, delete, ingest, consolidate, sync,       │
│  embed, prune, list, entities, send, groups              │
├─────────────────────────────────────────────────────┤
│  Modules (db, search, relay, embed, consolidate,         │
│  entities, groups, ingest, send, session, signer)        │
└─────────────────────────────────────────────────────┘
```

## Global CLI Options

```bash
nomen [OPTIONS] <COMMAND>

Options:
  --relay <URL>        Override relay URL
  --nsec <KEY>         Override nsec (repeatable)
  --config <PATH>      Override config file path
  -v, --verbose        Verbose output
```

## Configuration

Default config: `~/.config/nomen/config.toml`

See `nomen init` for guided setup or `nomen doctor` to validate.
