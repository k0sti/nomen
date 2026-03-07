# Nostr Memory Event Specification

**Version:** v0.2
**Date:** 2026-03-07
**Status:** Draft

Defines the Nostr event schema for the nomen memory system. Memory events are kind 31234 addressable/replaceable events stored on Nostr relays.

---

## Core Concepts

### Data Sovereignty

The Nostr relay is the **canonical store** for all memory data. Local caches (SQLite, JSON) are performance optimizations — not sources of truth. If local state is lost, everything recovers from the relay.

### Event Kind

All memory events use **kind 31234** (Nomen Memory). This is an addressable/replaceable event — the `d` tag makes it updatable. Publishing a new event with the same `d` tag replaces the previous version on the relay.

---

## 1. D-Tag Format

### Structure

```
{visibility}:{context}:{topic}
```

The d-tag encodes three dimensions separated by colons:

| Field | Description | Examples |
|-------|-------------|---------|
| **visibility** | Access level — who can read this memory | `public`, `group`, `circle`, `personal`, `internal` |
| **context** | Boundary — whose memory this is or where it belongs | group name, pubkey hash, npub, or empty |
| **topic** | Subject identifier — what this memory is about | `api-patterns`, `ssh-config` |

### Visibility Values

| Value | Context | Description |
|-------|---------|-------------|
| `public` | empty | Readable by anyone on the relay |
| `group` | group name/id | Readable by members of a named, managed group (NIP-29) |
| `circle` | hash of sorted pubkeys | Readable by an ad-hoc set of participants (no relay group) |
| `personal` | hex pubkey | User-auditable knowledge, readable only by author and their agents |
| `internal` | hex pubkey | Agent-only reasoning, readable only by the agent |

### Examples

```
public::nostr-relay-auth
public::rust-error-handling
group:techteam:deployment-process
group:inner-circle:weekend-plans
circle:a3f8b2c1e9d04712:shared-project-notes
personal:d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7:ssh-config
internal:d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7:agent-reasoning
```

### Design Rationale

- **Hex pubkeys, not npub** — consistent with event.pubkey, `p` tags, and all Nostr internals; no bech32 decoding needed for matching
- **No namespace prefix** — kind 31234 is the namespace; no `snow:memory:` prefix needed
- **Visibility in d-tag** — eliminates the need for a separate tier tag; relay can filter by d-tag prefix
- **Context in d-tag** — enables relay-side prefix queries like `group:techteam:*` without scanning tags
- **Topic last** — human-readable slug, unique within its visibility+context pair

### Circle Hash

For ad-hoc participant sets, the context is a deterministic hash:

1. Collect all participant pubkeys (hex)
2. Sort alphabetically
3. Concatenate with commas
4. SHA-256 hash
5. Take first 16 hex characters

```
circle:a3f8b2c1e9d04712:shared-notes
```

Participants are discoverable via `p` tags on the event.

---

## 2. Memory Event Structure

### JSON Example

```json
{
  "kind": 31234,
  "pubkey": "<author-pubkey-hex>",
  "created_at": 1739901000,
  "content": "{\"summary\":\"Use anyhow for application errors\",\"detail\":\"In application code, prefer anyhow::Result for ergonomic error propagation.\",\"context\":null}",
  "tags": [
    ["d", "public::rust-error-handling"],
    ["model", "anthropic/claude-opus-4-6"],
    ["confidence", "0.92"],
    ["version", "1"],
    ["t", "rust"],
    ["t", "error-handling"]
  ],
  "id": "<event-id>",
  "sig": "<signature>"
}
```

### Content Payload

The `content` field is a JSON object:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `summary` | string | Yes | Short summary of the memory |
| `detail` | string | Yes | Full detail / body |
| `context` | string | No | What triggered this memory |

No metadata in content — that's what tags are for.

### Tags

