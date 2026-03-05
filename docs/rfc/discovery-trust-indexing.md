# RFC: Discovery & Trust Event Indexing in Nomen

**Version:** v0.1
**Date:** 2026-03-05
**Status:** Draft
**Authors:** k0sh, Clarity
**Context:** Nomen — Nostr-native memory CLI

---

## Summary

Extend Nomen beyond memory events (kind 30078) to index agent capability announcements (kind 38990) and trust attestations (kind 1985). This turns Nomen from a personal memory tool into an **agent-aware knowledge index** — search not just what you know, but who can do what and how much they're trusted.

## Motivation

Nomen already syncs NIP-78 memory events from relays to a local SurrealDB cache with BM25 search. The same infrastructure can index two additional event kinds that are emerging in the agent ecosystem:

1. **Kind 38990** (Agent Capability Discovery) — agents advertising skills, pricing, availability
2. **Kind 1985** (NIP-32 Labels / Trust Attestations) — agents attesting to each other's work quality

With these indexed locally, a single `nomen search` query can answer: "Who can review Rust code, has a trust score above 30, and charges less than 100 sats?"

## New Event Kinds

### Kind 38990 — Agent Capabilities

Published by agents to advertise what they can do. Parameterized replaceable (NIP-33), keyed by `d` tag.

**Schema (as indexed by Nomen):**

```
Table: capabilities
├── pubkey       TEXT (agent's hex pubkey)
├── service_id   TEXT (d-tag value)
├── name         TEXT
├── about        TEXT
├── capabilities TEXT[] (from #c tags)
├── hashtags     TEXT[] (from #t tags)
├── model        TEXT
├── runtime      TEXT
├── status       TEXT (active|inactive|maintenance)
├── price_amount INTEGER
├── price_unit   TEXT
├── price_per    TEXT
├── ln_address   TEXT
├── dvm_kinds    INTEGER[]
├── relays       TEXT[] (from #r tags)
├── content_json TEXT (raw content field)
├── created_at   INTEGER
├── event_id     TEXT (hex event id)
└── synced_at    INTEGER
```

**FTS5 index on:** `name`, `about`, `capabilities` (joined), `hashtags` (joined)

### Kind 1985 — Trust Attestations

Published by agents/humans to attest to another agent's work. NIP-32 label events.

**Relevant tag structure:**
```json
{
  "kind": 1985,
  "tags": [
    ["L", "ai.wot"],
    ["l", "work-completed", "ai.wot"],
    ["p", "<attested-pubkey>"],
    ["e", "<related-event-id>"]
  ],
  "content": "Completed code review accurately and on time"
}
```

**Schema (as indexed by Nomen):**

```
Table: attestations
├── pubkey        TEXT (attester's hex pubkey)
├── target_pubkey TEXT (attested agent, from #p tag)
├── label_ns      TEXT (L-tag namespace, e.g., "ai.wot")
├── label_value   TEXT (l-tag value, e.g., "work-completed")
├── related_event TEXT (e-tag, optional)
├── content       TEXT (free-text attestation)
├── created_at    INTEGER
├── event_id      TEXT
└── synced_at     INTEGER
```

**FTS5 index on:** `content`, `label_value`

## CLI Extensions

### `nomen agents` — Query agent capabilities

```bash
# List all known agents
nomen agents list

# Find agents by capability
nomen agents find --capability code-review

# Find agents by capability + trust threshold
nomen agents find --capability code-review --min-trust 30

# Show a specific agent's full profile
nomen agents show <npub|hex>

# Sync capability events from relay
nomen agents sync
```

### `nomen trust` — Query and manage trust

```bash
# Show trust score for an agent
nomen trust score <npub|hex>

# Show trust attestations for an agent
nomen trust attestations <npub|hex>

# Show trust path between two agents
nomen trust path <from-npub> <to-npub>

# Publish a trust attestation
nomen trust attest <npub|hex> --type work-completed --content "Delivered on time"

# Sync trust events from relay
nomen trust sync
```

