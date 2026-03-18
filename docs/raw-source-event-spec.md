# Raw Source Event Specification

**Version:** v0.1  
**Date:** 2026-03-17  
**Status:** Draft

Defines the Nostr event schema for raw source messages ingested into Nomen from any provider. These events form the append-only ground-truth layer that semantic memories (kind 31234) are derived from.

---

## 1. Design Principles

1. **One generic event format for all providers.** No Telegram-specific or Discord-specific event kinds.
2. **Append-only.** Raw source events are immutable once published. No replaceable semantics.
3. **Relay is authoritative.** Local `raw_message` DB rows are a cache/index. If lost, they are rebuilt from the relay.
4. **DB maps 1:1 to relay events.** Each local raw-message row corresponds to exactly one relay raw source event. The relay event ID is the durable identifier.
5. **Preserve provider identity.** Provider message IDs, container IDs, timestamps, and sender identities are stored faithfully for deduplication, provenance, and room-context derivation.
6. **Consolidation does not delete source events.** Any source-event pruning is a separate maintenance system.

---

## 2. Event Kind

**Kind: 1235** (regular, non-replaceable, non-ephemeral)

---

## 3. Event Structure

### JSON Example

```json
{
  "kind": 1235,
  "pubkey": "<nomen-agent-pubkey-hex>",
  "created_at": 1742248140,
  "content": "{\"text\":\"do it, but add it to current doc\",\"metadata\":{\"topic_name\":\"Nomen Test\",\"chat_type\":\"group\"}}",
  "tags": [
    ["source", "telegram"],
    ["channel", "telegram:-1003821690204"],
    ["room", "telegram:-1003821690204"],
    ["topic", "9225"],
    ["sender", "60996061"],
    ["provider_id", "11540"],
    ["source_ts", "1742248140"],
    ["scope", "group:telegram:-1003821690204"],
    ["t", "raw"]
  ],
  "id": "<event-id>",
  "sig": "<signature>"
}
```

### Content Payload

The `content` field is a JSON object:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `text` | string | Yes | Original message text/body |
| `metadata` | object | No | Provider-specific extras (topic name, chat type, reply info, etc.) |

Raw source events store the full original message text. No truncation is applied.

Non-text content (images, files, voice messages) is referenced by URL in the metadata object (e.g. `metadata.attachment_url`). The raw event does not embed binary data.

Content is kept minimal. Structured identity and container fields live in tags for relay-side queryability.

### Tags

| Tag | Required | Description |
|-----|----------|-------------|
| `source` | Yes | Provider family: `telegram`, `discord`, `nostr`, `cli`, `webhook`, etc. |
| `channel` | Yes | Concrete conversation container identity. Provider-qualified. |
| `room` | Recommended | Room/group identity if distinct from channel. Provider-qualified. |
| `topic` | When present | Forum topic / thread ID within a room |
| `thread` | When present | Thread ID if distinct from topic (e.g. Discord threads) |
| `sender` | Yes | Provider user/sender ID |
| `sender_name` | Recommended | Human-readable sender name (for display, not identity) |
| `provider_id` | Recommended | Provider's native message/event ID |
| `source_ts` | Yes | Original message timestamp (unix seconds) |
| `scope` | Recommended | Resolved Nomen scope if known at ingest time |
| `t` | Yes | Always includes `raw` for filtering |

#### Tag design rationale

- **Tags over content fields** for identity/container data: enables relay-side subscription filters (`#source`, `#channel`, `#room`, `#topic`, `#sender`).
- **`source_ts` as tag**: the event's `created_at` is the Nostr publish time; `source_ts` preserves the original provider timestamp for temporal queries.
- **`provider_id` as tag**: enables relay-side dedup queries and provider-specific lookups.
- **`scope` is optional at ingest time**: scope may not be resolvable during ingestion (e.g. new room, no mapping yet). It can be derived or backfilled later.

---

## 4. Provider Identity Model

Each provider exposes different message identity semantics. The raw-event schema must handle all of them.

### Provider ID availability

