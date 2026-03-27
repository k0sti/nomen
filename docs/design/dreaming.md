# Dream Cycle — Associative Memory Discovery

Beyond structured consolidation (NREM-equivalent), a creative associative pass (REM-equivalent) discovers latent connections between memories.

## Building Blocks (Implemented)

The following existing features provide the graph structure the dream cycle would use:

- **Graph-aware retrieval** — traverse edges from search hits (`graph_expand`)
- **Entity extraction** — typed entity relationships (heuristic + LLM `CompositeExtractor`)
- **Cluster fusion** — namespace-grouped memory synthesis (`memory.cluster`)

## Concept

The dream cycle would:

1. Sample memories across different scopes/topics
2. Use embedding similarity to find non-obvious connections
3. Create `references` edges for discovered relationships
4. Optionally synthesize bridging memories

This is not implemented. It's a research-stage concept.

## References

- McClelland et al. Complementary Learning Systems (CLS) Theory
- González et al. (2022). "Sleep-like unsupervised replay." Nature Communications
- Park et al. (2023). "Generative Agents." Stanford
