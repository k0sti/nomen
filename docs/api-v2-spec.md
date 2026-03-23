# Nomen API v2 Specification

**Status:** Draft  
**Date:** 2026-03-12  
**Project:** [[Nomen]]

## Design Principle

The canonical operation model is transport-independent. HTTP, MCP, ContextVM, and socket are transport adapters or projections of the same operations. All canonical actions route through `api::dispatch()`. Individual transports may expose additional transport-specific capabilities (e.g., socket provides `subscribe`/`unsubscribe` for push event management) that are not part of the canonical operation model.

## Field Model

### First-class fields

| Field        | Type   | Description                                                                          |
| ------------ | ------ | ------------------------------------------------------------------------------------ |
| `visibility` | enum   | `public \| group \| circle \| personal \| internal`                                  |
| `scope`      | string | Stable durable boundary (group id, pubkey hex, circle hash, or empty for public)     |
| `channel`    | string | Concrete provider/container identity for raw messages (e.g. `telegram:-100382:9225`) |
| `topic`      | string | Durable semantic memory identity (e.g. `project/nomen/api-v2`)                       |
| `metadata`   | object | Optional host/container extras                                                       |

### Compatibility fields (deprecated, fallback only)

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | Legacy host compatibility hint. If both `scope` and `session_id` are present, `scope` wins. |
| `tier` | string | Legacy shorthand (e.g. `group:techteam`). Use `visibility` + `scope` instead. |

### Visibility + Scope combinations

| Visibility | Scope value                        | Example           |
| ---------- | ---------------------------------- | ----------------- |
| `public`   | `""` (empty)                       | General knowledge |
| `group`    | NIP-29 group id                    | `techteam`        |
| `circle`   | Deterministic participant-set hash | `a3f8b2c1...`     |
| `personal` | Hex pubkey                         | `d29fe7c1...`     |
| `internal` | Hex pubkey                         | `d29fe7c1...`     |

### Validation rules

- `visibility=group` requires non-empty `scope`
- `visibility=circle` requires non-empty `scope`
- `visibility=personal` requires `scope` to be a valid hex pubkey (may be auto-filled from auth context)
- `visibility=internal` requires `scope` to be the agent's own hex pubkey (may be auto-filled)
- `visibility=public` ignores `scope` (treated as empty)

---

## Response envelope

All responses use a structured JSON envelope:

### Success

```json
{
  "ok": true,
  "result": { ... },
  "meta": {
    "version": "v2",
    "request_id": "optional-correlation-id"
  }
}
```

### Error

```json
{
  "ok": false,
  "error": {
    "code": "invalid_scope",
    "message": "scope is required when visibility=group"
  },
  "meta": {
    "version": "v2"
  }
}
```

### Standard error codes

| Code | Meaning |
|------|---------|
| `invalid_params` | Missing or invalid required parameters |
| `invalid_scope` | Scope validation failed for given visibility |
| `not_found` | Memory/entity not found |
| `unauthorized` | ACL rejection |
| `rate_limited` | Too many requests |
| `internal_error` | Unexpected server error |
| `unknown_action` | Action name not recognized |

---

## HTTP Transport

The HTTP server exposes the canonical dispatch endpoint for remote agents and integrations.

### Canonical dispatch endpoint

**`POST /memory/api/dispatch`**

Request body:

```json
{
  "action": "memory.search",
  "params": {
    "query": "relay configuration",
    "limit": 10
  }
}
```

Response body: the canonical response envelope defined above.

```json
{
  "ok": true,
  "result": {
    "count": 3,
    "results": [...]
  },
  "meta": {
    "version": "v2"
  }
}
```

### Utility endpoints

