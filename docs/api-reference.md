# Nomen API Reference

**Version:** 0.3  
**Date:** 2026-03-11

Complete reference for all Nomen operations across CLI, MCP (Model Context Protocol), and Context-VM (Nostr-native) interfaces.

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
| `session_id` | string | | | ✅ | ❌ | Auto-derive tier/scope from session |
| `vector_weight` | float | | `--vector-weight` (default 0.7) | ❌ | ❌ | Vector similarity weight (0.0–1.0) |
| `text_weight` | float | | `--text-weight` (default 0.3) | ❌ | ❌ | BM25 full-text weight (0.0–1.0) |
| `aggregate` | bool | | `--aggregate` | ❌ | ❌ | Merge similar results (>0.85 similarity) |

**CLI:**
```bash
nomen search "nostr relay setup" --tier public --limit 5
nomen search "project decisions" --aggregate --vector-weight 0.8
```

**MCP tool:** `nomen_search`  
**Context-VM action:** `"search"`

---

### store — Store a memory

Create or update a named memory (kind 31234, replaceable by d-tag/topic).

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
| `scope` | string | | | ✅ | ❌ | Scope for group tier |
| `confidence` | float | | `--confidence` (default 0.8) | ✅ (default 0.8) | ✅ (default 0.8) | Confidence score 0.0–1.0 |
| `session_id` | string | | | ✅ | ❌ | Auto-derive tier/scope from session |

**CLI:**
```bash
nomen store "relay/config" --summary "Zooid relay on port 7777" --tier public
```

**MCP tool:** `nomen_store`  
**Context-VM action:** `"store"`

---

### delete — Delete a memory

Delete by topic (d-tag) or Nostr event ID. Also supports ephemeral message cleanup.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ❌ |

**Parameters:**

| Parameter | Type | Required | CLI | MCP | CVM | Description |
|-----------|------|----------|-----|-----|-----|-------------|
| `topic` | string | one of | positional | ✅ | — | Topic to delete |
| `id` | string | one of | `--id` | ✅ | — | Event ID to delete |
| `ephemeral` | bool | | `--ephemeral` | ❌ | — | Delete raw messages instead |
| `older_than` | string | | `--older-than` | ❌ | — | Age filter (e.g. `7d`, `24h`). Requires `--ephemeral` |

**CLI:**
```bash
nomen delete "relay/config"
nomen delete --id abc123
nomen delete --ephemeral --older-than 7d
```

**MCP tool:** `nomen_delete`  
**Context-VM:** Not implemented.

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
| `kind` | string | | `--kind` | ✅ | ✅ | Filter: `person`, `project`, `concept`, `place`, `organization` |
| `query` | string | | | ✅ | ❌ | Name substring search |

**CLI:**
```bash
nomen entities --kind person
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

### groups — Manage groups

Create, list, and manage named groups and their members.

| | CLI | MCP | Context-VM |
|---|---|---|---|
| **Available** | ✅ | ✅ | ✅ |

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

**MCP tool:** `nomen_groups` — uses `action` parameter: `list`, `members`, `create`, `add_member`, `remove_member`  
**Context-VM action:** `"groups"` — returns group list (read-only, no sub-actions)

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

## CLI-Only Operations

These commands are for local administration and don't have MCP/Context-VM equivalents.

### list — List memory events

Fetch memories directly from relay.

```bash
nomen list                    # All memories
nomen list --named            # Only named (consolidated) memories
nomen list --ephemeral        # Only ephemeral (pending consolidation)
nomen list --stats            # Consolidation statistics
```

### sync — Sync relay → local DB

```bash
nomen sync
```

### embed — Generate missing embeddings

```bash
nomen embed --limit 100
```

### prune — Remove old/unused memories

```bash
nomen prune --days 90 --dry-run
nomen prune --days 30
```

### config — Show config status

```bash
nomen config
```

### init — Interactive setup wizard

```bash
nomen init
nomen init --force --non-interactive   # requires NOMEN_NSEC env var
```

### doctor — Validate config and connectivity

```bash
nomen doctor
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

`search`, `store`, `ingest`, `messages`, `entities`, `consolidate`, `groups`, `send`

### Security

- **Allowlist:** Only npubs passed via `--allowed-npubs` can issue requests
- **Rate limiting:** 30 requests/minute per npub (configurable)
- **Encryption:** All payloads NIP-44 encrypted
- **Expiration:** Expired requests are silently dropped

---

## Feature Parity Matrix

| Operation | CLI | MCP | Context-VM | Notes |
|-----------|-----|-----|------------|-------|
| search | ✅ | ✅ | ✅ | CLI has weight/aggregate controls |
| store | ✅ | ✅ | ✅ | MCP has session_id |
| delete | ✅ | ✅ | ❌ | **Gap:** missing from Context-VM |
| ingest | ✅ | ✅ | ✅ | |
| messages | ✅ | ✅ | ✅ | CLI has around/context |
| entities | ✅ | ✅ | ✅ | MCP has query filter |
| consolidate | ✅ | ✅ | ✅ | CLI has batch/dry-run controls |
| groups | ✅ | ✅ | ✅ | CVM is read-only (list only) |
| send | ✅ | ✅ | ✅ | |
| list | ✅ | ❌ | ❌ | Admin/operational |
| sync | ✅ | ❌ | ❌ | Admin/operational |
| embed | ✅ | ❌ | ❌ | Admin/operational |
| prune | ✅ | ❌ | ❌ | Admin/operational |
| config | ✅ | ❌ | ❌ | Admin/operational |
| init | ✅ | ❌ | ❌ | Setup |
| doctor | ✅ | ❌ | ❌ | Setup |

### Known Gaps

1. **Context-VM missing `delete`** — agents can store but not remove memories via Nostr
2. **Context-VM `groups` is read-only** — cannot create/modify groups, only list
3. **Context-VM lacks `session_id`** — no automatic tier/scope derivation
4. **MCP/CVM lack search tuning** — `vector_weight`, `text_weight`, `aggregate` are CLI-only
5. **MCP/CVM lack consolidation controls** — `min_messages`, `batch_size`, `dry_run`, `older_than`, `tier` are CLI-only

---

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
