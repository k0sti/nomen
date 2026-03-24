# Collected Messages

Nomen stores messages from any platform as Nostr events (kind 30100). Producers convert platform messages to this format and call `message.store`. Nomen indexes tags and serves queries. Messages are not individually embedded — they are consolidated into memories (same as existing consolidation pipeline), and those memories get embedded.

## Event Schema

### Kind 30100 — Collected Message

Parameterized replaceable event (NIP-33). Same `d` tag = upsert (handles edits).

Content field is plain text — the message as written. Empty for media-only or location messages.

All metadata lives in tags.

```json
{
  "kind": 30100,
  "created_at": 1711540200,
  "pubkey": "<collector_npub>",
  "tags": [
    ["d", "telegram:-1003821690204:13943"],
    ["proxy", "telegram:-1003821690204:13943", "telegram"],
    ["chat", "-1003821690204", "TechTeam", "group"],
    ["sender", "60996061", "kosti", "koshdot"],
    ["thread", "13939", "Message Bridge"],
    ["e", "<event_id_of_replied_to>", "", "reply"],
    ["reply", "telegram:-1003821690204:13941"],
    ["imeta",
      "url https://blossom.example/abc.jpg",
      "m image/jpeg",
      "x abc123def"],
    ["location", "60.17", "24.94"]
  ],
  "content": "Hello world"
}
```

### Kind 30101 — Chat Metadata

One per chat. Content is JSON (structured metadata, not human text).

```json
{
  "kind": 30101,
  "created_at": 1711540200,
  "pubkey": "<collector_npub>",
  "tags": [
    ["d", "telegram:-1001234"],
    ["proxy", "telegram:-1001234", "telegram"],
    ["chat", "-1001234", "TechTeam", "group"]
  ],
  "content": "{\"participants\":[\"60996061\"],\"avatar_url\":\"https://blossom.example/sha256.jpg\"}"
}
```

## Tags

### Identity & Origin

| Tag | Format | Purpose | NIP |
|---|---|---|---|
| `d` | `["d", "<platform>:<chat_id>:<message_id>"]` | Unique replaceable identifier | NIP-33 |
| `proxy` | `["proxy", "<platform>:<chat_id>:<message_id>", "<platform>"]` | Marks event as bridged from external protocol | NIP-48 |

`d` tag format: `{platform}:{chat_id}:{message_id}`. This is the primary key.

`proxy` tag signals the event originated outside Nostr. Platform values: `telegram`, `discord`, `signal`, `whatsapp`, `slack`, `irc`, `matrix`, `email`, `nostr`.

### Chat & Thread

| Tag | Format | Purpose |
|---|---|---|
| `chat` | `["chat", "<chat_id>", "<chat_name>", "<chat_type>"]` | Conversation. `chat_type`: `direct`, `group` |
| `thread` | `["thread", "<thread_id>", "<thread_name>"]` | Forum topic or thread. Omitted for non-threaded |

### Sender

| Tag | Format | Purpose |
|---|---|---|
| `sender` | `["sender", "<sender_id>", "<name>", "<username>"]` | Message author. Fields after id are optional |

### Media (NIP-92)

| Tag | Format | Purpose | NIP |
|---|---|---|---|
| `imeta` | `["imeta", "url <blossom_url>", "m <mime>", "x <sha256>", ...]` | Media attachment | NIP-92 |

Multiple `imeta` tags for multiple attachments. URLs are Blossom server URLs. Supports NIP-94 fields: `dim`, `alt`, `blurhash`, `fallback`.

### Location

| Tag | Format | Purpose |
|---|---|---|
| `location` | `["location", "<lat>", "<lon>"]` | Geographic coordinates |

## Event References

### Replies

Both tags when parent is collected:

```json
["e", "<nostr_event_id_of_parent>", "", "reply"],
["reply", "telegram:-1001234:41"]
```

- `e` with NIP-10 `reply` marker — standard Nostr threading
- `reply` with d-tag of parent — resolves by platform ID

