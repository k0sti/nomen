# Graph Retrieval & Advanced Consolidation — Implementation Roadmap

**Date:** 2026-03-11  
**Status:** Features 1-3 Implemented ✅ · Feature 4 Design  
**Source:** Gap analysis vs "Memory in the Age of AI Agents" (Hu et al., arXiv:2512.13564)

---

## Priority Order

| # | Feature | Effort | Status | Enables |
|---|---------|--------|--------|---------|
| 1 | Graph-aware retrieval | Medium | ✅ Implemented | Multi-hop reasoning, context discovery |
| 2 | LLM entity extraction | Medium | ✅ Implemented | Richer graph, better linking |
| 3 | Cluster fusion | Medium | ✅ Implemented | Scalable memory, reduced noise |
| 4 | Dream cycle (Phase 2) | Large | 📋 Design | Latent connection discovery |

---

## 1. Graph-Aware Retrieval

### Problem

Nomen has graph edges in SurrealDB (`mentions`, `references`, `consolidated_from`, `related_to`) but search only uses vector+BM25. The graph is write-only — provenance is recorded but never exploited for retrieval. (Contradictions are stored as `references` edges with `relation: "contradicts"` rather than a separate edge table.)

When an agent searches for "Alhovuori business plan", it finds the memory about the business plan but NOT the related memories about Tommi Ullgrén (linked via `mentions`), the property holding model (linked via `references`), or the contradicted earlier revenue estimate (linked via `references` with `relation: "contradicts"`). The agent loses relational context.

### What This Enables

- **Multi-hop reasoning** — "Who is involved in the Alhovuori project?" traverses memory→mentions→entity→mentions→memory to find all related people and their context
- **Contradiction surfacing** — When retrieving a memory, automatically surface any memories linked via `references` edges where `relation = "contradicts"`, so the agent sees conflicting information
- **Provenance exploration** — "Where did this knowledge come from?" walks `consolidated_from` edges back to source messages
- **Related topic discovery** — "What else is relevant?" follows `references` and `related_to` edges to find adjacent knowledge the agent didn't explicitly search for
- **Entity-centric views** — "Everything about k0" traverses all `mentions` edges pointing to the k0 entity, returning a coherent profile assembled from many memories

### Design

Add a `graph_expand` post-processing step after hybrid search:

```
hybrid_search(query)
  → top-K results
  → for each result, traverse 1-hop graph edges
  → score expanded results by edge type + distance
  → merge into final ranked list (dedup by d_tag)
```

#### Edge traversal weights

| Edge Type | Direction | Weight | Rationale |
|-----------|-----------|--------|-----------|
| `mentions` | memory→entity→memory | 0.7 | Entity co-occurrence is strong signal |
| `references` (supports) | memory→memory | 0.6 | Supporting evidence |
| `references` (contradicts) | memory→memory | 0.8 | Conflicts are critical context (stored as `references` edge with `relation: "contradicts"`) |
| `consolidated_from` | memory→raw_message | 0.3 | Provenance, lower relevance |
| `related_to` | entity→entity | 0.5 | Indirect association |

#### Implementation

**File:** `src/search.rs`

```rust
/// Expand search results by traversing graph edges.
async fn graph_expand(
    db: &Surreal<Db>,
    results: &[SearchResult],
    max_hops: usize,  // default: 1
    max_expanded: usize,  // default: 5 per result
) -> Result<Vec<SearchResult>> {
    let mut expanded = Vec::new();
    let mut seen_dtags: HashSet<String> = results.iter()
        .filter_map(|r| r.d_tag.clone()).collect();

    for result in results {
        let Some(ref d_tag) = result.d_tag else { continue };

        // 1. Follow mentions edges: memory→entity→memory
        let mentioned_memories = db.query(
            "SELECT <-mentions<-memory.* AS mems FROM entity
             WHERE ->mentions->memory.d_tag CONTAINS $d_tag
             LIMIT $limit"
        ).bind(("d_tag", d_tag)).bind(("limit", max_expanded))
         .await?;

        // 2. Follow references edges: memory→memory
        let referenced = db.query(
            "SELECT ->references->memory.* AS refs,
                    <-references<-memory.* AS back_refs
             FROM memory WHERE d_tag = $d_tag"
        ).bind(("d_tag", d_tag)).await?;

        // 3. Follow contradicts relations on references edges (high priority)
        let contradictions = db.query(
            "SELECT ->references[WHERE relation = 'contradicts']->memory.* AS conflicts
             FROM memory WHERE d_tag = $d_tag"
        ).bind(("d_tag", d_tag)).await?;

        // Score and dedup expanded results
        // ...
    }

    Ok(expanded)
}
```

