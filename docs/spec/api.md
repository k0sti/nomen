# API Reference

All operations route through a single canonical dispatch layer (`api::dispatch()`). Transport adapters (CLI, MCP, HTTP, ContextVM, socket) are projections of these operations.

## Envelope

### Request

```json
{ "action": "memory.search", "params": { ... } }
```

### Response — Success

```json
{ "ok": true, "result": { ... } }
```

### Response — Error

```json
{ "ok": false, "error": { "code": "invalid_params", "message": "..." } }
```

### Error Codes

| Code | Meaning |
|---|---|
| `invalid_params` | Missing or invalid parameters |
| `invalid_scope` | Scope validation failed |
| `not_found` | Memory/entity not found |
| `unauthorized` | ACL rejection |
| `rate_limited` | Too many requests |
| `internal_error` | Unexpected server error |
| `unknown_action` | Action not recognized |

---

## Message Operations

### `message.store`

Store a kind 30100 collected event. Upserts by `d` tag.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `event` | object | ✅ | — | Full kind 30100 event object |

### `message.ingest`

Convenience ingest for platforms that don't construct full events.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `content` | string | ✅ | — | Message content |
| `source` | string | — | `"unknown"` | Source system |
| `sender` | string | — | `"unknown"` | Sender identifier |
| `platform` | string | — | — | Canonical platform |
| `community_id` | string | — | — | Community id |
| `chat_id` | string | — | — | Chat id |
| `thread_id` | string | — | — | Thread id |

### `message.query`

Query collected messages using canonical hierarchy filters.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `#platform` | array | — | — | Filter by platform |
| `#community` | array | — | — | Filter by community |
| `#chat` | array | — | — | Filter by chat |
| `#thread` | array | — | — | Filter by thread |
| `#sender` | array | — | — | Filter by sender |
| `since` | string/integer | — | — | RFC3339 or unix timestamp |
| `until` | string/integer | — | — | RFC3339 or unix timestamp |
| `limit` | integer | — | 50 | Max results |

### `message.search`

BM25 fulltext search over message content. No embedding search — messages are consolidated into memories which get embeddings.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `query` | string | ✅ | — | Search query |
| `#chat` | array | — | — | Narrow scope |
| `limit` | integer | — | 10 | Max results |

### `message.context`

Recent conversation context for a chat/thread.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `#chat` | array | ✅ | — | Chat id |
| `#platform` | array | — | — | Platform |
| `#thread` | array | — | — | Thread |
| `since` | integer | — | — | Lower bound timestamp |
| `before` | integer | — | — | Upper bound timestamp |
| `limit` | integer | — | 50 | Max messages |

### `message.store_media`

Store media locally with content-addressed naming (SHA-256, Blossom convention).

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `data` | string | ✅ | — | File path or base64 data |
| `mime_type` | string | ✅ | — | MIME type |

### `message.send`

Send a message to a recipient via Nostr or other channels.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `to` | string | ✅ | — | `npub1...`, `group:<id>`, or `public` |
| `content` | string | ✅ | — | Message content |
| `channel` | string | — | `nostr` | Delivery channel |

---

## Memory Operations

### `memory.search`

Hybrid semantic + full-text search with optional graph expansion.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `query` | string | ✅ | — | Search query |
| `visibility` | string | — | — | Filter by tier |
| `scope` | string | — | — | Filter by scope |
| `limit` | integer | — | 10 | Max results |
| `retrieval.vector_weight` | float | — | 0.7 | Vector similarity weight |
| `retrieval.text_weight` | float | — | 0.3 | BM25 weight |
| `retrieval.aggregate` | boolean | — | false | Merge similar results |
| `retrieval.graph_expand` | boolean | — | false | Traverse graph edges |
| `retrieval.max_hops` | integer | — | 1 | Max graph hops |

### `memory.put`

Create or replace a named memory. Publishes to relay and stores locally.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `topic` | string | ✅ | — | Topic path |
| `content` | string | ✅ | — | Full memory text (plain text/markdown) |
| `visibility` | string | — | `public` | Tier |
| `scope` | string | — | `""` | Scope |
| `importance` | integer | — | — | 1–10 scale |
| `pinned` | boolean | — | false | Pin memory |

### `memory.get`

Retrieve a single memory by topic or d_tag.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `topic` | string | one of | — | Topic to retrieve |
| `d_tag` | string | one of | — | Direct d_tag lookup |
| `visibility` | string | — | — | For topic → d_tag resolution |
| `scope` | string | — | — | For topic → d_tag resolution |

### `memory.list`

List memories with optional filters.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `visibility` | string | — | — | Filter by tier |
| `scope` | string | — | — | Filter by scope |
| `limit` | integer | — | 100 | Max results |
| `stats` | boolean | — | false | Include statistics |

### `memory.delete`

Delete by topic, d_tag, or event ID. Publishes NIP-09 deletion to relay.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `topic` | string | one of | — | Topic |
| `d_tag` | string | one of | — | D-tag |
| `id` | string | one of | — | Event ID |

---

## Entity Operations

### `entity.list`

List extracted entities, optionally filtered by kind.

### `entity.relationships`

List typed relationships between entities.

---

## Maintenance Operations

### `memory.consolidate`

Run the consolidation pipeline: group → extract → merge → store.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `#platform` | array | — | — | Filter by platform |
| `#community` | array | — | — | Filter by community |
| `#chat` | array | — | — | Filter by chat |
| `#thread` | array | — | — | Filter by thread |
| `since` | string/integer | — | — | Since timestamp |
| `min_messages` | integer | — | 3 | Minimum to trigger |
| `batch_size` | integer | — | 50 | Max per run |
| `dry_run` | boolean | — | false | Preview only |
| `older_than` | string | — | — | Duration filter (e.g. `30m`, `1h`) |

### `memory.consolidate_prepare`

Two-phase consolidation: prepare batches for external LLM processing. Returns grouped message batches. Same filters as `memory.consolidate`.

### `memory.consolidate_commit`

Commit agent-provided extractions for a prepared session.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `session_id` | string | ✅ | — | Session from prepare |
| `extractions` | array | ✅ | — | Batch extractions with memories |

### `memory.cluster`

Synthesize related memories by namespace prefix.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `prefix` | string | — | — | Topic prefix filter |
| `min_members` | integer | — | 3 | Min memories per cluster |
| `namespace_depth` | integer | — | 2 | Grouping depth |
| `dry_run` | boolean | — | false | Preview only |

### `memory.sync`

Sync memories from relay to local DB.

### `memory.embed`

Generate embeddings for memories that lack them.

### `memory.prune`

Prune old/unused memories and consolidated messages.

---

## Group Operations

### `group.list` / `group.members` / `group.create` / `group.add_member` / `group.remove_member`

Standard CRUD for NIP-29 group management.

---

## Transport Mapping

| Transport | Canonical dispatch | Transport-specific features |
|---|---|---|
| HTTP | `POST /memory/api/dispatch` | Health, stats, config endpoints |
| MCP | Tool names use `_` (e.g. `memory_search`) | Tool listing, initialize |
| ContextVM | Direct action dispatch over Nostr events | NIP-44/59 encryption, ACL |
| Socket | Length-prefixed JSON frames | `subscribe`/`unsubscribe` for push events |

All transports share the same canonical operation semantics. Transport-specific features (socket subscriptions, HTTP health endpoints) are separate from the API.
