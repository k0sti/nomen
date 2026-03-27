# Consolidation

Consolidation converts high-volume collected messages into durable named memories. It runs periodically as a background operation or on demand.

The analogy is sleep: the hippocampus (collected messages) replays experiences to the neocortex (named memories), which integrates them into general knowledge.

```
Collected Messages (high volume, low individual value)
    │  consolidation pipeline
    ▼
Named Memories (low volume, high value, topic-keyed)
```

## Pipeline Stages

### 1. Collection

Query `collected_message` table for unconsolidated messages matching filters:

```sql
SELECT * FROM collected_message
WHERE consolidated = false
  AND created_at < $cutoff
  AND [platform/chat_id/thread_id filters when provided]
ORDER BY created_at ASC
LIMIT $batch_size
```

Filters: `#platform` (platform), `#community`, `#chat`, `#thread`, `since`, `older_than`.

### 2. Grouping

Messages are grouped by **scope + conversation container + time window**:

- **Scope:** Nostr-native boundary for the resulting memory visibility
- **Container:** Canonical `platform → community? → chat → thread?`
- **Time window:** 4-hour blocks (`TIME_WINDOW_SECS`)

Groups smaller than `min_messages` are skipped (picked up later when more messages accumulate).

```
Messages from k0, 14:00-18:00       → Group A (7 messages)
Messages in #techteam/topic:9225     → Group B (5 messages)
Messages in #techteam/topic:8485     → Group C (8 messages)
```

Forum/thread partitioning: platforms like Telegram forums partition on `thread_id` within a chat. Each topic consolidates independently.

### 3. Extraction (LLM)

Each group is converted to canonical `ConsolidationMessage` records and sent to the LLM provider for structured extraction.

The LLM produces:

```json
{
  "memories": [
    {
      "topic": "namespace/category",
      "content": "full memory text",
      "importance": 8
    }
  ]
}
```

**Topic namespace convention:**

```
user/<name>/<aspect>       Per-user knowledge
project/<name>/<aspect>    Project knowledge
group/<id>/<aspect>        Group context
fact/<domain>/<topic>      General knowledge
lesson/<slug>              Behavioral patterns
```

**LLM providers:**

| Provider | Use case |
|---|---|
| `OpenAiLlmProvider` | Production — real extraction |
| `NoopLlmProvider` | Testing — pass-through |

### 4. Storage

Each extracted memory becomes a kind 31234 replaceable event. The pipeline:

1. Checks for existing memory with the same topic d-tag
2. If exists: merges new information into existing, bumps version
3. If not: creates new memory
4. Checks embedding similarity to catch near-duplicates
5. Publishes to relay

**Visibility assignment** is derived from source messages:

- DM messages → `personal` or `private`
- Group messages → `group` (scoped to that group)
- Public/CLI → `public`

### 5. Graph Edges

Creates provenance and knowledge edges:

- `consolidated_from` — memory → source collected messages
- `mentions` — memory → extracted entities
- `related_to` — entity → entity (typed relationships)

### 6. Cleanup

1. Mark source messages `consolidated = true`
2. Publish NIP-09 deletion events for consumed Nostr events on relay
3. Batch deletions: max 50 event IDs per kind 5 event

## Two-Phase Consolidation

For external LLM processing (e.g. via MCP clients):

1. **`memory.consolidate_prepare`** — groups messages into batches, returns batch metadata and message content for external processing
2. **`memory.consolidate_commit`** — accepts extracted memories for each batch, runs storage/merge/graph/cleanup

Sessions have a TTL (default 60 minutes). Batches include canonical container metadata (`container`, `chat`, `thread`).

## Update & Merge

When consolidation finds new information about an existing topic:

1. Fetch existing memory by d-tag
2. Present old content + new messages to LLM for merge
3. Store merged result with bumped version
4. Create `references` edge with `relation: "contradicts"` if conflict detected

Newer information takes precedence (last-write-wins with audit trail).

## Decay & Pruning

### Access Tracking

Each search hit updates `last_accessed` and `access_count` on the memory record.

### Pruning Rules

| Condition | Action |
|---|---|
| `access_count = 0` AND age > 90 days | Delete |
| Duplicate topic, similar content | Merge into newer version |

### Recency Decay

Search ranking applies a recency factor based on time since last access:

```
recency = 1.0 - (days_since_access / max_age) × (1.0 - min_decay)
```

This affects retrieval ranking only — memories are not modified or deleted based on recency.

## Cluster Fusion

Periodic pipeline that groups memories by topic namespace prefix and synthesizes coherent summaries:

1. Group memories by prefix at configurable depth
2. Filter: clusters with ≥ min_members
3. LLM synthesis → coherent cluster summary
4. Store as `cluster/<prefix>` with `summarizes` edges

## Configuration

```toml
[memory.consolidation]
enabled = true
interval_hours = 4
ephemeral_ttl_minutes = 60
min_messages = 3
batch_size = 50
time_window_hours = 4
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"

[memory.cluster]
min_members = 3
namespace_depth = 2

[memory.pruning]
enabled = true
max_age_days = 90
```

## Graph-Aware Retrieval

Search supports graph expansion: traverse edges from direct hits to discover related memories.

Edge type weights:

| Edge Type | Weight |
|---|---|
| `contradicts` | 0.8 |
| `mentions` (shared entity) | 0.7 |
| `references` | 0.6 |
| `supersedes` | 0.5 |
| `consolidated_from` | 0.3 |

Contradictions are flagged with `contradicts: true` for downstream handling.
