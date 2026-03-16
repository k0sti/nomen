# Memory Consolidation Specification

**Version:** v1.0  
**Date:** 2026-03-06  
**Status:** Active

Defines how Nomen converts raw ingested messages into structured, high-quality named memories. Based on research from Complementary Learning Systems theory, Mem0, A-MEM, and the "Memory in the Age of AI Agents" survey (Hu et al., 2025).

---

## 1. Overview

Raw messages flood in from conversations — Telegram DMs, NIP-29 groups, CLI input. Most are noise. Consolidation is the process of extracting signal: facts, preferences, decisions, lessons. It runs periodically as a background operation, transforming ephemeral traces into durable knowledge.

The analogy is sleep: the hippocampus (raw message store) replays experiences to the neocortex (named memory store), which integrates them into general knowledge.

```
Raw Messages (high volume, low individual value)
    │
    │  consolidation pipeline
    ▼
Named Memories (low volume, high value, topic-keyed)
```

## 2. Memory Types

### Ephemeral Memories (raw messages)

Stored in the `raw_message` table. Created by `nomen ingest` or auto-save from agent frameworks.

- **Canonical identity:** underlying Nostr event id + timestamp
- **Grouping fields:** `channel` (provider/container identity) and resolved `scope`
- **Lifecycle:** Short — consumed by consolidation, then deleted or marked superseded
- **Source:** Auto-save, ingest command, webhook

### Named Memories (consolidated)

Stored in the `memory` table as kind 31234 events. Created by consolidation or direct `nomen store`.

- **D-tag pattern:** `<namespace>/<topic>`
- **Lifecycle:** Long-lived — updated in place via replaceable event semantics (kind 31234)
- **Source:** Consolidation pipeline, agent's `memory_store` tool, manual CLI

## 3. Topic Namespace Convention

Topics use forward-slash hierarchy. The namespace encodes what kind of knowledge it is:

```
user/<name>/<aspect>          Per-user knowledge
  user/k0/preferences           Style, language, timezone
  user/k0/projects              What they're working on
  user/k0/schedule              Routines, availability

project/<name>/<aspect>       Project knowledge
  project/snowclaw/architecture  System design
  project/nomen/consolidation    This very feature
  project/alhovuori/status       Current state

group/<id>/<aspect>           Group context
  group/techteam/purpose         Why the group exists
  group/techteam/decisions       Key decisions made

fact/<domain>/<topic>         General knowledge
  fact/nostr/nip78-usage         Protocol knowledge
  fact/rust/error-handling       Language patterns

lesson/<slug>                 Behavioral patterns
  lesson/always-check-relay-auth Learned the hard way
```

Rules:
- Lowercase, alphanumeric + hyphens + underscores
- Max 3 levels deep (namespace/category/detail)
- LLM derives topic during consolidation; falls back to `conversation/<channel>` if unsure

## 4. Pipeline Stages

### Stage 1: Collection

Query `raw_message` table for unconsolidated messages matching filters:

| Filter | CLI Flag | Default |
|--------|----------|---------|
| Age threshold | `--older-than` | `60m` |
| Tier | `--tier` | all |
| Batch size | `--batch-size` | `50` |
| Minimum messages | `--min-messages` | `3` |

```sql
SELECT * FROM raw_message
WHERE consolidated = false
  AND created_at < $cutoff
ORDER BY created_at ASC
LIMIT $batch_size
```

### Stage 2: Grouping

Messages are grouped by **scope/channel identity + time window** before LLM processing:

- **Scope:** Nostr-native durable boundary used for resulting memory visibility
- **Channel:** concrete conversation container the messages came from
- **Sub-channel:** forum topic or thread ID extracted from sender metadata (e.g. `topic:9225`)
- **Time window:** 4-hour blocks (configurable via `TIME_WINDOW_SECS`)
- Groups smaller than `min_messages` are skipped (will be picked up in a later run when more messages accumulate)