| Tag | Required | Description |
|-----|----------|-------------|
| `d` | Yes | `{visibility}:{context}:{topic}` — addressable/replaceable key |
| `model` | Yes | Model that generated this memory (e.g. `anthropic/claude-opus-4-6`) |
| `confidence` | Yes | Self-assessed confidence score, float in [0.0, 1.0] |
| `version` | Yes | Version number (monotonically increasing per d-tag) |
| `supersedes` | No | D-tag of the previous version this replaces |
| `t` | No | Freeform topic tags for filtering (repeatable) |
| `h` | No | NIP-29 group id (for `group` visibility, relay-enforced) |
| `p` | No | Participant pubkeys (for `circle` visibility) |

**Removed from previous spec:**
- `tier` tag — encoded in d-tag visibility
- `source` tag — redundant with `event.pubkey`

---

## 3. Visibility-Specific Behavior

### Public

```json
["d", "public::api-rate-limiting"]
```

No access restrictions. No `h` or `p` tags needed.

### Group

```json
["d", "group:techteam:deployment-process"],
["h", "techteam"]
```

The `h` tag enables relay-side group scoping (NIP-29). Membership managed by the relay. Groups are pre-defined with a name/id.

### Circle

```json
["d", "circle:a3f8b2c1e9d04712:shared-notes"],
["p", "<pubkey-hex-1>"],
["p", "<pubkey-hex-2>"],
["p", "<pubkey-hex-3>"]
```

Ad-hoc participant sets. No relay group — access enforced client-side or via NIP-44 encryption. The hash in the context deterministically identifies the participant set.

Content SHOULD be NIP-44 encrypted when the relay doesn't enforce access control.

### Personal

```json
["d", "personal:d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7:ssh-config"]
```

User-auditable knowledge. Content SHOULD be NIP-44 encrypted (self-encrypt: author encrypts to their own pubkey). Only the author and agents holding the author's nsec can decrypt.

### Internal

```json
["d", "internal:d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7:agent-reasoning"]
```

Agent-only reasoning and internal state. Same encryption as personal. Users may audit but this tier is for agent-internal use.

---

## 4. Per-User Memory

Each user that interacts with an agent gets an auto-created memory record.

### D-Tag

```
personal:1634b87b5fcfd4a6c4ff2f2de17450ccce46f9abe0b02a71876c596ec165bfed:user-profile
```

### Content Schema

```json
{
  "summary": "Project lead, prefers concise responses",
  "detail": "{\"display_names\":[[\"k0\",1739800000]],\"first_seen\":1739800000,\"notes\":[\"Project lead\"],\"preferences\":{\"language\":\"en\"},\"is_owner\":true}",
  "context": "Auto-created from user interactions"
}
```

---

## 5. Per-Group Memory

Each NIP-29 group gets an auto-created memory record.

### D-Tag

```
group:techteam:group-context
```

### Content Schema

```json
{
  "summary": "Core team coordination and architecture decisions",
  "detail": "{\"purpose\":\"Core team coordination\",\"members\":[[\"npub1abc...\",\"k0\"],[\"npub1def...\",\"Clarity\"]],\"themes\":[\"nostr\",\"agents\"],\"decisions\":[[\"Use NIP-78 for memory\",1739800000]]}",
  "context": "Auto-created from group activity"
}
```

---

## 6. Agent Lessons — Kind 4129

Behavioral learnings published by agents. Not kind 31234 — these are regular events forming an append-only log.

```json
{
  "kind": 4129,
  "pubkey": "<agent-pubkey-hex>",
  "created_at": 1739900000,
  "content": "When users ask about relay configuration, they usually mean NIP-29 group setup.",
  "tags": [
    ["e", "<agent-definition-event-id>", "", "agent-definition"]
  ]
}
```

---

## 7. Agent Identity Events

### Kind 0 — Profile Metadata

Standard NIP-01 profile with agent-specific extensions:

```json
{
  "kind": 0,
  "content": "{\"name\":\"Snowclaw\",\"about\":\"Nostr-native AI agent\"}",
  "tags": [
    ["agent", "snowclaw"],
    ["bot"],
    ["p", "<owner-pubkey-hex>"]
  ]
}
```

### Kind 4199 — Agent Definition (NIP-AE)

