# Data Model


## Collected Messages — Kind 30100

Messages from any platform stored as Nostr events. Parameterized replaceable (NIP-33): same `d` tag = upsert.

### Canonical Messaging Hierarchy

```
platform → community? → chat → thread? → message
```

- **platform** — messaging ecosystem (`telegram`, `discord`, `slack`, `nostr`, etc.)
- **community** — optional layer above chats (Discord guild, Slack workspace)
- **chat** — primary conversation boundary
- **thread** — optional sub-conversation inside a chat
- **message** — atomic stored item

### Event Structure

```json
{
  "kind": 30100,
  "created_at": 1711540200,
  "pubkey": "<collector_npub>",
  "tags": [
    ["d", "telegram:-1003821690204:13943"],
    ["platform", "telegram"],
    ["proxy", "telegram:-1003821690204:13943", "telegram"],
    ["chat", "-1003821690204", "TechTeam", "group"],
    ["sender", "60996061", "kosti", "koshdot"],
    ["thread", "13939", "Message Bridge"],
    ["e", "<event_id_of_replied_to>", "", "reply"],
    ["reply", "telegram:-1003821690204:13941"],
    ["imeta", "url https://blossom.example/abc.jpg", "m image/jpeg", "x abc123def"]
  ],
  "content": "Hello world"
}
```

Content is plain text. Empty for media-only or location messages.

### D-Tag Formation

Use the **smallest stable provider-native coordinate set** sufficient for uniqueness:

```
<platform>:<chat_id>:<message_id>
```

Do not encode optional hierarchy layers positionally. `community` and `thread` live in their own tags.

### Collected Message Tags

| Tag | Format | Purpose | NIP |
|---|---|---|---|
| `d` | `["d", "<platform>:<chat_id>:<message_id>"]` | Replaceable identifier | NIP-33 |
| `platform` | `["platform", "<platform>"]` | Platform identifier (e.g. `telegram`, `discord`) | — |
| `proxy` | `["proxy", "<full_id>", "<platform>"]` | NIP-48 bridged origin (optional, for relay compat) | NIP-48 |
| `community` | `["community", "<id>", "<name>", "<type>"]` | Optional layer above chat |
| `chat` | `["chat", "<id>", "<name>", "<type>"]` | Primary conversation boundary |
| `thread` | `["thread", "<id>", "<name>"]` | Forum topic or thread |
| `sender` | `["sender", "<id>", "<name>", "<username>"]` | Message author |
| `imeta` | `["imeta", "url ...", "m ...", "x ..."]` | Media attachment | NIP-92 |
| `location` | `["location", "<lat>", "<lon>"]` | Geographic coordinates |
| `reply` | `["reply", "<d-tag>"]` | Reply to collected message |
| `forward` | `["forward", "<src>", "<name>", "<type>"]` | Forwarded content |
| `edited` | `["edited", "<timestamp>"]` | Edit marker |

### Chat Metadata — Kind 30101

One per chat. Content is JSON with structured metadata.

```json
{
  "kind": 30101,
  "tags": [
    ["d", "telegram:-1001234"],
    ["proxy", "telegram:-1001234", "telegram"],
    ["chat", "-1001234", "TechTeam", "group"]
  ],
  "content": "{\"participants\":[\"60996061\"],\"avatar_url\":\"...\"}"
}
```

### Indexing

| Tag | Indexed column | Filter |
|---|---|---|
| `platform` | `platform` | `#platform` |
| `chat` | `chat_id`, `chat_name`, `chat_type` | `#chat` |
| `sender` | `sender_id` | `#sender` |
| `thread` | `thread_id` | `#thread` |

Compound indexes: `(platform, chat_id)`, `(chat_id, thread_id)`.

### Event References

| Relation | When both collected | When target unknown |
|---|---|---|
| Reply | `["e", id, "", "reply"]` + `["reply", d-tag]` | `["reply", d-tag]` only |
| Forward | `["e", id, "", "mention"]` + `["forward", ...]` | `["forward", ...]` only |
| Edit | Same `d` tag (auto-replace) | N/A |
| Delete | NIP-09 kind 5 | N/A |

