# Memory Consolidation

**Version:** v0.1  
**Date:** 2026-03-04  
**Status:** Draft

Defines how raw conversation memories are consolidated into higher-quality named memories over time.

---

## Problem

Snowclaw's `auto_save` stores every message ≥20 chars as a separate kind 30078 event. A single conversation generates dozens of events like:

```
[private] conv:telegram_koshdot_42  "What's the weather?"
[private] conv:assistant_resp_telegram_koshdot_42  "It's -5°C in Helsinki..."
[private] conv:telegram_koshdot_43  "Thanks"
```

These raw events:
- Drown out meaningful memories in recall results
- Waste relay storage
- Have no topic structure — just message-level dumps
- Never get cleaned up

Meanwhile, the actually valuable information ("user is in Helsinki, cares about weather") is buried in the noise.

---

## Design

### Two Types of Memory

**Ephemeral memories** — raw conversation logs. High volume, low value individually. These are the `conv:*` and `group:*` auto-saved messages.

- D-tag pattern: `snow:memory:conv:*`, `snow:memory:group:<id>:*`
- Generated automatically by auto_save
- Short-lived — consolidated and replaced

**Named memories** — curated, topic-keyed knowledge. Low volume, high value. These are created by consolidation or directly by the agent.

- D-tag pattern: `snow:memory:<namespace>/<topic>` (e.g., `snow:memory:user/k0/preferences`)
- Created by consolidation pass or agent's `memory_store` tool
- Long-lived — updated in place via NIP-78 replaceable events
- Higher confidence scores

### Consolidation Process

Periodically (triggered by cron, heartbeat, or nomen CLI), the consolidation pass:

1. **Select** ephemeral memories older than a threshold (e.g., >1 hour, configurable)
2. **Group** by conversation context:
   - Private: group by sender npub + time window (e.g., 4-hour blocks)
   - Group: group by group_id + time window
3. **Summarize** each group using an LLM:
   - Extract facts, preferences, decisions, commitments
   - Assign a topic key (e.g., `user/k0/timezone`, `project/snowclaw/memory-design`)
   - Assess confidence
4. **Publish** named memory events (kind 30078) with proper topic d-tags
5. **Delete** the ephemeral source events (NIP-09 deletion events, kind 5)

### Named Memory Schema

Same kind 30078 event, but with structured topic keys:

```json
{
  "kind": 30078,
  "content": "{\"summary\":\"k0 is in Finland (Europe/Helsinki), prefers concise responses\",\"detail\":\"Extracted from Telegram DMs 2026-03-04. k0 asked about weather, timezone context confirmed.\",\"context\":\"consolidated from 12 ephemeral memories\"}",
  "tags": [
    ["d", "snow:memory:user/k0/preferences"],
    ["snow:tier", "private"],
    ["snow:model", "anthropic/claude-opus-4-6"],
    ["snow:confidence", "0.88"],
    ["snow:source", "<agent-pubkey>"],
    ["snow:version", "2"],
    ["snow:consolidated_from", "12"],
    ["snow:consolidated_at", "1772630000"],
    ["t", "user"],
    ["t", "preferences"]
  ]
}
```

### Topic Namespace Convention

```
user/<name>/<aspect>        — per-user knowledge
  user/k0/preferences
  user/k0/projects
  user/k0/schedule

project/<name>/<aspect>     — project knowledge
  project/snowclaw/architecture
  project/snowclaw/memory-design
  project/alhovuori/status

group/<id>/<aspect>         — group context
  group/techteam/purpose
  group/techteam/decisions

fact/<domain>/<topic>       — general knowledge
  fact/nostr/nip78-usage
  fact/rust/error-handling

lesson/<id>                 — behavioral patterns (alias for kind 4129)
```

### Updating Named Memories

Named memories are NIP-78 replaceable events — publishing a new event with the same `d` tag replaces the old one on the relay. The `snow:version` tag increments.

When new conversations add information to an existing topic:
1. Consolidation detects overlap with existing named memory
2. Merges new information into existing summary
3. Publishes updated event (same d-tag, bumped version)
4. Deletes the ephemeral sources

### Deletion of Ephemeral Events

After consolidation, source events are deleted using NIP-09:

```json
{
  "kind": 5,
  "tags": [
    ["e", "<ephemeral-event-id-1>"],
    ["e", "<ephemeral-event-id-2>"],
    ["a", "30078:<pubkey>:snow:memory:conv:telegram_koshdot_42"]
  ],
  "content": "consolidated"
}
```

Relays that support NIP-09 will remove the events. Local cache is cleaned up directly.

---

## Nomen CLI Commands

### `nomen consolidate`

Run consolidation pass manually:

```bash
# Consolidate all ephemeral memories older than 1 hour
nomen consolidate

# Dry run — show what would be consolidated without publishing
nomen consolidate --dry-run

# Custom time threshold
nomen consolidate --older-than 30m

# Consolidate specific tier only
nomen consolidate --tier private
nomen consolidate --tier group
```

### `nomen store`

Create/update a named memory directly:

```bash
# Create a public named memory
nomen store "project/snowclaw/overview" \
  --summary "Nostr-native AI agent system built on ZeroClaw" \
  --detail "Rust binary, NIP-29 groups, unified memory, relay sync..." \
  --tier public \
  --confidence 0.95

# Update existing (same d-tag = replace on relay)
nomen store "user/k0/preferences" \
  --summary "Prefers concise responses, timezone Europe/Helsinki" \
  --tier private
```

### `nomen delete`

Delete ephemeral or named memories:

```bash
# Delete all ephemeral memories older than 7 days
nomen delete --ephemeral --older-than 7d

# Delete a specific named memory
nomen delete "project/old-project/status"
```

### `nomen list` (updated)

```bash
# Show only named memories (skip ephemeral)
nomen list --named

# Show only ephemeral (pending consolidation)
nomen list --ephemeral

# Show consolidation stats
nomen list --stats
```

Output with stats:
```
Memory Events for npub1cg4d4...
═══════════════════════════════════════════

Named memories: 12 (8 public, 3 group, 1 private)
Ephemeral memories: 147 (pending consolidation)
Last consolidation: 2026-03-04 12:00:00 UTC

[public] project/snowclaw/overview (v2, confidence: 0.95)
  Summary: Nostr-native AI agent system built on ZeroClaw
  Consolidated from: 24 messages, last updated 2h ago

[private] user/k0/preferences (v4, confidence: 0.88)
  Summary: Finland, concise style, uses Telegram
  Consolidated from: 56 messages, last updated 30m ago
```

---

## Snowclaw Integration

### Auto-consolidation

Snowclaw triggers consolidation via:

1. **Cron job** — every N hours (configurable)
2. **Heartbeat** — check if consolidation is due during heartbeat
3. **Threshold** — when ephemeral count exceeds N (e.g., 100)

### Config

```toml
[memory.consolidation]
enabled = true
interval_hours = 4           # run every 4 hours
ephemeral_ttl_minutes = 60   # consolidate messages older than 1h
max_ephemeral_count = 200    # force consolidation above this count
dry_run = false              # set true to test without publishing
```

### Memory Store Tool

The agent's `memory_store` tool should prefer named topics:

```
Agent: I'll remember that. [calls memory_store with key "user/k0/timezone", value "Europe/Helsinki"]
```

Instead of the current behavior where it stores with a random UUID key.

---

## Event Lifecycle

```
User message arrives
  → auto_save creates ephemeral event (conv:telegram_koshdot_42)
  → published to relay as kind 30078

... time passes, more messages ...

Consolidation triggers
  → groups 15 ephemeral events from last 4h DM session
  → LLM extracts: "k0 discussed Snowclaw memory tiers, decided on 3-tier model"
  → publishes named event: snow:memory:project/snowclaw/memory-tiers (kind 30078)
  → deletes 15 ephemeral events (kind 5)

Next conversation
  → recall("memory tiers") finds the named memory (high relevance, high confidence)
  → instead of 15 raw message fragments
```

---

## Open Questions

1. **Consolidation model** — Use the same model as the agent, or a cheaper/faster model? Consolidation is background work, could use a tier-2 model.

2. **Cross-tier consolidation** — Can private ephemeral memories consolidate into group-tier named memories? Probably not by default (privacy leak). Require explicit promotion.

3. **Multi-agent consolidation** — When multiple agents share a relay, should consolidation merge across agents? Probably only within trust boundaries.

4. **Relay-side cleanup** — Not all relays support NIP-09 deletion. Ephemeral events may linger on non-compliant relays. The `snow:consolidated_from` tag on named memories indicates the sources are superseded.