| Provider | Message ID type | Uniqueness | Notes |
|----------|----------------|------------|-------|
| **Telegram** | `message_id` (integer) | Unique per chat | Monotonically increasing within a chat. Globally unique when combined with `chat_id`. |
| **Discord** | `message.id` (snowflake) | Globally unique | 64-bit snowflake, globally unique across all of Discord. |
| **Nostr** | `event.id` (hex) | Globally unique | SHA-256 hash of serialized event. Globally unique by construction. |
| **Signal** | `timestamp + sender` | Per-conversation | No stable message ID exposed to bridges. Timestamp + sender + conversation is the practical identity. |
| **IRC** | None | None | No message IDs. Identity is timestamp + sender + channel + content hash. |
| **Webhook** | Varies | Varies | Depends on the webhook source. May include a request ID or correlation ID. |
| **CLI** | None | None | Manual input. Identity is timestamp + content hash. |

### Identity resolution rules

Use the strongest available identity, in order:

1. **Globally unique provider ID** (Discord snowflake, Nostr event ID)
   - Store directly as `provider_id` tag
   - Dedup by `provider_id` alone

2. **Per-container unique provider ID** (Telegram `message_id`)
   - Store as `provider_id` tag
   - Dedup by `provider_id` + `channel`

3. **Composite fallback** (Signal, IRC, CLI, unknown)
   - No `provider_id` tag, or a synthetic one
   - Dedup by deterministic hash of: `source` + `channel` + `sender` + `source_ts` + `text`

### Dedup at ingest time

Before publishing a new raw source event:
1. Check local DB for existing row with same identity (per rules above)
2. If match found, skip publish (idempotent ingest)
3. If no match, publish to relay and create local row with relay event ID

### Dedup at sync time

When importing raw source events from relay:
1. Check local DB for existing row with same relay event ID
2. If match found, skip import
3. If no match, create local row from event payload

---

## 5. Container Fields

These fields describe *where* the message was observed. They must be preserved faithfully for:
- consolidation grouping (existing logic groups by channel + topic + time window)
- room-context derivation (room-context spec needs provider-qualified room/topic/sender IDs)
- provenance tracking

### Field definitions

| Field | Description | Examples |
|-------|-------------|---------|
| `channel` | Full provider-qualified conversation container | `telegram:-1003821690204`, `discord:123456:789012`, `nostr-group:wss://zooid:techteam` |
| `room` | Room/group-level identity (may equal channel) | `telegram:-1003821690204`, `discord:123456` |
| `topic` | Forum topic / sub-channel / thread ID | `9225`, `8485` |
| `thread` | Thread ID if distinct from topic | Discord thread snowflake |

### Rules
- `channel` is always set. It is the most specific container.
- `room` is set when meaningful. For flat chats, it equals `channel`. For forums, it is the parent group.
- `topic` is set when the message belongs to a forum topic or thread within a room.
- `thread` is set only when distinct from `topic` (rare; mainly Discord nested threads).

### Relation to room-context d-tags

The room-context injection spec (`03-17 Room Context Injection Spec.md`) derives d-tags from these same container fields:

| Room-context d-tag | Derived from raw-event fields |
|---|---|
| `group:<room>:room` | `room` tag |
| `group:<room>:room/<topic>` | `room` + `topic` tags |
| `personal:<sender>:room` | `sender` tag (for DMs) |

Raw events must preserve these fields in structured tags so room-context derivation does not depend on parsing opaque sender strings or channel blobs.

---

## 6. Scope Resolution

`scope` is Nomen's durable privacy/group boundary. It determines where derived semantic memories end up.

### Resolution at ingest time

| Source context | Resolved scope |
|---|---|
| DM / private message | `personal` |
| Named group with Nomen mapping | `group:<group_id>` |
| Group without mapping | `group:<channel>` (provider-qualified fallback) |
| Nostr DM | `personal` |
| Nostr NIP-29 group | `group:<h-tag>` |
| Public / CLI | `public` |

### When scope is unknown

If scope cannot be resolved at ingest time (new room, no mapping):
- Omit the `scope` tag from the raw event
- Scope is derived later during consolidation or room-context creation
- Raw events without scope are still valid and publishable

---

## 7. Encryption