```
Messages from k0, 14:00-18:00       → Group A (7 messages)
Messages from k0, 18:00-22:00       → Group B (4 messages)
Messages in #techteam/topic:9225     → Group C (5 messages)
Messages in #techteam/topic:8485     → Group D (8 messages)
Messages in #techteam/topic:8399     → Group E (3 messages)
```

**Forum topic partitioning:** Platforms like Telegram forums have multiple topics within a single chat. All topics share the same `channel` but the sender field encodes the topic (e.g. `telegram:group:-1003821690204:topic:9225`). The grouping logic extracts the topic suffix and appends it to the identity key, ensuring each forum topic is consolidated independently. This prevents unrelated conversations from being merged into the same memory batch.

### Stage 3: Extraction (LLM)

Each group is sent to the configured LLM provider for structured extraction.

**System prompt:**
```
You are a memory consolidation agent. Given a batch of raw messages,
extract significant facts, decisions, and context into structured memories.

Return JSON: {"memories": [{"topic": "namespace/category", "summary": "one line",
"detail": "full context", "confidence": 0.8}]}

Topic conventions:
- user/<name>/<aspect> for per-user knowledge
- project/<name>/<aspect> for project knowledge  
- group/<id>/<aspect> for group context
- fact/<domain>/<topic> for general knowledge

Only extract genuinely significant information. Skip greetings, filler, and
already-known facts. Confidence: 0.5 (uncertain) to 1.0 (definitive).
Return empty array if nothing significant.
```

**LLM providers:**

| Provider | Config key | Use case |
|----------|-----------|----------|
| `OpenAiLlmProvider` | `openai` / `openrouter` | Production — real extraction |
| `NoopLlmProvider` | (fallback) | Testing — pass-through summary |

**Model selection:** Consolidation is background work. Use a capable but cost-effective model (e.g., `claude-sonnet-4-6`, `gpt-4o-mini`). Configured in `[memory.consolidation]`.

### Stage 4: Storage

Each extracted memory becomes a kind 31234 replaceable event:

```json
{
  "kind": 31234,
  "content": "{\"summary\":\"k0 prefers concise responses\",\"detail\":\"...\"}",
  "tags": [
    ["d", "user/k0/preferences"],
    ["tier", "private"],
    ["model", "anthropic/claude-sonnet-4-6"],
    ["confidence", "0.88"],
    ["source", "<agent-pubkey>"],
    ["version", "1"],
    ["consolidated_from", "7"],
    ["consolidated_at", "1753000000"],
    ["t", "user"],
    ["t", "preferences"]
  ]
}
```

**Scope/visibility assignment:** Derived from source messages' resolved scope:
- DM messages → `personal` or `internal` depending on memory class
- Group messages → `group` (scoped to that group)
- Public/CLI → `public`

Provider-specific channel/container identifiers remain provenance metadata; they do not become part of the durable memory d-tag.

**Deduplication:** Before creating a new memory, check if a memory with the same topic d-tag already exists. If it does:
1. Merge the new information into the existing memory
2. Increment `version`
3. Republish (kind 31234 replaces by d-tag)

This is the **update path** — the most important part of keeping memories current without duplicating.

### Stage 5: Graph Edges

Create `consolidated_from` edges linking the new memory to its source messages:

```sql
RELATE $memory_id->consolidated_from->$raw_message_id
  SET created_at = time::now();
```

Preserves provenance — trace any named memory back to the raw conversation that produced it.

**Entity edges:** During consolidation, the entity extractor (heuristic or LLM-powered `CompositeExtractor`) produces both entities and typed relationships. Entities get `mentions` edges to the memory. Relationships between entities create `related_to` edges with typed relations (e.g. `works_on`, `collaborates_with`, `manages`, `contradicts`). These typed edges are used by graph-aware retrieval to surface related context.

### Stage 6: Cleanup

1. **Mark consolidated:** Set `consolidated = true` on all source `raw_message` records
2. **NIP-09 deletion:** Publish kind 5 events to delete consumed ephemeral events from the relay
3. **Batch deletions:** Max 50 event IDs per deletion event