Only `reply` tag when parent not yet collected. Links resolve as backfill catches up.

### Forwards

Forwarded from collected message:

```json
["e", "<event_id_of_original>", "", "mention"],
["forward", "telegram:-1001234:99"]
```

Forwarded from unknown source:

```json
["forward", "telegram:channel:@reuters:5678", "Reuters News", "channel"]
```

Format: `["forward", "<source_id>", "<source_name>", "<source_type>"]`. Source types: `user`, `channel`, `group`, `unknown`.

### Edits

No special tag. Same `d` tag = relay replaces previous version (NIP-33 semantics). Optionally add `["edited", "<unix_timestamp>"]`.

### Deletions

Standard NIP-09:

```json
{
  "kind": 5,
  "tags": [["a", "30100:<pubkey>:telegram:-1001234:42"]],
  "content": "deleted"
}
```

### Reactions (future)

Standard NIP-25:

```json
{
  "kind": 7,
  "tags": [["e", "<event_id_of_message>"]],
  "content": "👍"
}
```

### Reference Summary

| Relation | When both collected | When target unknown |
|---|---|---|
| Reply | `["e", id, "", "reply"]` + `["reply", d-tag]` | `["reply", d-tag]` only |
| Forward | `["e", id, "", "mention"]` + `["forward", src, name, type]` | `["forward", src, name, type]` only |
| Edit | Same `d` tag (auto-replace) | N/A |
| Delete | NIP-09 kind 5 | N/A |
| Reaction | NIP-25 kind 7 `["e", id]` | N/A |

## Nomen API

### message.store

Store a kind 30100 event. Upserts by `d` tag. Content is indexed for BM25 fulltext search (no embeddings — memories get those).

```json
// Request
{"id": "req-1", "action": "message.store", "params": {
  "event": {
    "kind": 30100,
    "created_at": 1711540200,
    "pubkey": "abc123...",
    "tags": [
      ["d", "telegram:-1003821690204:13943"],
      ["proxy", "telegram:-1003821690204:13943", "telegram"],
      ["chat", "-1003821690204", "TechTeam", "group"],
      ["sender", "60996061", "kosti", "koshdot"],
      ["thread", "13939", "Message Bridge"]
    ],
    "content": "Explain 30100/30101 kinds"
  }
}}

// Response
{"id": "req-1", "ok": true, "result": {
  "d_tag": "telegram:-1003821690204:13943",
  "stored": true,
  "replaced": false
}}
```

Validation: kind must be 30100, `d` tag required.

### message.query

Tag-based filtering. Nostr filter conventions with `#` prefix for tag queries. All filters optional.

```json
// Request
{"id": "req-2", "action": "message.query", "params": {
  "#chat": ["-1003821690204"],
  "#thread": ["13939"],
  "#sender": ["60996061"],
  "#proxy": ["telegram"],
  "since": 1711500000,
  "until": 1711600000,
  "limit": 50
}}

// Response
{"id": "req-2", "ok": true, "result": {
  "count": 12,
  "events": [...]
}}
```

Events returned in chronological order.

### message.search

BM25 fulltext search over message content. No embedding search — messages are consolidated into memories which get embeddings. Tag filters narrow scope before scoring.

```json
// Request
{"id": "req-3", "action": "message.search", "params": {
  "query": "nostr event schema design",
  "#chat": ["-1003821690204"],
  "limit": 10
}}

// Response
{"id": "req-3", "ok": true, "result": {
  "count": 3,
  "events": [
    { "kind": 30100, "content": "...", "tags": [...], "score": 4.21 },
    ...
  ]
}}
```

### message.context

Convenience for retrieving conversation context. Returns recent messages from a chat/thread.

```json
// Request
{"id": "req-4", "action": "message.context", "params": {
  "#chat": ["-1003821690204"],
  "#thread": ["13939"],
  "limit": 50,
  "before": 1711540200
}}

// Response
{"id": "req-4", "ok": true, "result": {
  "count": 50,
  "events": [...]
}}
```