| Source context | Encryption | Method |
|---|---|---|
| Public / group messages | None | Content is plaintext |
| DM / personal messages | Recommended | NIP-44 self-encrypt (same as personal named memories) |
| Circle messages | Recommended | NIP-44 circle key (same as circle named memories) |

Only the `content` field is encrypted. Tags remain plaintext for relay-side filtering and subscription.

### Privacy note
Raw source events contain original message text. For personal/DM contexts, encrypting content is strongly recommended. Tags (`sender`, `channel`, etc.) remain visible to the relay — this is acceptable because the same metadata is visible in the transport layer anyway.

### Relay retention
Raw source events MAY include a NIP-40 `expiration` tag as a hint for relays, but this is optional. Manual cleanup (Section 12) is the default retention strategy.

---

## 8. Local DB Schema

The `raw_message` table should align directly with the raw source event structure.

### Current schema (from `src/db.rs`)
```sql
DEFINE TABLE IF NOT EXISTS raw_message SCHEMALESS;
DEFINE FIELD IF NOT EXISTS source       ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS source_id    ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS sender       ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS channel      ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS content      ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS metadata     ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at   ON raw_message TYPE string;
DEFINE FIELD IF NOT EXISTS consolidated ON raw_message TYPE bool DEFAULT false;
```

### Proposed additions
```sql
DEFINE FIELD IF NOT EXISTS nostr_event_id   ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS provider_id      ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS sender_id        ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS room             ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS topic            ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS thread           ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS scope            ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS source_created_at ON raw_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS publish_status   ON raw_message TYPE option<string>;

DEFINE INDEX IF NOT EXISTS raw_msg_nostr_id    ON raw_message FIELDS nostr_event_id UNIQUE;
DEFINE INDEX IF NOT EXISTS raw_msg_provider_id ON raw_message FIELDS provider_id, channel;
```

### Field mapping

| Raw event tag/field | Local DB field | Notes |
|---|---|---|
| Nostr event `id` | `nostr_event_id` | Primary durable identity |
| `source` tag | `source` | Provider family |
| `provider_id` tag | `provider_id` | Provider message/event ID |
| `sender` tag | `sender_id` | Provider user/sender ID |
| `sender_name` tag | `sender` | Human-readable name |
| `channel` tag | `channel` | Container identity |
| `room` tag | `room` | Room/group identity |
| `topic` tag | `topic` | Forum topic / thread |
| `thread` tag | `thread` | Thread if distinct from topic |
| `scope` tag | `scope` | Resolved Nomen scope |
| `source_ts` tag | `source_created_at` | Original provider timestamp |
| Event `created_at` | `created_at` | Nostr publish time |
| Content `text` | `content` | Message text |
| Content `metadata` | `metadata` | JSON blob |
| — | `consolidated` | Local processing flag |
| — | `publish_status` | `published`, `pending`, `failed` |

### Migration note
Existing `raw_message` rows without the new fields get null values. New rows should populate all available fields. The `source_id` field is superseded by `provider_id` + `nostr_event_id` but can be kept for backward compat during migration.

---

## 9. Ingest Flow

### Publish-on-ingest (normal path)

```
1. Receive raw message from bridge/plugin/CLI/webhook
2. Extract structured fields: source, channel, room, topic, thread, sender, sender_id, provider_id, source_ts, content, metadata
3. Check local DB for duplicate (by provider_id + channel, or fallback identity)
4. If duplicate → skip, return existing row ID
5. Build raw source event (kind TBD, tags + content JSON)
6. Publish to relay
7. On success → create local raw_message row with nostr_event_id + all fields, publish_status = "published"
8. On relay failure → create local row with publish_status = "pending" (retry queue or next sync)
9. Return row ID
```

### Import-from-relay (sync/recovery path)

```
1. Fetch raw source events from relay (filter by kind + author pubkey)
2. For each event:
   a. Check local DB for existing row with same nostr_event_id
   b. If exists → skip
   c. If not → parse tags + content into raw_message fields
   d. Create local row with publish_status = "published", consolidated = false
3. Consolidation can then process newly imported rows normally
```

---

## 10. Relay Subscription Filters

### Raw source events
```json
{"kinds": [1235], "authors": ["<nomen-pubkey>"]}
```

### By provider
```json
{"kinds": [1235], "authors": ["<nomen-pubkey>"], "#source": ["telegram"]}
```

