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
{visibility}:{scope}:{topic}
```

The d-tag encodes three dimensions separated by colons:

| Field | Description | Examples |
|-------|-------------|---------|
| **visibility** | Access level — who can read this memory | `public`, `group`, `circle`, `personal`, `internal` |
| **scope** | Nostr-native boundary — whose memory this is or where it belongs | group id, pubkey hash, circle hash, or empty |
| **topic** | Subject identifier — what this memory is about | `api-patterns`, `ssh-config` |

**Terminology:** `scope` is the durable Nostr-native boundary used by memories. Provider-specific transport/container details (Telegram topic IDs, Discord thread IDs, etc.) do **not** belong in `scope`; they belong in conversation-container metadata (canonical: `platform/community/chat/thread`) attached to collected messages and provenance records.

### Visibility Values

| Value | Context | Description |
|-------|---------|-------------|
| `public` | empty | Readable by anyone on the relay |
| `group` | group name/id | Readable by members of a named, managed group (NIP-29) |
| `circle` | hash of sorted pubkeys | Readable by an ad-hoc set of participants (no relay group) |
| `personal` | hex pubkey | User-auditable knowledge, readable only by author and their agents |
| `internal` | hex pubkey | Agent-only reasoning, readable only by the agent |

> **Tier Resolution (v0.2):** The legacy `private` visibility is normalized to `personal` on read.
> The five-tier model (`public`, `group`, `circle`, `personal`, `internal`) supersedes
> the earlier three-tier (`public`, `group`, `private`) and four-tier proposals.
> `circle` visibility encryption is TBD and not yet implemented.

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
- **Visibility in d-tag** — human-readable, self-documenting addressing (e.g. `group:techteam:deploy`)
- **Indexed tags for querying** — `visibility` and `scope` tags enable relay-side filtering without prefix matching (NIP-01 tag filters are exact-match only; prefix queries like `group:techteam:*` are not supported by relays)
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
    ["visibility", "public"],
    ["scope", ""],
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
| `d` | Yes | `{visibility}:{scope}:{topic}` — addressable/replaceable key |
| `visibility` | Yes | Visibility tier: `public`, `group`, `circle`, `personal`, `internal` — indexed for relay-side filtering |
| `scope` | Yes | Scope identifier (group name, circle hash, pubkey hex, or empty for public) — indexed for relay-side filtering |
| `model` | Yes | Model that generated this memory (e.g. `anthropic/claude-opus-4-6`) |
| `confidence` | Yes | Self-assessed confidence score, float in [0.0, 1.0] |
| `version` | Yes | Version number (monotonically increasing per d-tag) |
| `supersedes` | No | D-tag of the previous version this replaces |
| `t` | No | Freeform topic tags for filtering (repeatable) |
| `h` | No | NIP-29 group id (for `group` visibility, relay-enforced) |
| `p` | No | Participant pubkeys (for `circle` visibility) |

**Querying by visibility/scope:**
```json
{"kinds": [31234], "#visibility": ["group"]}
{"kinds": [31234], "#visibility": ["group"], "#scope": ["techteam"]}
{"kinds": [31234], "#visibility": ["personal", "internal"], "authors": ["<pubkey>"]}
```

**Removed from previous spec:**
- `tier` tag — replaced by `visibility` tag
- `source` tag — redundant with `event.pubkey`

---

## 3. Visibility-Specific Behavior

### Public

```json
["d", "public::api-rate-limiting"],
["visibility", "public"],
["scope", ""]
```

No access restrictions. No `h` or `p` tags needed.

### Group

```json
["d", "group:techteam:deployment-process"],
["visibility", "group"],
["scope", "techteam"],
["h", "techteam"]
```

The `h` tag enables relay-side group scoping (NIP-29). Membership managed by the relay. Groups are pre-defined with a name/id.

### Circle

```json
["d", "circle:a3f8b2c1e9d04712:shared-notes"],
["visibility", "circle"],
["scope", "a3f8b2c1e9d04712"],
["p", "<pubkey-hex-1>"],
["p", "<pubkey-hex-2>"],
["p", "<pubkey-hex-3>"]
```

Ad-hoc participant sets defined by their members. The agent (Nomen) is always a participant. The hash in the context deterministically identifies the participant set.

Content MUST be encrypted with the circle's shared symmetric key (see §10 Encryption). For 2-participant circles (agent + 1 user), the key is ECDH-derived. For multi-participant circles, the agent generates and distributes the key via NIP-44 DMs.

### Personal

```json
["d", "personal:d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7:ssh-config"],
["visibility", "personal"],
["scope", "d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7"]
```

User-auditable knowledge. Content SHOULD be NIP-44 encrypted (self-encrypt: author encrypts to their own pubkey). Only the author and agents holding the author's nsec can decrypt.

### Internal

```json
["d", "internal:d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7:agent-reasoning"],
["visibility", "internal"],
["scope", "d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7"]
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