### message.store_media

Store media locally with content-addressed naming. Returns sha256 and path for use in `imeta` tags.

```json
// Request
{"id": "req-5", "action": "message.store_media", "params": {
  "data": "/tmp/photo.jpg",
  "mime_type": "image/jpeg"
}}

// Response
{"id": "req-5", "ok": true, "result": {
  "sha256": "a1b2c3d4...",
  "path": "/var/lib/nomen/media/a1b2c3d4.jpg",
  "size": 184292
}}
```

Accepts file path or base64 data. Deduplicates by sha256 — storing the same content twice returns the existing ref.

Media is stored locally using Blossom's content-addressing convention (SHA-256 hash as filename). No HTTP serving — files are for Nomen's internal use (consolidation, content extraction). The naming convention ensures future migration to a real Blossom server is a path swap.

```
Storage: {nomen_data_dir}/media/{sha256}.{ext}
Example: /var/lib/nomen/media/a1b2c3d4e5f6...jpg
```

Media storage is **optional**. If media archival is not needed, messages are stored with text content only. Media can be fetched later on demand via `message.fetch_media`.

`imeta` tags reference files by sha256 (`x` field) as the stable identifier:

```json
["imeta", "url file:///var/lib/nomen/media/a1b2c3.jpg", "m image/jpeg", "x a1b2c3..."]
```

If media is not yet fetched, the original platform URL is stored:

```json
["imeta", "url https://api.telegram.org/file/bot.../photo.jpg", "m image/jpeg"]
```

After `message.fetch_media`, the tag is updated with the local path and hash.

### message.import

Import historical messages from a channel. Nomen fetches messages from the platform and stores them as 30100 events.

```json
// Request
{"id": "req-6", "action": "message.import", "params": {
  "platform": "telegram",
  "chat_id": "-1003821690204",
  "since": "2025-01-01T00:00:00Z",
  "until": "2026-03-24T00:00:00Z",
  "fetch_media": false
}}

// Response
{"id": "req-6", "ok": true, "result": {
  "imported": 1234,
  "skipped": 12,
  "errors": 0
}}
```

`fetch_media`: when true, downloads media and uploads to Blossom. When false, stores text content and original media URLs only. Media can be fetched later with `message.fetch_media`.

### message.fetch_media

Fetch and archive media for already-stored messages that have original URLs but no Blossom URLs.

```json
// Request
{"id": "req-7", "action": "message.fetch_media", "params": {
  "#chat": ["-1003821690204"],
  "limit": 100
}}

// Response
{"id": "req-7", "ok": true, "result": {
  "fetched": 45,
  "failed": 3,
  "already_archived": 52
}}
```

Downloads from original URLs, uploads to Blossom, updates `imeta` tags on existing events.

## Media Store

Media storage is abstracted behind a trait. Local filesystem is the default, using Blossom's SHA-256 content-addressing convention.

```rust
#[async_trait]
pub trait MediaStore: Send + Sync {
    async fn store(&self, data: &[u8], mime_type: &str) -> Result<MediaRef>;
    async fn exists(&self, sha256: &str) -> Result<bool>;
    async fn get(&self, sha256: &str) -> Result<Option<Vec<u8>>>;
    async fn path(&self, sha256: &str) -> Option<PathBuf>;
}

pub struct MediaRef {
    pub sha256: String,
    pub path: PathBuf,
    pub size: u64,
    pub mime_type: String,
}
```

Lives in `nomen-media` crate. `LocalMediaStore` is the default. Future: `BlossomStore` (remote), S3, etc. SHA-256 naming ensures seamless migration between backends.

## Indexing

Nomen indexes all custom tags (not just single-letter). Indexed tag fields extracted on write:

| Tag | Indexed column | Used in filters |
|---|---|---|
| `proxy` | `platform` (value[1]) | `#proxy` |
| `chat` | `chat_id` (value[0]), `chat_name` (value[1]), `chat_type` (value[2]) | `#chat` |
| `sender` | `sender_id` (value[0]) | `#sender` |
| `thread` | `thread_id` (value[0]) | `#thread` |