### By room/channel
```json
{"kinds": [1235], "authors": ["<nomen-pubkey>"], "#channel": ["telegram:-1003821690204"]}
```

### By topic within room
```json
{"kinds": [1235], "authors": ["<nomen-pubkey>"], "#room": ["telegram:-1003821690204"], "#topic": ["9225"]}
```

### Combined startup filter (all data)
```json
[
  {"kinds": [31234], "authors": ["<nomen-pubkey>", "<owner-pubkey>"]},
  {"kinds": [1235], "authors": ["<nomen-pubkey>"]},
  {"kinds": [4129], "authors": ["<nomen-pubkey>"]},
  {"kinds": [0], "authors": ["<nomen-pubkey>"]}
]
```

---

## 11. Consolidation Interaction

Raw source events are **input** to the consolidation pipeline. Consolidation reads them, extracts semantic memories, and marks them processed locally. It does **not** delete or modify the relay source events.

Raw source events capture a point-in-time snapshot of the original message. If a provider message is later edited or deleted, the original raw event is not modified or retracted. Downstream consumers should treat raw events as immutable historical records.

### What consolidation uses from raw events
- `content` (text) — extraction input
- `source_created_at` — populates `source_time_start` / `source_time_end` on derived semantic memories
- `channel` + `room` + `topic` + `thread` — grouping/partitioning
- `sender` / `sender_id` — grouping, entity extraction
- `scope` — tier/visibility derivation for output memories
- `provider_id` — provenance links

### What consolidation does NOT do
- delete raw source events from relay
- modify raw source events
- re-publish raw source events

---

## 12. Source Event Cleanup (separate system)

Source event deletion/pruning is **not** part of consolidation. It is a separate maintenance task with its own policy.

### Proposed cleanup modes (future work)

| Mode | Behavior |
|---|---|
| `none` (default) | Never delete raw source events |
| `local-only` | Delete local `raw_message` rows older than retention period; relay copies preserved |
| `full` | Delete both local rows and relay events (NIP-09) older than retention period |

### Cleanup should respect
- minimum retention period (configurable)
- only delete rows/events that have been consolidated at least once
- never delete raw events that are the sole source for an unconsolidated batch

---

## 13. Compatibility Notes

### With Nostr Memory Spec (kind 31234)
- Raw source events (kind TBD) and named memories (kind 31234) are distinct event kinds
- Named memories reference raw source events via `consolidated_from` edges (local DB) and optionally `e` tags (relay)
- Both share the same relay, same author pubkey, same auth (NIP-42)

### With Room Context Injection Spec
- Room-context d-tags are derived from raw-event container fields (`room`, `topic`, `sender`)
- Raw events preserve provider-qualified container IDs, enabling room-context derivation without string parsing
- Raw events do not themselves carry room-context d-tags

### With Two-Phase Consolidation Spec
- `consolidate_prepare` reads from local `raw_message` cache (populated from relay raw events)
- `consolidate_commit` writes semantic memories; does not touch raw source events
- Two-phase flow is fully compatible; no changes needed beyond ensuring raw rows have the new structured fields

---

## Resolved Decisions

1. **Event kind number** — Kind 1235 confirmed (regular, non-replaceable, non-ephemeral).
2. **Relay retention policy** — Optional NIP-40 expiration tag as relay hint; manual cleanup (Section 12) is the default retention strategy.
3. **Content size limits** — Full message storage, no truncation applied.
4. **Media/attachments** — Non-text content referenced by URL in metadata (e.g. `metadata.attachment_url`); no embedded binary data.
5. **Edit/delete signals** — Original point-in-time snapshot is preserved; no edit/delete follow-up events. Raw events are immutable historical records.

---

## References

- `docs/nostr-memory-spec.md` — Named memory event schema (kind 31234)
- `docs/consolidation-spec.md` — Consolidation pipeline spec
- `docs/architecture.md` — Nomen architecture overview
- `obsidian/03-17 Room Context Injection Spec.md` — Room context injection design
- `obsidian/03-17 Consolidation Design Notes.md` — Design research notes
- `obsidian/03-17 Raw Event Source Implementation Tasks.md` — Implementation task checklist
