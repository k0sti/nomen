# Nostr Memory Event Specification

**Version:** v0.1
**Date:** 2026-03-04
**Status:** Draft

Defines the Nostr event schema for the nomen memory system. Memory events are custom kind 31234 addressable/replaceable events stored on Nostr relays.

---

## Core Concepts

### Data Sovereignty

The Nostr relay is the **canonical store** for all memory data. Local caches (SQLite, JSON) are performance optimizations — not sources of truth. If local state is lost, everything recovers from the relay.

### Event Kind

All memory events use **kind 31234** (Nomen Memory). This is an addressable/replaceable event — the `d` tag makes it updatable. Publishing a new event with the same `d` tag replaces the previous version on the relay.

---

## 1. Memory Event Structure

### JSON Example

```json
{
  "kind": 31234,
  "pubkey": "<author-pubkey-hex>",
  "created_at": 1739901000,
  "content": "{\"summary\":\"Use anyhow for application errors\",\"detail\":\"In application code, prefer anyhow::Result for ergonomic error propagation.\",\"context\":null}",
  "tags": [
    ["d", "rust/error-handling"],
    ["tier", "public"],
    ["model", "anthropic/claude-opus-4-6"],
    ["confidence", "0.92"],
    ["source", "<originating-agent-pubkey-hex>"],
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
| `context` | string | No | Optional structured context |

### Tags

| Tag | Required | Description |
|-----|----------|-------------|
| `d` | Yes | Topic key: `<namespace>/<topic>` — makes event addressable/replaceable |
| `tier` | Yes | Visibility tier: `public`, `group`, or `private` |
| `model` | Yes | Model that generated this memory (e.g. `anthropic/claude-opus-4-6`) |
| `confidence` | Yes | Self-assessed confidence score, float in [0.0, 1.0] |
| `source` | Yes | Pubkey (hex) of the originating agent |
| `version` | Yes | Version number (monotonically increasing per topic+source) |
| `supersedes` | No | Event ID of the previous version this replaces |
| `t` | No | Topic tags for relay-side filtering (repeatable) |

### D-Tag Namespace

| Pattern | Scope | Description |
|---------|-------|-------------|
| `<namespace>/<topic>` | Memory | Collective memory entry keyed by topic |
| `snowclaw:memory:npub:<npub1...>` | Per-user | Agent's memory about a specific user |
| `snowclaw:memory:group:<group_id>` | Per-group | Agent's memory about a specific group |
| `snowclaw:config:group:<group_id>` | Config | Dynamic group configuration |
| `snowclaw:config:global` | Config | Global dynamic configuration |

---

## 2. Per-User Memory

Each user that interacts with an agent gets an auto-created memory record.

### Content Schema

```json
{
  "display_names": [["k0", 1739800000]],
  "first_seen": 1739800000,
  "notes": ["Project lead", "Prefers concise responses"],
  "preferences": {"language": "en"},
  "is_owner": true
}
```

### D-Tag

```
snowclaw:memory:npub:npub1zc6ts76lel22d38l9uk7zazsen8yd7dtuzcz5uv8d3vkast9hlks4725sl
```

Uses bech32 npub for human readability. Full npub, no truncation.

---

## 3. Per-Group Memory

Each NIP-29 group gets an auto-created memory record.

### Content Schema

```json
{
  "purpose": "Core team coordination and architecture decisions",
  "members": [["npub1abc...", "k0"], ["npub1def...", "Clarity"]],
  "themes": ["nostr", "agents", "task-system"],
  "decisions": [["Use NIP-78 for memory", 1739800000], ["Zooid as primary relay", 1739850000]]
}
```

### D-Tag

```
snowclaw:memory:group:techteam
```

### Group Scoping

The `h` tag scopes visibility to group members (relay-enforced):

```json
["h", "techteam"]
```

### Group Types

**Named groups** (NIP-29): Pre-defined with an ID, mapped to a relay group via `h` tag. Members managed by the relay. Used for teams, communities, projects.

```json
["h", "techteam"]
```

**Ad-hoc npub sets**: Implicit groups formed by a set of participants (e.g., a multi-party DM). No relay group — scoped by a deterministic hash of sorted participant npubs. Used for private conversations between specific agents/users.

```json
["snow:scope", "sha256:sorted_npub1,npub2,npub3"]
```

Ad-hoc sets are always private tier with NIP-44 encryption. They don't use `h` tags since they aren't relay-managed groups.

---

## 4. Agent Lessons — Kind 4129

Behavioral learnings published by agents. Not NIP-78 — these are regular events forming an append-only log.

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

## 5. Agent Identity Events

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

## 6. Relay Subscription Filters

### Memory Events

```json
{"kinds": [31234], "authors": ["<agent-pubkey>", "<owner-pubkey>"]}
```

### With D-Tag Prefix Filter

```json
{"kinds": [31234], "#d": [...]}
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

## 7. Relay Authentication — NIP-42

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

## NIP Compatibility

| NIP | Usage |
|-----|-------|
| NIP-01 | Event signing, basic protocol |
| NIP-09 | Event deletion (memory cleanup) |
| NIP-29 | Relay-based groups (h tag scoping) |
| NIP-42 | Relay AUTH (mandatory for zooid) |
| NIP-44 | Encryption (optional for private tier) |
| Custom 31234 | Nomen memory events (replaceable by author+kind+d) |
| NIP-AE | Agent attribution & verification |
