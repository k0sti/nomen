# D-Tag Format v0.3

**Version:** v0.3
**Date:** 2026-03-23
**Status:** Draft
**Supersedes:** v0.2 (`{visibility}:{scope}:{topic}`)

---

## Motivation

The v0.2 d-tag format encodes `{visibility}:{scope}:{topic}` where scope includes the owner's pubkey for `personal` and `internal` tiers. Since the owner pubkey is already present on every Nostr event (`event.pubkey`), duplicating it in the d-tag is redundant. This wastes space and creates a dependency between d-tag content and the signing key.

v0.3 removes pubkey from the d-tag entirely and restructures the format so that the visibility tier acts as a top-level namespace prefix followed by an optional scope and the topic path.

---

## Design Principles

1. **No pubkey in d-tag** — the event's `pubkey` field is the owner; don't duplicate it
2. **Tier as top-level prefix** — the first path segment declares who can read the memory
3. **Slash-separated** — `/` instead of `:` for natural path semantics
4. **Scope identifies the counterparty or group** — not the owner
5. **Topic is a freeform path** — allows hierarchical organization

---

## Terminology

| Term | Definition |
|------|-----------|
| **Tier** | The visibility level — declares who is allowed to read the memory. Implies a hierarchy from most open (`public`) to most restricted (`private`). Determines encryption strategy and access control model. |
| **Scope** | The boundary within a tier — identifies the specific counterparty (pubkey), group (id), or circle (hash). Empty for tiers that don't need one (`public`, `private`). |
| **Namespace** | The combination of tier + scope. The full prefix before the topic. Determines visibility, encryption, and access as a single unit. In code: `memory.namespace()` returns `{tier}` or `{tier}/{scope}`. |
| **Topic** | The subject identifier — a freeform, optionally hierarchical path describing what the memory is about. |

---

## Format

```
{namespace}/{topic}
```

Where namespace is:

```
{tier}            — for public, private (no scope)
{tier}/{scope}    — for personal, group, circle
```

### Tiers

| Tier | Scope | Encryption | Description |
|------|-------|------------|-------------|
| `public` | — | None | Readable by anyone on the relay |
| `private` | — | NIP-44 self-encrypt | Agent-only knowledge, readable only by the author |
| `personal` | `{hex-pubkey}` | NIP-44 self-encrypt | Knowledge between the agent and a specific user |
| `group` | `{group-id}` | None (relay-enforced) | Readable by members of a NIP-29 group |
| `circle` | `{circle-hash}` | Shared symmetric key | Encrypted memories for an ad-hoc set of participants |

### Tier Semantics

- **`public`** — anyone can read. No scope needed. Unencrypted.
- **`private`** — only the event author (agent) can read. No scope needed — ownership is `event.pubkey`. NIP-44 self-encrypted. Replaces v0.2 `internal`.
- **`personal`** — between the agent and one specific user. Scope is the user's hex pubkey (the counterparty, not the owner). NIP-44 self-encrypted. The user can audit these memories via their agent relationship.
- **`group`** — members of a named NIP-29 group. Scope is the group identifier. Access enforced by the relay. Unencrypted content (relay handles membership).
- **`circle`** — ad-hoc set of participants. Scope is a deterministic hash of participant pubkeys. Content encrypted with a shared symmetric key.

---

## D-Tag Examples

```
public/rust-error-handling
public/nostr/relay-auth
private/agent-reasoning
private/planning/weekly-review
personal/d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7/ssh-config
personal/d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7/user-profile
group/techteam/deployment-process
group/inner-circle/weekend-plans
circle/a3f8b2c1e9d04712/shared-project-notes
circle/a3f8b2c1e9d04712/design/architecture
```

### Hierarchical Topics

Topics can use `/` for sub-categorization:

```
public/rust/error-handling
public/nostr/nip-29/group-management
private/planning/2026/q1-goals
personal/{pubkey}/projects/nomen
group/techteam/incidents/2026-03-15
```

---

## Indexed Tags

The `visibility` and `scope` tags from v0.2 are retained for relay-side filtering (NIP-01 tag filters are exact-match only; prefix queries on d-tags are not supported).

```json
{
  "tags": [
    ["d", "public/rust-error-handling"],
    ["visibility", "public"],
    ["scope", ""]
  ]
}
```