### Extended `nomen search` — Unified search

```bash
# Search across all indexed types (memories + capabilities + attestations)
nomen search "rust code review"

# Search only capabilities
nomen search "rust code review" --type capabilities

# Search with trust filter
nomen search "text generation" --min-trust 20
```

## Sync Behavior

### Capability Events (kind 38990)

- **Initial sync:** Fetch all kind 38990 from configured relays
- **Incremental:** Track `since` timestamp per relay
- **Dedup:** Parameterized replaceable — upsert on `(pubkey, service_id)`
- **Eviction:** Mark `inactive` if event older than 30 days without update (configurable)

### Trust Attestations (kind 1985)

- **Filter:** Only sync events with `["L", "ai.wot"]` tag (avoid unrelated NIP-32 labels)
- **Incremental:** Track `since` per relay
- **Dedup:** By event_id (not replaceable)
- **Aggregation:** Compute trust scores locally using same formula as ai-wot:
  - Base: count of unique attesters
  - Weighted by: attester's own score (recursive, max depth 3)
  - Decay: attestations older than 90 days get 0.5× weight
  - Zap bonus: attestations with zap receipts get 1.5× weight

### Trust Score Computation

```
trust_score(pubkey) =
  Σ (attestation_weight × attester_influence × recency_decay)

attestation_weight:
  work-completed  = 1.2
  service-quality = 1.0
  identity        = 0.8

attester_influence:
  min(trust_score(attester) / 100, 1.0)  // recursive, cached, max depth 3
  unknown attester = 0.1

recency_decay:
  age < 30 days  = 1.0
  age < 90 days  = 0.75
  age < 180 days = 0.5
  age > 180 days = 0.25
```

Score is normalized to 0-100 range.

## SurrealDB Schema

```surql
-- Capability announcements
DEFINE TABLE capabilities SCHEMAFULL;
DEFINE FIELD pubkey ON capabilities TYPE string;
DEFINE FIELD service_id ON capabilities TYPE string;
DEFINE FIELD name ON capabilities TYPE string;
DEFINE FIELD about ON capabilities TYPE string;
DEFINE FIELD capabilities ON capabilities TYPE array<string>;
DEFINE FIELD model ON capabilities TYPE option<string>;
DEFINE FIELD runtime ON capabilities TYPE option<string>;
DEFINE FIELD status ON capabilities TYPE string DEFAULT "active";
DEFINE FIELD price_amount ON capabilities TYPE option<int>;
DEFINE FIELD ln_address ON capabilities TYPE option<string>;
DEFINE FIELD dvm_kinds ON capabilities TYPE array<int>;
DEFINE FIELD relays ON capabilities TYPE array<string>;
DEFINE FIELD content_json ON capabilities TYPE option<string>;
DEFINE FIELD created_at ON capabilities TYPE int;
DEFINE FIELD event_id ON capabilities TYPE string;
DEFINE FIELD synced_at ON capabilities TYPE int;

DEFINE INDEX idx_cap_pubkey ON capabilities FIELDS pubkey;
DEFINE INDEX idx_cap_unique ON capabilities FIELDS pubkey, service_id UNIQUE;
DEFINE INDEX idx_cap_status ON capabilities FIELDS status;

DEFINE ANALYZER cap_analyzer TOKENIZERS blank, class FILTERS lowercase, snowball(english);
DEFINE INDEX idx_cap_search ON capabilities
  FIELDS name, about SEARCH ANALYZER cap_analyzer BM25;

-- Trust attestations
DEFINE TABLE attestations SCHEMAFULL;
DEFINE FIELD pubkey ON attestations TYPE string;
DEFINE FIELD target_pubkey ON attestations TYPE string;
DEFINE FIELD label_ns ON attestations TYPE string;
DEFINE FIELD label_value ON attestations TYPE string;
DEFINE FIELD related_event ON attestations TYPE option<string>;
DEFINE FIELD content ON attestations TYPE string;
DEFINE FIELD created_at ON attestations TYPE int;
DEFINE FIELD event_id ON attestations TYPE string ASSERT $value != NONE;
DEFINE FIELD synced_at ON attestations TYPE int;

DEFINE INDEX idx_att_event ON attestations FIELDS event_id UNIQUE;
DEFINE INDEX idx_att_target ON attestations FIELDS target_pubkey;
DEFINE INDEX idx_att_pubkey ON attestations FIELDS pubkey;

DEFINE ANALYZER att_analyzer TOKENIZERS blank, class FILTERS lowercase, snowball(english);
DEFINE INDEX idx_att_search ON attestations
  FIELDS content, label_value SEARCH ANALYZER att_analyzer BM25;

-- Computed trust scores (cached, recomputed on sync)
DEFINE TABLE trust_scores SCHEMAFULL;
DEFINE FIELD pubkey ON trust_scores TYPE string;
DEFINE FIELD score ON trust_scores TYPE float;
DEFINE FIELD attestation_count ON trust_scores TYPE int;
DEFINE FIELD unique_attesters ON trust_scores TYPE int;
DEFINE FIELD categories ON trust_scores TYPE object;
DEFINE FIELD computed_at ON trust_scores TYPE int;

DEFINE INDEX idx_trust_pubkey ON trust_scores FIELDS pubkey UNIQUE;
```