## 8. Scope and Channel Model

### Scope

`scope` is defined exactly as in the memory model and is always Nostr-native:

- `public` → empty scope
- `group` → NIP-29 group id
- `circle` → deterministic participant-set hash
- `personal` / `internal` → hex pubkey

Scope is the durable boundary for visibility, access control, and memory ownership.

### Conversation Container (formerly "Channel")

Normalized collected messages use the canonical messaging hierarchy:

**platform → community → chat → thread → message**

For details, see `collected-messages.md`.

The legacy term `channel` referred to provider-specific container identity for raw messages. In canonical normalized data, prefer structured fields: `platform`, optional `community`, `chat`, optional `thread`.

### Rule

- **Memories attach to scope.**
- **Messages attach to their conversation container (platform/community/chat/thread) and resolve to a scope.**
- Non-Nostr provider identifiers MUST NOT be embedded into the memory scope or d-tag.

This keeps the durable memory model transport-neutral while still allowing multi-container ingestion and provenance.

## 9. Relay Subscription Filters

### Memory Events

```json
{"kinds": [31234], "authors": ["<agent-pubkey>", "<owner-pubkey>"]}
```

### By Visibility / Scope

```json
{"kinds": [31234], "#visibility": ["public"]}
{"kinds": [31234], "#visibility": ["group"], "#scope": ["techteam"]}
{"kinds": [31234], "#visibility": ["personal", "internal"], "authors": ["<pubkey>"]}
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

### Circle Encryption

Circle encryption uses a **shared symmetric key** model. The agent (Nomen) is always a participant in every circle.

**Key derivation for 2-participant circles (agent + 1 user):**

The circle key is derived via ECDH — both sides compute it independently, no key distribution needed:

```
circle_key = HKDF-SHA256(
  ikm  = ECDH(my_nsec, their_pubkey),     // NIP-44 conversation key
  salt = "nomen-circle",
  info = circle_id                          // first 16 hex chars of SHA-256(sorted pubkeys)
)
```

**Key derivation for multi-participant circles (agent + N users, N ≥ 2):**

ECDH cannot produce a shared secret for 3+ parties. The agent generates a random 256-bit symmetric key and distributes it to each participant via NIP-44 encrypted DM (pairwise ECDH with each user).

**Encryption algorithm:** ChaCha20-Poly1305 (NIP-44 primitives). Content is encrypted with the shared key. No forward secrecy — by design, these are persistent memories meant to be readable forever by all participants.

### Group Encryption

Group visibility events use the same shared symmetric key model. The group key is generated by the agent and distributed to group members via NIP-44 DMs. Relay-side access control (NIP-29) provides an additional layer but encryption ensures confidentiality even on shared relays.

### Personal/Internal Encryption

Personal/internal self-encryption is straightforward: author uses NIP-44 `getConversationKey(own_privkey, own_pubkey)` to encrypt/decrypt.

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