```json
{
  "tags": [
    ["d", "personal/d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7/ssh-config"],
    ["visibility", "personal"],
    ["scope", "d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7"]
  ]
}
```

```json
{
  "tags": [
    ["d", "group/techteam/deployment-process"],
    ["visibility", "group"],
    ["scope", "techteam"],
    ["h", "techteam"]
  ]
}
```

```json
{
  "tags": [
    ["d", "circle/a3f8b2c1e9d04712/shared-notes"],
    ["visibility", "circle"],
    ["scope", "a3f8b2c1e9d04712"],
    ["p", "<pubkey-hex-1>"],
    ["p", "<pubkey-hex-2>"]
  ]
}
```

### Relay Subscription Filters

```json
{"kinds": [31234], "#visibility": ["public"]}
{"kinds": [31234], "#visibility": ["group"], "#scope": ["techteam"]}
{"kinds": [31234], "#visibility": ["personal", "private"], "authors": ["<agent-pubkey>"]}
{"kinds": [31234], "#visibility": ["circle"], "#scope": ["a3f8b2c1e9d04712"]}
```

---

## Circle Hash

Unchanged from v0.2. For ad-hoc participant sets:

1. Collect all participant hex pubkeys (including the agent)
2. Sort alphabetically
3. Concatenate with commas
4. SHA-256 hash the result
5. Take the first 16 hex characters

Participants are discoverable via `p` tags on the event.

---

## Encryption Model

| Tier | Encryption | Method |
|------|-----------|--------|
| `public` | None | — |
| `private` | Required | NIP-44 self-encrypt (`getConversationKey(own_privkey, own_pubkey)`) |
| `personal` | Required | NIP-44 self-encrypt (same as private — author's key) |
| `group` | None | Relay-enforced access (NIP-29) |
| `circle` | Required | Shared symmetric key (ChaCha20-Poly1305) |

Only the `content` field is encrypted. Tags remain plaintext for relay-side filtering.

### Circle Key Derivation

**2-participant circles (agent + 1 user):**
```
circle_key = HKDF-SHA256(
  ikm  = ECDH(my_nsec, their_pubkey),
  salt = "nomen-circle",
  info = circle_id
)
```

**Multi-participant circles (agent + N users, N ≥ 2):**
Agent generates a random 256-bit symmetric key and distributes it to each participant via NIP-44 encrypted DM.

---

## Migration from v0.2

### D-Tag Mapping

| v0.2 | v0.3 |
|------|------|
| `public::{topic}` | `public/{topic}` |
| `internal:{hex-pubkey}:{topic}` | `private/{topic}` |
| `personal:{hex-pubkey}:{topic}` | `personal/{hex-pubkey}/{topic}` |
| `group:{group-id}:{topic}` | `group/{group-id}/{topic}` |
| `circle:{hash}:{topic}` | `circle/{hash}/{topic}` |

### Key Changes

1. **Separator: `:` → `/`** — path-like semantics
2. **`internal` → `private`** — clearer visibility naming; the tier describes *who can see it*, not what it's used for
3. **Pubkey removed from `private` (was `internal`) scope** — no longer needed; `event.pubkey` is the owner
4. **Pubkey retained in `personal` scope** — this is the *counterparty* pubkey, not the owner

### Migration Procedure

One-time rewrite. No backwards compatibility period.

1. Read all kind 31234 events from relay
2. Parse v0.2 d-tag format
3. Compute new v0.3 d-tag
4. Publish new event with v0.3 d-tag and updated `visibility`/`scope` tags
5. Publish NIP-09 deletion for old event

### Dual-Read Support

During migration, the parser should accept both `:` and `/` separators and both `internal`/`private` as tier names. After migration is complete, v0.2 parsing can be removed.

---

## Full Event Example

```json
{
  "kind": 31234,
  "pubkey": "<agent-pubkey-hex>",
  "created_at": 1742742000,
  "content": "{\"summary\":\"Use anyhow for application errors\",\"detail\":\"In application code, prefer anyhow::Result for ergonomic error propagation.\",\"context\":null}",
  "tags": [
    ["d", "public/rust-error-handling"],
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

---

## Compatibility Notes

- All other tags (`model`, `confidence`, `version`, `t`, `h`, `p`, `supersedes`) remain unchanged from v0.2
- Content payload schema unchanged
- Event kind unchanged (31234)
- Circle hash algorithm unchanged
- Agent lessons (kind 4129) and agent identity events are not affected