These endpoints live outside the dispatch model and serve operational needs:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/stats` | GET | Memory statistics |
| `/config` | GET | Current configuration |
| `/config/reload` | POST | Reload configuration |

---

## Operations

## 1. Memory domain

### `memory.search`

Search memories using hybrid semantic + full-text search with optional graph expansion.

**Request:**

```json
{
  "action": "memory.search",
  "params": {
    "query": "contextvm bridge design",
    "visibility": "group",
    "scope": "techteam",
    "limit": 10,
    "retrieval": {
      "vector_weight": 0.7,
      "text_weight": 0.3,
      "aggregate": false,
      "graph_expand": true,
      "max_hops": 1
    }
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `query` | string | ✅ | — | Search query |
| `visibility` | string | — | — | Filter by visibility tier |
| `scope` | string | — | — | Filter by scope |
| `limit` | integer | — | 10 | Max results |
| `retrieval.vector_weight` | float | — | 0.7 | Vector similarity weight (0.0–1.0) |
| `retrieval.text_weight` | float | — | 0.3 | BM25 full-text weight (0.0–1.0) |
| `retrieval.aggregate` | boolean | — | false | Merge similar results (>0.85 similarity) |
| `retrieval.graph_expand` | boolean | — | false | Traverse graph edges from hits |
| `retrieval.max_hops` | integer | — | 1 | Max hops for graph traversal |
| `session_id` | string | — | — | Legacy: auto-derive visibility/scope |

**Response result:**

```json
{
  "count": 3,
  "results": [
    {
      "topic": "project/nomen/api-v2",
      "summary": "ContextVM is canonical; MCP wraps it",
      "detail": "...",
      "visibility": "group",
      "scope": "techteam",
      "confidence": 0.93,
      "version": 2,
      "match_type": "hybrid",
      "graph_edge": null,
      "contradicts": false,
      "created_at": "2026-03-12T10:30:00Z"
    }
  ]
}
```

---

### `memory.put`

Create or replace a named memory. Publishes to relay and stores locally. Automatically supersedes existing memory with the same topic.

**Request:**

```json
{
  "action": "memory.put",
  "params": {
    "topic": "project/nomen/api-v2",
    "summary": "ContextVM is canonical; MCP wraps it",
    "detail": "Remote agents should use the ContextVM-defined operation model.",
    "visibility": "group",
    "scope": "techteam",
    "confidence": 0.93,
    "metadata": {
      "host_system": "openclaw"
    }
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `topic` | string | ✅ | — | Topic/namespace for the memory |
| `summary` | string | ✅ | — | Short summary |
| `detail` | string | — | `""` | Full detail text |
| `visibility` | string | — | `public` | Visibility tier |
| `scope` | string | — | `""` | Scope (required if visibility needs it) |
| `confidence` | float | — | 0.8 | Confidence score 0.0–1.0 |
| `metadata` | object | — | — | Arbitrary metadata |
| `session_id` | string | — | — | Legacy: auto-derive visibility/scope |

**Response result:**

```json
{
  "d_tag": "group:techteam:project/nomen/api-v2",
  "topic": "project/nomen/api-v2",
  "version": 2,
  "superseded": "abc123hex..."
}
```

---

### `memory.get`

Retrieve a single memory by topic or d_tag. Deterministic fetch, not search.

**Request:**

```json
{
  "action": "memory.get",
  "params": {
    "topic": "project/nomen/api-v2",
    "visibility": "group",
    "scope": "techteam"
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `topic` | string | one of | — | Topic to retrieve |
| `d_tag` | string | one of | — | Direct d_tag lookup |
| `visibility` | string | — | — | For topic → d_tag resolution |
| `scope` | string | — | — | For topic → d_tag resolution |

**Response result:**

```json
{
  "topic": "project/nomen/api-v2",
  "summary": "ContextVM is canonical; MCP wraps it",
  "detail": "...",
  "visibility": "group",
  "scope": "techteam",
  "confidence": 0.93,
  "version": 2,
  "created_at": "2026-03-12T10:30:00Z",
  "d_tag": "group:techteam:project/nomen/api-v2"
}
```

Returns `null` result if not found (with `ok: true`).

---

### `memory.list`

List memories from local DB with optional filters.

**Request:**

```json
{
  "action": "memory.list",
  "params": {
    "visibility": "group",
    "scope": "techteam",
    "limit": 100,
    "stats": true
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `visibility` | string | — | — | Filter by visibility |
| `scope` | string | — | — | Filter by scope |
| `limit` | integer | — | 100 | Max results |
| `stats` | boolean | — | false | Include memory statistics |

**Response result:**

```json
{
  "count": 42,
  "memories": [ ... ],
  "stats": {
    "total": 42,
    "named": 38,
    "pending": 15
  }
}
```

---

### `memory.delete`

Delete a memory by topic, d_tag, or event ID. Removes from local DB and publishes NIP-09 deletion to relay.

**Request:**

```json
{
  "action": "memory.delete",
  "params": {
    "topic": "project/nomen/api-v2",
    "visibility": "group",
    "scope": "techteam"
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `topic` | string | one of | — | Topic to delete |
| `d_tag` | string | one of | — | Direct d_tag lookup |
| `id` | string | one of | — | Nostr event ID |

**Response result:**

```json
{
  "deleted": true,
  "d_tag": "group:techteam:project/nomen/api-v2",
  "relay_deleted": true
}
```

---

## 2. Message domain

### `message.ingest`

Ingest a raw message for later consolidation.

**Request:**

```json
{
  "action": "message.ingest",
  "params": {
    "source": "telegram",
    "channel": "telegram:-1003821690204:9225",
    "sender": "kosti",
    "content": "Scrap old MCP, let's make new",
    "metadata": {
      "message_id": "9276"
    }
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `content` | string | ✅ | — | Message content |
| `source` | string | — | `"unknown"` | Source system |
| `sender` | string | — | `"unknown"` | Sender identifier |
| `channel` | string | — | — | Channel/room identity |
| `source_id` | string | — | — | Source-specific message ID |
| `metadata` | object | — | — | Arbitrary metadata |

**Response result:**

```json
{
  "id": "raw_message:abc123",
  "source": "telegram",
  "channel": "telegram:-1003821690204:9225"
}
```

---

### `message.list`

Query raw messages with filters.

**Request:**

```json
{
  "action": "message.list",
  "params": {
    "channel": "telegram:-1003821690204:9225",
    "since": "2026-03-12T00:00:00Z",
    "limit": 50
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `source` | string | — | — | Filter by source |
| `channel` | string | — | — | Filter by channel |
| `sender` | string | — | — | Filter by sender |
| `since` | string | — | — | RFC3339 timestamp |
| `limit` | integer | — | 50 | Max results |

**Response result:**

```json
{
  "count": 12,
  "messages": [
    {
      "source": "telegram",
      "sender": "kosti",
      "channel": "telegram:-1003821690204:9225",
      "content": "...",
      "consolidated": false,
      "created_at": "2026-03-12T10:30:00Z"
    }
  ]
}
```

---

### `message.context`

Get messages surrounding a specific message (context window).

**Request:**

```json
{
  "action": "message.context",
  "params": {
    "source_id": "msg_123",
    "before": 5,
    "after": 5
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `source_id` | string | ✅ | — | Source message ID to center on |
| `before` | integer | — | 5 | Messages before |
| `after` | integer | — | 5 | Messages after |

---

## 3. Maintenance domain

### `memory.consolidate`

Trigger consolidation pipeline: group → extract → merge/dedup → store.

**Request:**

```json
{
  "action": "memory.consolidate",
  "params": {
    "channel": "telegram:-1003821690204:9225",
    "since": "2026-03-12T00:00:00Z",
    "min_messages": 3,
    "batch_size": 50,
    "dry_run": false
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `channel` | string | — | — | Filter by channel |
| `since` | string | — | — | Only messages since (RFC3339) |
| `min_messages` | integer | — | 3 | Minimum messages to trigger |
| `batch_size` | integer | — | 50 | Max messages per run |
| `dry_run` | boolean | — | false | Preview without publishing |
| `older_than` | string | — | — | Duration filter (e.g. `30m`, `1h`) |

**Response result:**

```json
{
  "messages_processed": 15,
  "memories_created": 3,
  "events_published": 3,
  "channels": ["telegram:-1003821690204:9225"]
}
```

---

### `memory.cluster`

Synthesize related memories by namespace prefix.

**Request:**

```json
{
  "action": "memory.cluster",
  "params": {
    "prefix": "project/nomen/",
    "min_members": 3,
    "namespace_depth": 2,
    "dry_run": true
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `prefix` | string | — | — | Only fuse under this prefix |
| `min_members` | integer | — | 3 | Min memories per cluster |
| `namespace_depth` | integer | — | 2 | Grouping depth |
| `dry_run` | boolean | — | false | Preview without storing |

---

### `memory.sync`

Sync memories from relay to local DB.

**Request:**

```json
{
  "action": "memory.sync",
  "params": {}
}
```

**Response result:**

```json
{
  "stored": 12,
  "skipped": 45,
  "errors": 0
}
```

---

### `memory.embed`

Generate embeddings for memories that lack them.

**Request:**

```json
{
  "action": "memory.embed",
  "params": {
    "limit": 100
  }
}
```

---

### `memory.prune`

Prune old/unused memories and consolidated raw messages.

**Request:**

```json
{
  "action": "memory.prune",
  "params": {
    "days": 90,
    "dry_run": true
  }
}
```

---

## 4. Group domain

### `group.list`

```json
{ "action": "group.list", "params": {} }
```

### `group.members`

```json
{ "action": "group.members", "params": { "id": "techteam" } }
```

### `group.create`

```json
{
  "action": "group.create",
  "params": {
    "id": "atlantislabs.engineering",
    "name": "Engineering",
    "members": ["npub1abc..."],
    "nostr_group": "techteam",
    "relay": "wss://zooid.atlantislabs.space"
  }
}
```

### `group.add_member`

```json
{ "action": "group.add_member", "params": { "id": "techteam", "npub": "npub1abc..." } }
```

### `group.remove_member`

```json
{ "action": "group.remove_member", "params": { "id": "techteam", "npub": "npub1abc..." } }
```

---

## ContextVM transport mapping

ContextVM is one of several transport adapters. It wraps the canonical API over Nostr:

- Request: kind 25910 ephemeral event, NIP-44/NIP-59 encrypted
- Response: kind 25910 response, encrypted to requester
- Action field maps directly to canonical operation names
- Params field maps directly to canonical parameter objects
- Response envelope is the structured JSON envelope defined above

The ContextVM server should:
1. Decrypt incoming request
2. Parse `action` + `params`
3. Dispatch to shared canonical operation layer
4. Wrap result in response envelope
5. Encrypt and send response

---

## MCP tool mapping

Each canonical operation maps to an MCP tool with the same name and argument schema:

| Canonical action | MCP tool name | Notes |
|-----------------|---------------|-------|
| `memory.search` | `memory_search` | Underscore for MCP compat |
| `memory.put` | `memory_put` | |
| `memory.get` | `memory_get` | |
| `memory.list` | `memory_list` | |
| `memory.delete` | `memory_delete` | |
| `message.ingest` | `message_ingest` | |
| `message.list` | `message_list` | |
| `message.context` | `message_context` | |
| `message.send` | `message_send` | |
| `entity.list` | `entity_list` | |
| `entity.relationships` | `entity_relationships` | |
| `memory.consolidate` | `memory_consolidate` | |
| `memory.cluster` | `memory_cluster` | |
| `memory.sync` | `memory_sync` | |
| `memory.embed` | `memory_embed` | |
| `memory.prune` | `memory_prune` | |
| `group.list` | `group_list` | |
| `group.members` | `group_members` | |
| `group.create` | `group_create` | |
| `group.add_member` | `group_add_member` | |
| `group.remove_member` | `group_remove_member` | |

MCP tools use the same parameter names and types. The MCP response wraps the structured result as MCP content text (JSON-serialized).

---

## Implementation architecture

```
src/
├── api/
│   ├── mod.rs          — re-exports
│   ├── types.rs        — canonical request/response structs
│   ├── errors.rs       — structured error model
│   ├── dispatch.rs     — action name → handler routing
│   └── operations/
│       ├── memory.rs   — search, put, get, list, delete
│       ├── message.rs  — ingest, list, context
│       ├── maintenance.rs — consolidate, cluster, sync, embed, prune
│       └── group.rs    — list, members, create, add/remove member
├── http.rs             — HTTP transport (calls api::dispatch)
├── mcp.rs              — MCP transport (calls api::dispatch)
├── cvm.rs              — ContextVM transport (calls api::dispatch)
├── socket.rs           — Socket transport (calls api::dispatch)
└── ... (existing modules unchanged)
```

All four transport adapters (`http.rs`, `mcp.rs`, `cvm.rs`, `socket.rs`) follow the same pattern for canonical operations:
1. Parse transport-specific framing
2. Extract `action` + `params`
3. Call `api::dispatch()`
4. Format transport-specific response

Socket additionally handles `subscribe` and `unsubscribe` as transport-specific capabilities for push event management. These are not canonical API actions and are handled directly by the socket layer before reaching dispatch.
