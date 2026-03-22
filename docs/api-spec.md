# Nomen API v2 Specification

**Status:** Draft  
**Date:** 2026-03-12  
**Project:** [[Nomen]]

## Design Principle

The canonical operation model is transport-independent. HTTP, MCP, ContextVM, and socket are transport adapters or projections of the same operations. All 28 canonical actions route through `api::dispatch()`. Individual transports may expose additional transport-specific capabilities (e.g., socket provides `subscribe`/`unsubscribe` for push event management) that are not part of the canonical operation model.

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

### `memory.get_batch`

Retrieve multiple memories by d_tag in a single call. Useful for room context injection where group, topic, and participant context are needed together.

**Request:**

```json
{
  "action": "memory.get_batch",
  "params": {
    "d_tags": [
      "group:techteam:room",
      "group:techteam:room/deploys",
      "personal:d29f...96d7:user-profile"
    ]
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `d_tags` | string[] | yes | — | Array of d_tags to fetch |

**Response result:**

```json
{
  "count": 2,
  "results": [
    { "topic": "room", "summary": "...", "d_tag": "group:techteam:room", ... },
    { "topic": "room/deploys", "summary": "...", "d_tag": "group:techteam:room/deploys", ... }
  ],
  "by_d_tag": {
    "group:techteam:room": { "topic": "room", "summary": "...", ... },
    "group:techteam:room/deploys": { "topic": "room/deploys", "summary": "...", ... }
  }
}
```

Missing d_tags are silently omitted from results. The `by_d_tag` map enables keyed access without iterating `results`.

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

### `memory.consolidate_prepare`

Two-phase consolidation: prepare batches for external agent extraction (stages 1-2).

**Request:**

```json
{
  "action": "memory.consolidate_prepare",
  "params": {
    "min_messages": 3,
    "batch_size": 50,
    "older_than": "30m",
    "ttl_minutes": 30
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `min_messages` | integer | — | 3 | Minimum messages to trigger |
| `batch_size` | integer | — | 50 | Max messages per run |
| `older_than` | string | — | — | Duration filter (e.g. `30m`, `1h`) |
| `ttl_minutes` | integer | — | 30 | Session TTL in minutes |

**Response result:**

```json
{
  "session_id": "cons_01abc...",
  "expires_at": "2026-03-12T11:00:00Z",
  "batch_count": 3,
  "message_count": 15,
  "batches": [
    {
      "batch_id": "b_0",
      "scope": "personal",
      "visibility": "personal",
      "message_count": 5,
      "time_range": { "start": "...", "end": "..." },
      "messages": [...]
    }
  ]
}
```

---

### `memory.consolidate_commit`

Two-phase consolidation: commit agent-extracted memories (stages 4-6).

**Request:**

```json
{
  "action": "memory.consolidate_commit",
  "params": {
    "session_id": "cons_01abc...",
    "extractions": [
      {
        "batch_id": "b_0",
        "memories": [
          {
            "topic": "project/nomen/api-v2",
            "summary": "API v2 uses canonical dispatch",
            "detail": "...",
            "importance": 7,
            "entities": ["nomen", "api-v2"]
          }
        ]
      }
    ]
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `session_id` | string | ✅ | — | Session ID from prepare |
| `extractions` | array | ✅ | — | Array of batch extractions |

**Response result:**

```json
{
  "session_id": "cons_01abc...",
  "memories_created": 3,
  "memories_merged": 1,
  "memories_deduped": 0,
  "messages_consolidated": 15,
  "events_published": 3,
  "events_deleted": 0
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

## 5. Entity domain

### `entity.list`

List extracted entities, optionally filtered by kind.

```json
{ "action": "entity.list", "params": { "kind": "person" } }
```

### `entity.relationships`

List entity relationships, optionally filtered by entity name.

```json
{ "action": "entity.relationships", "params": { "entity": "nomen" } }
```

---

## 6. Room domain

Room operations map provider-specific chat/group IDs to Nomen memory d-tags via the `provider_binding` table.

### `room.resolve`

Resolve a provider ID to its bound memory d-tags.

**Request:**

```json
{
  "action": "room.resolve",
  "params": {
    "provider_id": "telegram:-1003821690204"
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `provider_id` | string | ✅ | — | Provider-specific room/chat ID |

**Response result:**

```json
{
  "provider_id": "telegram:-1003821690204",
  "d_tags": ["group:techteam:room", "group:techteam:room/deploys"]
}
```

---

### `room.bind`

Bind a provider ID to a memory d-tag.

**Request:**

```json
{
  "action": "room.bind",
  "params": {
    "provider_id": "telegram:-1003821690204",
    "d_tag": "group:techteam:room"
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `provider_id` | string | ✅ | — | Provider-specific room/chat ID |
| `d_tag` | string | ✅ | — | Memory d-tag to bind |

**Response result:**

```json
{
  "bound": true,
  "provider_id": "telegram:-1003821690204",
  "d_tag": "group:techteam:room"
}
```

---

### `room.unbind`

Unbind a provider ID from a memory d-tag.

**Request:**

```json
{
  "action": "room.unbind",
  "params": {
    "provider_id": "telegram:-1003821690204",
    "d_tag": "group:techteam:room"
  }
}
```

**Parameters:**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `provider_id` | string | ✅ | — | Provider-specific room/chat ID |
| `d_tag` | string | ✅ | — | Memory d-tag to unbind |

**Response result:**

```json
{
  "unbound": true,
  "provider_id": "telegram:-1003821690204",
  "d_tag": "group:techteam:room"
}
```

---

## Socket transport

The socket server provides a persistent, bidirectional connection over Unix domain sockets for local agents and integrations. It is the only transport that supports push events via subscriptions.

### Connection

Connect to the Unix domain socket at the configured path (default: `$XDG_RUNTIME_DIR/nomen/nomen.sock`).

Configuration (`config.toml`):

```toml
[socket]
enabled = true
path = "/run/user/1000/nomen/nomen.sock"   # default: $XDG_RUNTIME_DIR/nomen/nomen.sock
max_connections = 64                         # default
max_frame_size = 16777216                    # 16 MB default
```

Socket permissions are set to `0660`.

### Wire format

All frames use **length-delimited JSON**: a 4-byte big-endian length prefix followed by a JSON payload.

```text
┌──────────┬──────────────────────┐
│ len: u32 │ payload: JSON bytes  │
│ (BE)     │ (len bytes)          │
└──────────┴──────────────────────┘
```

### Frame types

Frames are distinguished by field presence (untagged):

| Frame | Discriminator | Direction |
|-------|--------------|-----------|
| **Request** | Has `action` field | Agent → Nomen |
| **Response** | Has `ok` + `id` fields | Nomen → Agent |
| **Event** | Has `event` field | Nomen → Agent (push) |

#### Request

```json
{
  "id": "01JRQX...",
  "action": "memory.search",
  "params": { "query": "relay config", "limit": 10 }
}
```

- `id` — Correlation ID (ULID recommended, UUID accepted).
- `action` — Canonical action name (same as HTTP/MCP/CVM) or transport-specific action (`subscribe`, `unsubscribe`).
- `params` — Action parameters. Defaults to `{}` if omitted.

#### Response

```json
{
  "id": "01JRQX...",
  "ok": true,
  "result": { "count": 3, "results": [...] },
  "meta": { "version": "v2" }
}
```

Error response:

```json
{
  "id": "01JRQX...",
  "ok": false,
  "error": { "code": "not_found", "message": "No memory with that topic" },
  "meta": { "version": "v2" }
}
```

#### Event (push)

```json
{
  "event": "memory.updated",
  "ts": 1741860000,
  "data": { "topic": "project/nomen" }
}
```

Events are only delivered to connections that have an active subscription matching the event type.

### Canonical dispatch

All actions except `subscribe` and `unsubscribe` are routed through `api::dispatch()`, producing the same request/response envelope as HTTP, MCP, and CVM. The socket is a trusted local transport — all requests receive owner-level access.

### Transport-specific actions

#### `subscribe`

Register to receive push events on this connection.

```json
{
  "id": "sub1",
  "action": "subscribe",
  "params": { "events": ["memory.updated", "memory.deleted"] }
}
```

Use `"events": ["*"]` to subscribe to all event types.

Response:

```json
{
  "id": "sub1",
  "ok": true,
  "result": { "subscribed": ["memory.updated", "memory.deleted"] }
}
```

#### `unsubscribe`

Remove event subscriptions for this connection.

```json
{
  "id": "unsub1",
  "action": "unsubscribe",
  "params": { "events": ["memory.deleted"] }
}
```

### Built-in events

| Event type | Emitted when | Data |
|-----------|-------------|------|
| `agent.connected` | A new socket client connects | `{"agent_id": <conn_id>}` |
| `agent.disconnected` | A socket client disconnects | `{"agent_id": <conn_id>}` |
| `memory.updated` | A memory is created or modified | *(varies)* |

Additional event types may be emitted by other Nomen components via the shared broadcast channel.

### Client library

The `nomen-wire` crate provides `NomenClient` (single connection) and `ReconnectingClient` (auto-reconnect) for Rust consumers:

```rust
use nomen_wire::NomenClient;

let client = NomenClient::connect("/run/user/1000/nomen/nomen.sock").await?;

// Search memories
let results = client.request("memory.search", json!({"query": "test"})).await?;

// Subscribe to events
client.request("subscribe", json!({"events": ["*"]})).await?;
let mut events = client.events();
while let Ok(event) = events.recv().await {
    println!("{}: {:?}", event.event, event.data);
}
```

### Connection lifecycle

1. Agent connects to Unix socket
2. Server assigns a connection ID and emits `agent.connected`
3. Agent sends Request frames, receives Response frames
4. Agent optionally subscribes to push events
5. On disconnect, server cleans up subscriptions and emits `agent.disconnected`

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
| `memory.get_batch` | `memory_get_batch` | Batch fetch by d_tags |
| `memory.list` | `memory_list` | |
| `memory.delete` | `memory_delete` | |
| `message.ingest` | `message_ingest` | |
| `message.list` | `message_list` | |
| `message.context` | `message_context` | |
| `message.send` | `message_send` | |
| `entity.list` | `entity_list` | |
| `entity.relationships` | `entity_relationships` | |
| `memory.consolidate` | `memory_consolidate` | |
| `memory.consolidate_prepare` | `memory_consolidate_prepare` | Two-phase: prepare batches |
| `memory.consolidate_commit` | `memory_consolidate_commit` | Two-phase: commit extractions |
| `memory.cluster` | `memory_cluster` | |
| `memory.sync` | `memory_sync` | |
| `memory.embed` | `memory_embed` | |
| `memory.prune` | `memory_prune` | |
| `group.list` | `group_list` | |
| `group.members` | `group_members` | |
| `group.create` | `group_create` | |
| `group.add_member` | `group_add_member` | |
| `group.remove_member` | `group_remove_member` | |
| `room.resolve` | `room_resolve` | Provider → d-tag lookup |
| `room.bind` | `room_bind` | Bind provider ID to d-tag |
| `room.unbind` | `room_unbind` | Unbind provider ID from d-tag |

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
│       ├── memory.rs   — search, put, get, get_batch, list, delete
│       ├── message.rs  — ingest, list, context, send
│       ├── maintenance.rs — consolidate, prepare, commit, cluster, sync, embed, prune
│       ├── group.rs    — list, members, create, add/remove member
│       ├── room.rs     — resolve, bind, unbind
│       └── entity.rs   — list, relationships
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