**New `SearchOptions` field:**
```rust
pub struct SearchOptions {
    // ... existing fields ...
    /// Enable graph expansion (traverse edges from results).
    pub graph_expand: bool,
    /// Max hops for graph traversal (default: 1).
    pub max_hops: usize,
}
```

**CLI:**
```bash
nomen search "Alhovuori" --graph          # enable graph expansion
nomen search "Alhovuori" --graph --hops 2 # 2-hop traversal
```

**MCP:** Add `graph_expand: bool` to `nomen_search` tool input schema.

#### SurrealDB Queries (key patterns)

Entity-centric lookup (all memories mentioning an entity):
```sql
SELECT <-mentions<-memory.* FROM entity WHERE name = $name
```

Contradiction chain:
```sql
SELECT ->references[WHERE relation = 'contradicts']->memory.* FROM memory WHERE d_tag = $dtag
```

Related memories via shared entities:
```sql
SELECT <-mentions<-memory.* FROM entity
WHERE <-mentions<-memory CONTAINS (SELECT id FROM memory WHERE d_tag = $dtag)
AND <-mentions<-memory.d_tag != $dtag
```

### Acceptance Criteria

- [x] `nomen search "X" --graph` returns results from graph traversal alongside vector+BM25 hits
- [x] Contradicting memories are surfaced with a `[CONTRADICTS]` marker
- [x] Entity-centric queries return all memories mentioning that entity (via shared-entity traversal)
- [x] MCP `nomen_search` supports `graph_expand` and `max_hops` parameters
- [x] Graph-expanded results are scored lower than direct hits (parent score × edge type weight)

---

## 2. LLM Entity Extraction

### Problem

Current entity extraction (`entities.rs`) is heuristic: @mentions, capitalized phrases, known entity matching. This misses:
- Entities mentioned in natural language without capitalization ("talked to my colleague about the relay")
- Relationships between entities ("k0 hired Tommi to handle finances")  
- Typed relations (works_on, collaborates_with, hired_by)
- Concept entities ("NIP-44 encryption", "memory consolidation")

### What This Enables

- **Richer knowledge graph** — more entities = more edges = better graph retrieval (Feature 1)
- **Typed relationships** — not just "mentions" but "works_on", "decided_to", "collaborates_with"
- **Concept tracking** — technical concepts, decisions, and patterns as first-class entities
- **Relationship reasoning** — "Who works on what?" becomes a graph query, not a text search
- **Automated knowledge base** — over time, the entity graph becomes a structured representation of the agent's world model

### Design

Add an `LlmEntityExtractor` that runs during consolidation (alongside or replacing the heuristic extractor).

**System prompt:**
```
Extract entities and relationships from this memory.

Return JSON:
{
  "entities": [
    {"name": "k0", "kind": "person", "attributes": {"role": "owner"}},
    {"name": "Nomen", "kind": "project", "attributes": {"language": "rust"}}
  ],
  "relationships": [
    {"from": "k0", "to": "Nomen", "relation": "works_on", "detail": "primary developer"},
    {"from": "Tommi", "to": "Alhovuori", "relation": "manages_finances", "detail": "handles business plan"}
  ]
}

Entity kinds: person, project, concept, place, organization, technology
Relationship types: works_on, collaborates_with, decided, contradicts, depends_on, member_of, located_in
```

#### Implementation

**File:** `src/entities.rs` — add `LlmEntityExtractor`

```rust
pub struct ExtractedRelationship {
    pub from: String,
    pub to: String,
    pub relation: String,
    pub detail: Option<String>,
}

#[async_trait]
pub trait EntityExtractor: Send + Sync {
    async fn extract(
        &self,
        text: &str,
        known_entities: &[ExtractedEntity],
    ) -> Result<(Vec<ExtractedEntity>, Vec<ExtractedRelationship>)>;
}

// HeuristicExtractor — existing code, wrapped in trait
// LlmEntityExtractor — new, calls OpenAI-compatible API
// CompositeExtractor — runs heuristic first, then LLM for refinement
```

**File:** `src/db.rs` — add `create_typed_edge`