```json
{
  "kind": 5,
  "tags": [["e", "<id1>"], ["e", "<id2>"], ...],
  "content": "consolidated"
}
```

Note: Not all relays honor NIP-09. The `consolidated_from` edges on named memories serve as a secondary indicator that sources are superseded.

## 5. Update & Merge Strategy

When consolidation finds new information about an existing topic:

### Same Topic Exists

1. Fetch existing memory by d-tag
2. Present both old content and new messages to LLM:
   ```
   Existing memory: {summary, detail}
   New messages: [transcript]
   
   Merge the new information into the existing memory. 
   Update what changed. Keep what's still true. Note conflicts.
   ```
3. Store merged result with bumped version

### Conflict Detection

When new information contradicts existing memories:

1. LLM flags the contradiction in extraction
2. Create a `references` edge with `relation: "contradicts"`
3. The newer information takes precedence (last-write-wins with audit trail)
4. Optionally: keep both versions accessible via the graph

## 6. Decay & Pruning

### Access Tracking

Each memory search hit updates `last_accessed` and `access_count`:

```sql
UPDATE memory SET 
  last_accessed = time::now(),
  access_count += 1
WHERE id = $id;
```

### Pruning Rules

| Condition | Action |
|-----------|--------|
| `access_count = 0` AND age > 90 days | Delete |
| `confidence < 0.3` AND age > 30 days | Delete |
| `access_count = 0` AND `confidence < 0.5` AND age > 30 days | Delete |
| Duplicate topic (same namespace, similar content) | Merge into highest-confidence version |

Run via `nomen prune [--days 30] [--dry-run]`.

### Confidence Decay

Memories lose confidence over time if not accessed:

```
effective_confidence = confidence × decay_factor(age, access_count)
decay_factor = 1.0 - (days_since_access / max_age) × (1.0 - min_decay)
```

Affects retrieval ranking, not storage.

## 7. Retrieval Scoring

Search results ranked by composite score:

```
score = semantic_similarity × 0.4
      + text_match × 0.3
      + recency × 0.15
      + importance × 0.15
```

Named memories naturally score higher than unconsolidated raw messages due to higher confidence and focused content.

## 8. Configuration

```toml
[memory.consolidation]
enabled = true
interval_hours = 4              # auto-trigger interval
ephemeral_ttl_minutes = 60      # consolidate messages older than this
max_ephemeral_count = 200       # force consolidation above this count
min_messages = 3                # minimum per group to trigger
batch_size = 50                 # max messages per run
time_window_hours = 4           # grouping window
dry_run = false

# LLM for extraction
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
api_key_env = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"

[memory.pruning]
enabled = true
max_age_days = 90
min_confidence = 0.3
```

## 9. CLI Interface

```bash
# Run consolidation
nomen consolidate
nomen consolidate --dry-run
nomen consolidate --older-than 30m --tier private
nomen consolidate --batch-size 100

# Prune old/unused memories
nomen prune --days 30 --dry-run
nomen prune --days 90

# View consolidation state
nomen list --stats
nomen list --named
nomen list --ephemeral
```

## 10. API Interface

### HTTP

```
POST /memory/api/consolidate
Body: { "older_than": "1h", "tier": "private", "dry_run": false }
Response: { "messages_processed": 47, "memories_created": 5, "events_deleted": 47 }
```

### MCP

```json
Tool: memory_consolidate
Input: { "older_than": "1h", "tier": "private" }
Output: { "messages_processed": 47, "memories_created": 5 }
```

## 11. Implementation Status

### Done ✅

- `consolidate.rs` — Core pipeline (collection → grouping → extraction → storage → cleanup)
- `OpenAiLlmProvider` — Real LLM extraction via OpenAI-compatible API
- `NoopLlmProvider` — Fallback pass-through for testing
- NIP-09 deletion of consumed ephemerals
- `consolidated_from` graph edges for provenance
- CLI: `nomen consolidate [--dry-run] [--older-than] [--tier] [--batch-size]`
- HTTP: `POST /memory/api/consolidate`
- MCP: `memory_consolidate` tool
- Config: `[memory.consolidation]` section in TOML