Addressable event defining agent capabilities, published by owner:

```json
{
  "kind": 4199,
  "tags": [
    ["d", "snowclaw"],
    ["title", "Snowclaw"],
    ["role", "Nostr-native AI agent"],
    ["tool", "web_search", "Search the web"],
    ["tool", "memory", "Persistent memory store"],
    ["p", "<agent-pubkey-hex>"]
  ]
}
```

### Kind 14199 — Owner Claims (NIP-AE)

Owner declares which agent pubkeys they control:

```json
{
  "kind": 14199,
  "pubkey": "<owner-pubkey-hex>",
  "tags": [
    ["p", "<agent-pubkey-hex-1>"],
    ["p", "<agent-pubkey-hex-2>"]
  ]
}
```

### Bidirectional Verification

For verified owner-agent relationship, both must exist:
1. Agent's kind 0 has `["p", "<owner-pubkey>"]`
2. Owner's kind 14199 has `["p", "<agent-pubkey>"]`

---

## 8. Relay Subscription Filters

### Memory Events

```json
{"kinds": [31234], "authors": ["<agent-pubkey>", "<owner-pubkey>"]}
```

### By Visibility (D-Tag Prefix)

```json
{"kinds": [31234], "#d": ["public:", "group:techteam:"]}
```

### Agent Lessons

```json
{"kinds": [4129], "authors": ["<agent-pubkey>"]}
```

### Full Startup Filter Set

```json
[
  {"kinds": [31234], "authors": ["<agent-pubkey>", "<owner-pubkey>"]},
  {"kinds": [4129], "authors": ["<agent-pubkey>"]},
  {"kinds": [0], "authors": ["<agent-pubkey>"]}
]
```

---

## 9. Relay Authentication — NIP-42

The zooid relay requires NIP-42 AUTH. On connection, the relay sends an AUTH challenge:

```json
["AUTH", "<challenge-string>"]
```

Client responds with a signed kind 22242 event:

```json
{
  "kind": 22242,
  "tags": [
    ["relay", "wss://zooid.atlantislabs.space"],
    ["challenge", "<challenge-string>"]
  ]
}
```

---

## 10. Encryption

| Visibility | Encryption | Method |
|------------|-----------|--------|
| public | None | — |
| group | Optional | TBD — relay-enforced access may suffice |
| circle | Recommended | TBD — envelope model (per-participant key wrapping) likely |
| personal | Recommended | NIP-44 self-encrypt (author encrypts to own pubkey) |
| internal | Recommended | NIP-44 self-encrypt (author encrypts to own pubkey) |

Only the `content` field is encrypted. Tags remain plaintext for relay-side filtering.

Circle and group encryption schemes are not yet finalized. Personal/internal self-encryption is straightforward: author uses NIP-44 `getConversationKey(own_privkey, own_pubkey)` to encrypt/decrypt.

---

## NIP Compatibility

| NIP | Usage |
|-----|-------|
| NIP-01 | Event signing, basic protocol |
| NIP-09 | Event deletion (memory cleanup) |
| NIP-29 | Relay-based groups (h tag scoping) |
| NIP-42 | Relay AUTH (mandatory for zooid) |
| NIP-44 | Encryption (circle + private visibility) |
| Custom 31234 | Nomen memory events (replaceable by author+kind+d) |
| NIP-AE | Agent attribution & verification |

---

## Migration from v0.1

| v0.1 | v0.2 |
|------|------|
| `snow:memory:{topic}` d-tag | `{visibility}:{context}:{topic}` d-tag |
| `tier` tag | Removed — visibility in d-tag |
| `source` tag | Removed — use `event.pubkey` |
| `snow:tier`, `snow:confidence`, etc. | `confidence`, `version`, `model` (no prefix) |
| `snowclaw:memory:npub:{npub}` | `personal:{hex-pubkey}:{topic}` |
| `snowclaw:memory:group:{id}` | `group:{id}:{topic}` |

Events with old d-tag formats should be re-published with new format during migration. Old events can be deleted via NIP-09 after migration is confirmed.