---


## Memory Events — Kind 31234

Memories are addressable/replaceable Nostr events. The `d` tag is the primary key; publishing a new event with the same `d` tag replaces the previous version.

### D-Tag Format (v0.3)

```
{namespace}/{topic}
```

Where namespace is `{tier}` (for `public`, `private`) or `{tier}/{scope}` (for `personal`, `group`, `circle`).

### Tiers

| Tier | Scope | Encryption | Description |
|---|---|---|---|
| `public` | — | None | Readable by anyone |
| `private` | — | NIP-44 self-encrypt | Agent-only knowledge |
| `personal` | `{hex-pubkey}` | NIP-44 self-encrypt | Between agent and a specific user |
| `group` | `{group-id}` | None (relay-enforced) | NIP-29 group members |
| `circle` | `{circle-hash}` | Shared symmetric key | Ad-hoc participant set |

### D-Tag Examples

```
public/rust-error-handling
private/agent-reasoning
personal/d29fe7c1.../ssh-config
group/techteam/deployment-process
circle/a3f8b2c1e9d04712/shared-notes
```

### Event Structure

```json
{
  "kind": 31234,
  "pubkey": "<agent-pubkey-hex>",
  "created_at": 1742742000,
  "content": "Full content as plain text/markdown...",
  "tags": [
    ["d", "public/rust-error-handling"],
    ["visibility", "public"],
    ["scope", ""],
    ["model", "anthropic/claude-opus-4-6"],
    ["version", "1"],
    ["t", "rust"],
    ["t", "error-handling"]
  ]
}
```

Content is **plain text or markdown**, not JSON. First line can serve as display title.

### Memory Types

All knowledge is stored as kind 31234 events. The `type` tag distinguishes what kind of knowledge:

| Type | Description |
|---|---|
| *(absent)* | Regular memory (default) |
| `entity:person` | Person entity |
| `entity:project` | Project entity |
| `entity:concept` | Concept entity |
| `entity:place` | Place entity |
| `entity:organization` | Organization entity |
| `entity:technology` | Technology entity |
| `cluster` | Synthesized cluster summary |

Entities are memories with structured metadata. They follow the same d-tag namespace, visibility, and relay sync as regular memories.

#### Entity Example

```json
{
  "kind": 31234,
  "tags": [
    ["d", "personal/d29fe7c1.../kosti"],
    ["visibility", "personal"],
    ["scope", "d29fe7c1..."],
    ["type", "entity:person"],
    ["rel", "personal/d29fe7c1.../nomen", "works_on"],
    ["rel", "personal/d29fe7c1.../openclaw", "works_on"]
  ],
  "content": "k0 / kosti — developer, runs OpenClaw and Nomen. Based in Finland."
}
```

#### Cluster Example

```json
{
  "kind": 31234,
  "tags": [
    ["d", "public/rust-error-handling-cluster"],
    ["visibility", "public"],
    ["scope", ""],
    ["type", "cluster"],
    ["ref", "public/rust-anyhow", "summarizes"],
    ["ref", "public/rust-thiserror", "summarizes"]
  ],
  "content": "Rust error handling: use anyhow for applications, thiserror for libraries..."
}
```

### Memory Tags

| Tag | Required | Description |
|---|---|---|
| `d` | Yes | `{namespace}/{topic}` — replaceable key |
| `visibility` | Yes | Tier: `public`, `group`, `circle`, `personal`, `private` |
| `scope` | Yes | Group id, circle hash, hex pubkey, or empty |
| `model` | Yes | Model that generated this memory |
| `type` | No | Memory type (e.g. `entity:person`, `cluster`). Absent = regular memory |
| `rel` | No | Directed relationship to another memory: `["rel", "<d-tag>", "<relation>"]` (repeatable) |
| `ref` | No | Reference to another memory: `["ref", "<d-tag>", "<relation>"]` (repeatable) |
| `version` | No | Monotonically increasing per d-tag |
| `supersedes` | No | D-tag of previous version |
| `pinned` | No | `"true"` if pinned |
| `importance` | No | 1–10 scale |
| `t` | No | Freeform topic tags (repeatable) |
| `h` | No | NIP-29 group id (for `group` tier) |
| `p` | No | Participant pubkeys (for `circle` tier) |