### TODO 📋

- [x] **Tier derivation from source context** — `private` for DM sources, `group` for group sources.
- [x] **Merge into existing memories** — When topic d-tag already exists, merge instead of creating duplicate.
- [x] **Conflict detection** — Flag contradictions between new and existing memories. Create `contradicts` graph edges.
- [x] **Access tracking** — `last_accessed` and `access_count` fields on memory records. Updated on search hits.
- [x] **Confidence decay** — Time-based decay factor in retrieval scoring.
- [x] **Pruning command** — `nomen prune` to delete unaccessed/low-confidence memories.
- [x] **Importance scoring at creation** — LLM assigns importance (1-10) during extraction, stored alongside confidence.
- [x] **Deduplication pass** — Embedding similarity check before creating new memories to catch near-duplicates.
- [x] **Entity extraction during consolidation** — Extract entities from consolidated content, create `mentions` edges.
- [x] **Auto-trigger** — `check_consolidation_due()` checks interval_hours and max_ephemeral_count. HTTP GET `/consolidate/status`. Meta table tracks last run.
- [x] **Cross-group consolidation guard** — Prevent private ephemeral memories from leaking into group-tier named memories.
- [x] **Aggregated search results** — Post-retrieval merging of semantically similar hits (>0.85 cosine) into coherent summaries. CLI `--aggregate` flag.
- [x] **Graph-aware retrieval** — Post-search graph traversal surfaces related memories via `mentions`, `references`, `contradicts`, and `consolidated_from` edges. CLI `--graph` / `--hops`, MCP `graph_expand` param.
- [x] **LLM entity extraction** — `EntityExtractor` trait with `HeuristicExtractor`, `LlmEntityExtractor`, `CompositeExtractor`. Typed relationships stored as `related_to` edges. Config: `[entities]` section.
- [x] **Cluster fusion** — Namespace-grouped memory synthesis. Groups memories by topic prefix, LLM produces coherent summaries stored as `cluster/<prefix>` with `summarizes` edges. CLI `nomen cluster`, config `[memory.cluster]`.

## 12. Future: Dream Cycle

Beyond structured consolidation (NREM-equivalent), a creative associative pass (REM-equivalent) can discover latent connections between memories. See `obsidian/03-06 Dreaming & Sleep-Inspired Memory.md` for the full design.

**Prerequisites completed:** Graph-aware retrieval (Feature 1), LLM entity extraction (Feature 2), and cluster fusion (Feature 3) are all implemented. These provide the rich graph structure and cluster summaries that the dream cycle needs for effective latent connection discovery. See [graph-and-consolidation-roadmap.md](graph-and-consolidation-roadmap.md) for the full roadmap — Feature 4 (Dream Cycle) is the remaining work.

---

## References

- Hu et al. (2025). "Memory in the Age of AI Agents: A Survey." arxiv.org/abs/2512.13564
- A-MEM: Zettelkasten-inspired dynamic memory. arxiv.org/abs/2502.12110
- Mem0: Production LLM memory pipeline. arxiv.org/abs/2504.19413
- McClelland et al. Complementary Learning Systems (CLS) Theory
- González et al. (2022). "Sleep-like unsupervised replay." Nature Communications
- Park et al. (2023). "Generative Agents." Stanford

---

*Internal docs with more research context:*
- `obsidian/03-05 Memory Consolidation.md` — Original design notes
- `obsidian/03-05 Memory Survey - Nomen Reflections.md` — Survey analysis
- `obsidian/03-05 Agentic Memory Landscape.md` — Research landscape
- `obsidian/03-06 Dreaming & Sleep-Inspired Memory.md` — Dream cycle design
- `obsidian/Consolidation Spec Draft.md` — Earlier draft (superseded by this doc)