```rust
pub async fn create_typed_edge(
    db: &Surreal<Db>,
    from_entity: &str,
    to_entity: &str,
    relation: &str,
    detail: Option<&str>,
) -> Result<()> {
    db.query(
        "RELATE $from->related_to->$to SET relation = $relation, detail = $detail, created_at = $now"
    )
    .bind(("from", Thing::from(("entity", from_entity))))
    .bind(("to", Thing::from(("entity", to_entity))))
    .bind(("relation", relation))
    .bind(("detail", detail.unwrap_or("")))
    .bind(("now", Utc::now().to_rfc3339()))
    .await?.check()?;
    Ok(())
}
```

**Config:**
```toml
[entities]
provider = "openrouter"           # or "heuristic" for no LLM
model = "anthropic/claude-sonnet-4-6"
api_key_env = "OPENROUTER_API_KEY"
```

**Cost control:** LLM extraction only runs during consolidation (batch, not per-message). Heuristic extraction remains for real-time ingest.

### Acceptance Criteria

- [x] `nomen consolidate` extracts entities via LLM when configured (CompositeExtractor)
- [x] Typed `related_to` edges created between entities (not just `mentions`)
- [x] `nomen entities --relations` shows entity relationships
- [x] Falls back to heuristic when LLM is unconfigured (`build_entity_extractor()`)
- [x] Entity dedup by normalized name (case-insensitive)

---

## 3. Cluster Fusion

### Problem

Over time, Nomen accumulates many topic-keyed memories that are related but separate:
- `user/k0/preferences` — UI preferences
- `user/k0/timezone` — timezone
- `user/k0/projects` — active projects
- `user/k0/schedule` — availability

These are never synthesized into a coherent "everything about k0" summary. An agent searching for general context about k0 gets fragmented results. The paper calls this "cluster-level fusion" — grouping related memories and producing higher-level summaries that capture cross-instance regularities.

### What This Enables

- **Coherent user profiles** — "Tell me about k0" returns a synthesized overview, not 12 fragments
- **Project summaries** — all `project/nomen/*` memories fused into a current-state overview
- **Reduced retrieval noise** — fewer, higher-quality results instead of many overlapping fragments
- **Memory scalability** — as memory count grows, cluster summaries prevent search degradation
- **Context window efficiency** — one cluster summary vs N individual memories saves tokens

### Design

Periodic background task that groups memories by topic namespace prefix, uses LLM to synthesize cluster summaries, and stores them as meta-memories with `references` edges back to source memories.

```
Cluster fusion pipeline:
1. Group memories by namespace prefix (user/k0/*, project/nomen/*)
2. For each cluster with ≥3 members:
   a. Concatenate summaries + details
   b. LLM prompt: "Synthesize a coherent summary of this person/project/topic"
   c. Store as cluster memory: topic = "cluster/user/k0" or "cluster/project/nomen"
   d. Create `references` edges (relation: "summarizes") from cluster → source memories
3. Cluster memories are refreshed on next run (replaceable by d-tag)
```

#### Implementation

**File:** `src/cluster.rs` (new)

```rust
pub struct ClusterConfig {
    pub min_members: usize,      // minimum memories per cluster (default: 3)
    pub namespace_depth: usize,  // how deep to group (default: 2, e.g. "user/k0")
    pub llm_provider: Box<dyn LlmProvider>,
    pub dry_run: bool,
}

pub async fn run_cluster_fusion(
    db: &Surreal<Db>,
    embedder: &dyn Embedder,
    config: &ClusterConfig,
    relay: Option<&RelayManager>,
) -> Result<ClusterReport> {
    // 1. Query all named memories, group by namespace prefix
    let memories = db::list_all_memories(db).await?;
    let clusters = group_by_namespace(&memories, config.namespace_depth);

    // 2. For each cluster, synthesize
    for (prefix, members) in &clusters {
        if members.len() < config.min_members { continue; }

        let cluster_topic = format!("cluster/{prefix}");

        // Build context from member summaries
        let context: String = members.iter()
            .map(|m| format!("- [{}] {}: {}", m.topic, m.summary, m.detail))
            .collect::<Vec<_>>().join("\n");

        // LLM synthesize
        let synthesis = config.llm_provider.synthesize_cluster(&context).await?;

        // Store as cluster memory
        let mem = NewMemory {
            topic: cluster_topic,
            summary: synthesis.summary,
            detail: synthesis.detail,
            tier: derive_cluster_tier(members),
            confidence: synthesis.confidence,
            source: Some("cluster_fusion".into()),
            model: Some("nomen/cluster".into()),
        };
        let d_tag = Nomen::store_direct(db, embedder, mem).await?;

        // Create "summarizes" edges
        for member in members {
            db::create_references_edge(db, &d_tag, &member.d_tag, "summarizes").await.ok();
        }
    }

    Ok(report)
}
```