## Config Extension

```toml
# ~/.config/nomen/config.toml

relay = "wss://zooid.atlantislabs.space"
nsec = "nsec1..."

[discovery]
enabled = true
# Additional relays to scan for agent capabilities
relays = [
  "wss://relay.damus.io",
  "wss://nos.lol"
]
# Auto-sync interval (seconds, 0 = manual only)
sync_interval = 3600
# Minimum trust score to include in search results
default_min_trust = 0

[trust]
enabled = true
# Namespace filter for attestations
namespace = "ai.wot"
# Score computation parameters
max_recursion_depth = 3
decay_days = [30, 90, 180]
decay_weights = [1.0, 0.75, 0.5, 0.25]
```

## Implementation Phases

### Phase 1: Read-only indexing
- Sync kind 38990 + 1985 from relays
- Store in SurrealDB tables
- `nomen agents list/find/show`
- `nomen trust score/attestations`
- Extended `nomen search` with `--type`

### Phase 2: Trust computation
- Local trust score calculation
- Score caching in `trust_scores` table
- `--min-trust` filter on all queries
- Trust path discovery

### Phase 3: Publishing
- `nomen trust attest` — publish attestations
- Auto-attest after successful DVM interactions
- Integrate with Snowclaw's job completion flow

### Phase 4: Unified search
- Cross-type search: memories + capabilities + trust in one query
- Ranked by: BM25 relevance × trust score × recency
- Agent recommendations: "for this task, try these agents"

## Compatibility

- **Jeletor `agent-discovery`**: Same kind 38990, same `#c` tag convention. Nomen can index Jeletor-published capabilities.
- **Jeletor `ai-wot`**: Same kind 1985 with `["L", "ai.wot"]` namespace. Same scoring formula (intentionally aligned).
- **Snowclaw capability events**: Indexed identically. Snowclaw publishes, Nomen indexes.
- **NIP-90 DVM**: The `dvm` tags in capabilities map directly to DVM job kinds.

## Relationship to Existing Nomen

```
Current Nomen:
  kind 30078 (memories) → SurrealDB → BM25 search

Extended Nomen:
  kind 30078 (memories)    ─┐
  kind 38990 (capabilities) ├→ SurrealDB → unified BM25 search + trust scoring
  kind 1985  (attestations) ─┘
```

The existing memory sync, search, and CLI patterns extend naturally. No architectural changes — just new tables, new event kinds in the sync filter, and new CLI subcommands.

---

*Start with Phase 1 (read-only indexing). Real usage will reveal what Phase 2-4 actually need.*