Compound indexes: `(platform, chat_id)`, `(chat_id, thread_id)`.

## Nostr NIP Alignment

| NIP | Usage |
|---|---|
| NIP-01 | Event structure, `e`/`p` tags |
| NIP-09 | Deletion requests (kind 5) |
| NIP-10 | Reply threading (`e` tag with `reply` marker) |
| NIP-25 | Reactions (kind 7) |
| NIP-33 | Parameterized replaceable events (`d` tag, upsert) |
| NIP-48 | Proxy tags for bridged content |
| NIP-92 | Media attachments (`imeta` tags) |
| NIP-94 | File metadata fields within `imeta` |
| NIP-B7 | Blossom media server |

## Examples

### Text in forum topic

```json
{
  "kind": 30100, "created_at": 1711540200, "pubkey": "<collector>",
  "tags": [
    ["d", "telegram:-1003821690204:13943"],
    ["proxy", "telegram:-1003821690204:13943", "telegram"],
    ["chat", "-1003821690204", "TechTeam", "group"],
    ["sender", "60996061", "kosti", "koshdot"],
    ["thread", "13939", "Message Bridge"]
  ],
  "content": "Explain 30100/30101 kinds"
}
```

### Reply with image

```json
{
  "kind": 30100, "created_at": 1711540300, "pubkey": "<collector>",
  "tags": [
    ["d", "telegram:-1001234:50"],
    ["proxy", "telegram:-1001234:50", "telegram"],
    ["chat", "-1001234", "Photos", "group"],
    ["sender", "12345", "alice"],
    ["e", "<event_id_of_msg_49>", "", "reply"],
    ["reply", "telegram:-1001234:49"],
    ["imeta", "url https://blossom.example/a1b2c3.jpg", "m image/jpeg", "dim 1920x1080", "x a1b2c3d4e5f6"]
  ],
  "content": "Here's the screenshot you asked for"
}
```

### Forwarded from channel

```json
{
  "kind": 30100, "created_at": 1711540400, "pubkey": "<collector>",
  "tags": [
    ["d", "telegram:-1001234:51"],
    ["proxy", "telegram:-1001234:51", "telegram"],
    ["chat", "-1001234", "TechTeam", "group"],
    ["sender", "60996061", "kosti"],
    ["forward", "telegram:channel:@reuters:5678", "Reuters News", "channel"]
  ],
  "content": "Breaking: Nostr adoption reaches 100M users"
}
```

### Native Nostr message (re-collected)

```json
{
  "kind": 30100, "created_at": 1711540500, "pubkey": "<collector>",
  "tags": [
    ["d", "nostr:abc123eventid"],
    ["proxy", "abc123eventid", "nostr"],
    ["e", "abc123eventid", "", "mention"],
    ["chat", "nip29:chat.example.com:groupid", "Devs", "group"],
    ["sender", "npub1xyz...", "k0sh"]
  ],
  "content": "gm"
}
```

### Media-only message

```json
{
  "kind": 30100, "created_at": 1711540600, "pubkey": "<collector>",
  "tags": [
    ["d", "telegram:-1001234:52"],
    ["proxy", "telegram:-1001234:52", "telegram"],
    ["chat", "-1001234", "Photos", "group"],
    ["sender", "12345", "alice"],
    ["imeta", "url https://blossom.example/d4e5f6.mp4", "m video/mp4", "dim 1920x1080"]
  ],
  "content": ""
}
```

### Location share

```json
{
  "kind": 30100, "created_at": 1711540700, "pubkey": "<collector>",
  "tags": [
    ["d", "telegram:-1001234:53"],
    ["proxy", "telegram:-1001234:53", "telegram"],
    ["chat", "-1001234", "TechTeam", "group"],
    ["sender", "60996061", "kosti"],
    ["location", "60.1699", "24.9384"]
  ],
  "content": ""
}
```