### Relations

All relationships are directed: the source event carries the tag, pointing to the target's d-tag.

#### Entity Relations (`rel` tag)

Used on `entity:*` type memories. Extracted during consolidation.

| Relation | From → To | Description |
|---|---|---|
| `works_on` | person → project | Active contributor |
| `collaborates_with` | person → person | Working together |
| `manages` | person → project/org | Management role |
| `owns` | person/org → project | Ownership |
| `member_of` | person → org/group | Membership |
| `depends_on` | project → project | Technical dependency |
| `uses` | project → technology | Technology usage |
| `created` | person → project | Original creator |
| `located_in` | person/org → place | Geographic location |
| `hired_by` | person → org | Employment |
| `decided` | person → concept | Decision attribution |

#### Memory References (`ref` tag)

Used between regular memories and clusters. Affect search ranking.

| Relation | From → To | Search weight | Description |
|---|---|---|---|
| `supports` | memory → memory | 0.6 | Affirms or adds evidence |
| `contradicts` | memory → memory | 0.8 | Conflicts (flagged in results) |
| `supersedes` | memory → memory | 0.5 | Replaces older knowledge |
| `summarizes` | cluster → memory | 0.6 | Cluster synthesis source |

#### Structural Edges (DB only, not in events)

| Edge | From → To | Description |
|---|---|---|
| `mentions` | memory → entity | Memory references this entity |
| `consolidated_from` | memory → collected_messages | Consolidation provenance |

### Scope and Visibility Rules

- `public` → empty scope, no encryption
- `private` → no scope needed; `event.pubkey` is the owner
- `personal` → scope is the **counterparty** pubkey, not the owner
- `group` → scope is the group identifier
- `circle` → scope is deterministic hash of sorted participant pubkeys (first 16 hex chars of SHA-256)

Legacy `internal` is normalized to `private`. Legacy `private` with pubkey scope is normalized to `personal`.

---


## Entity Extraction

Entities are extracted from messages during consolidation by a `CompositeExtractor` (heuristic + optional LLM). They are stored as kind 31234 events with `type=entity:*` — see Memory Types above.

Currently, entities are stored in the local `entity` SurrealDB table. Migration to kind 31234 relay events with `type` and `rel` tags is the next step to make them survive DB loss.

---


## Agent Identity Events

### Kind 0 — Profile Metadata

Standard NIP-01 profile with agent extensions:

```json
{
  "kind": 0,
  "content": "{\"name\":\"Nomen Agent\",\"about\":\"Nostr-native AI agent\"}",
  "tags": [["agent", "nomen"], ["bot"], ["p", "<owner-pubkey>"]]
}
```

### Kind 4199 — Agent Definition (NIP-AE)

Addressable event defining agent capabilities, published by owner.

### Kind 14199 — Owner Claims (NIP-AE)

Owner declares which agent pubkeys they control. Bidirectional verification: agent's kind 0 has owner `p` tag, owner's kind 14199 has agent `p` tag.

### Kind 4129 — Agent Lessons

Behavioral learnings. Append-only log, not replaceable.

---


## NIP Alignment

| NIP | Usage |
|---|---|
| NIP-01 | Event structure, tags |
| NIP-09 | Deletion requests (kind 5) |
| NIP-10 | Reply threading |
| NIP-17 | Gift-wrapped DMs (key distribution) |
| NIP-25 | Reactions |
| NIP-29 | Relay-based groups |
| NIP-33 | Parameterized replaceable events |
| NIP-42 | Relay AUTH |
| NIP-44 | Encryption |
| NIP-48 | Proxy tags |
| NIP-59 | Gift wrap |
| NIP-92 | Media attachments (`imeta`) |
| NIP-98 | HTTP Auth |
| NIP-94 | File metadata |
| NIP-AE | Agent attribution |
| NIP-B7 | Blossom media server |