**CLI:**
```bash
nomen cluster --dry-run        # preview clusters and what would be synthesized
nomen cluster                  # run cluster fusion
nomen cluster --prefix user/   # only fuse user/* memories
```

**Config:**
```toml
[memory.cluster]
enabled = true
min_members = 3
namespace_depth = 2
interval_hours = 24   # run daily (less frequent than consolidation)
```

### Acceptance Criteria

- [x] `nomen cluster` groups memories by namespace and produces synthesis summaries
- [x] Cluster memories have `references` edges (relation: "summarizes") to source memories
- [x] Cluster memories are replaceable (refresh on next run via d-tag)
- [x] `nomen search` can return cluster summaries alongside individual memories
- [x] Dry-run mode shows what clusters would be formed (with member topics)

---

## 4. Dream Cycle (Phase 2)

### Problem

Consolidation (NREM-equivalent) extracts structured knowledge from raw messages. But it processes each message group in isolation — it never looks across the full memory space to find latent connections, unexpected patterns, or creative associations.

The paper identifies this as a gap in most agent memory systems: they consolidate but don't synthesize emergent understanding.

### What This Enables

- **Latent connection discovery** — "k0's interest in decentralized protocols (Nostr) + k0's rural property (Alhovuori) → could Nostr be relevant for community coordination at Alhovuori?"
- **Pattern recognition** — "Agent has repeatedly seen user frustrated with X → create a behavioral lesson"
- **Hypothesis generation** — "Several memories mention Y failing after Z → maybe Z causes Y"
- **Self-reflection** — agent reviews its own behavioral patterns and generates lessons
- **Serendipitous recall** — during dream cycle, memories that are distant in topic space but connected in meaning get linked

### Design

(Referenced in consolidation-spec.md §12 as future work)

Periodic background task (less frequent than consolidation — daily or weekly):

```
Dream cycle pipeline:
1. Sample N random memories from different topic namespaces
2. For each pair/triplet, ask LLM:
   "Are there any non-obvious connections, patterns, or insights
    between these memories? If so, what?"
3. If LLM finds a connection:
   a. Store as a "dream" memory: topic = "dream/<slug>"
   b. Create `references` edges (relation: "associates") to source memories
   c. Low initial confidence (0.4-0.6) — these are speculative
4. Dreams that get accessed in search gain confidence over time
5. Dreams that are never accessed get pruned by normal decay
```

**Key principle:** Dreams are speculative and ephemeral by default. They must prove their value through retrieval access — Darwinian selection for useful associations.

#### Implementation

**File:** `src/dream.rs` (new)

**Config:**
```toml
[memory.dream]
enabled = false              # opt-in
sample_size = 20             # memories per cycle
pair_count = 10              # pairs to evaluate
interval_hours = 168         # weekly
initial_confidence = 0.5
```

**CLI:**
```bash
nomen dream --dry-run    # preview what pairs would be evaluated
nomen dream              # run dream cycle
```

### Acceptance Criteria

- [ ] `nomen dream` samples random memory pairs and evaluates connections
- [ ] Dream memories stored with low confidence and "associates" edges
- [ ] Normal decay/pruning applies — unused dreams die naturally
- [ ] Dreams that get search hits gain confidence
- [ ] Configurable and opt-in (disabled by default)

---

## Dependency Graph

```
Feature 1: Graph Retrieval ──────────────────────┐
    (uses existing edges)                        │
                                                 ├── richer results
Feature 2: LLM Entity Extraction ──┐             │
    (creates better edges)          ├── feeds ───┘
                                    │
Feature 3: Cluster Fusion ──────────┘
    (creates summary memories + edges)

Feature 4: Dream Cycle
    (uses graph for association discovery)
    (depends on: 1 + 2 for maximum value)
```

Feature 1 delivers immediate value with existing data. Feature 2 makes Feature 1 significantly better. Feature 3 is independently valuable for memory scalability. Feature 4 requires 1+2 to be most effective but can work standalone.

---

## References

- Hu et al. (2025). "Memory in the Age of AI Agents." arXiv:2512.13564
- §3.1 — Memory topology (1D/2D/3D)
- §5.2.1 — Memory consolidation (local/cluster/global)
- §2.3.2 — Graph RAG
- A-MEM: arxiv.org/abs/2502.12110
- Mem0: arxiv.org/abs/2504.19413
- Nomen consolidation-spec.md §12 (Dream Cycle)
