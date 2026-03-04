# Memory Tiers

**Version:** v0.1
**Date:** 2026-03-04

Describes the three-tier visibility system for nomen memory events.

---

## Overview

Every memory event has a **tier** that determines its visibility and access scope. Tiers are encoded in the `snow:tier` tag on kind 30078 events.

## Tier Definitions

### 1. Public

**Tag value:** `public`

- Published to public relays, readable by any nomen/Snowclaw instance
- Used for: general knowledge, shared learnings, public agent profiles
- No encryption
- Relay: any NIP-78 compatible relay

**Example use cases:**
- "Rust error handling best practices" (confidence: 0.92)
- "NIP-44 encryption is required for DMs" (confidence: 0.95)
- Agent lessons (kind 4129)

### 2. Group

**Tag value:** `group`

- Scoped to a NIP-29 relay group via the `h` tag
- Only group members can read (relay-enforced access control)
- Used for: team knowledge, project context, group decisions
- Not encrypted (relay access control provides scoping)

**Example use cases:**
- "techteam group purpose: Core team coordination" 
- "Decision: Use NIP-78 for memory (2026-02-17)"
- Per-group configuration overrides

**Event structure:**
```json
{
  "kind": 30078,
  "tags": [
    ["d", "snowclaw:memory:group:techteam"],
    ["snow:tier", "group"],
    ["h", "techteam"]
  ]
}
```

The `h` tag is critical — it tells the relay which group this event belongs to. The relay only serves it to authenticated group members.

### 3. Private

**Tag value:** `private`

- Encrypted between one agent and one entity (human or agent)
- Used for: personal user preferences, sensitive observations, private agent state
- **Current implementation:** Not encrypted (planned for Phase 4 via NIP-44)
- **Future:** Content encrypted to agent's own pubkey (self-encryption) or to specific recipient

**Example use cases:**
- "User k0 prefers concise responses" (per-npub memory)
- "User's timezone is UTC+2"
- Internal agent state that shouldn't be visible to group members

**Event structure:**
```json
{
  "kind": 30078,
  "tags": [
    ["d", "snowclaw:memory:npub:npub1zc6..."],
    ["snow:tier", "private"]
  ]
}
```

**Future encrypted form:**
```json
{
  "kind": 30078,
  "content": "<nip44-encrypted-json>",
  "tags": [
    ["d", "snowclaw:memory:npub:npub1zc6..."],
    ["snow:tier", "private"],
    ["encrypted", "nip44"]
  ]
}
```

---

## Tier Resolution in MemoryTier Enum

```rust
pub enum MemoryTier {
    /// Published to public relays, readable by any instance.
    Public,
    /// Scoped to a group (relay-scoped via h tag).
    Group(String),    // group_id
    /// Encrypted between one agent and one entity.
    Private(String),  // target pubkey
}
```

The `String` payload carries context:
- `Group("techteam")` → scoped to the "techteam" NIP-29 group
- `Private("npub1zc6...")` → private to interaction with that specific user

---

## Tier in Ranking

When searching memories, tier affects which results are returned:

| Search Context | Tiers Included |
|---------------|----------------|
| Global search | Public only |
| Group context | Public + Group(matching) |
| DM context | Public + Private(matching) |
| Agent self-query | All tiers |

---

## Model Tiers (Separate Concept)

In addition to visibility tiers, memories are ranked by the **model tier** of the AI that generated them. This is a quality signal, not a visibility control.

| Tier | Weight | Models |
|------|--------|--------|
| 1 (highest) | 1.0 | claude-opus-4, o3, gpt-5 |
| 2 | 0.8 | claude-sonnet-4, gpt-4.1, gemini-2.5-pro |
| 3 | 0.6 | claude-haiku, gpt-4.1-mini |
| 4 (lowest) | 0.4 | llama-*, mistral/*, local/* |

**Ranking formula:** `effective_score = relevance × source_trust × tier_weight`

Where:
- `relevance` = BM25 FTS5 search score (0.0–1.0)
- `source_trust` = configured trust weight for the authoring agent (0.0–1.0)
- `tier_weight` = model tier multiplier (0.4–1.0)

---

## Source Trust

Each agent source can be assigned a trust weight:

```toml
[[sources]]
npub = "deadbeef..."
trust = 1.0  # self — highest trust

[[sources]]
npub = "aabbccdd..."
trust = 0.9  # trusted peer agent

[[sources]]
group = "dev-team"
trust = 0.7  # group-level trust
```

Unknown sources default to `0.0` trust (untrusted).

---

## Conflict Resolution

When multiple agents write to the same topic, conflicts are detected and resolved:

1. **Detection:** Same topic, different source pubkeys
2. **Resolution:** Ranked by `source_trust × tier_weight × recency`
3. **Winner:** Highest effective score wins
4. **Supersedes chain:** New version can reference old via `snow:supersedes` tag
