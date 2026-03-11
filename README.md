# Nomen

<p align="center">
  <img src="nomen.png" alt="Nomen" width="200" />
</p>

**Scoped memory for AI agents on Nostr.**

Nomen gives AI agents persistent, searchable, shareable memory using [Nostr](https://nostr.com) as the sync layer. Memory is scoped by visibility — from public knowledge to agent-private reasoning — and signed with the agent's keypair. No vendor lock-in, no central database. Your memory travels with your keys.

## Features

### Five Visibility Tiers

| Tier | Scope | Encryption | Use Case |
|------|-------|-----------|----------|
| **Public** | All agents on relay | None | General knowledge, facts |
| **Group** | NIP-29 group members | Relay-gated | Team decisions, project context |
| **Circle** | Ad-hoc participant set | NIP-44 | Shared project notes |
| **Personal** | Agent + user | NIP-44 (user-scoped) | User preferences, history |
| **Internal** | Agent only | NIP-44 (self-encrypt) | Reasoning, self-reflection |

### Hybrid Search + Knowledge Graph

- **Semantic + full-text** — HNSW vector similarity + BM25, weighted and composable
- **Graph-aware retrieval** — Traverses entity connections, contradictions, and provenance chains from search hits (`--graph`, configurable `--hops`)
- **Entity extraction** — Heuristic + LLM-powered via `EntityExtractor` trait with typed relationships (`works_on`, `collaborates_with`, `contradicts`)
- **Cluster fusion** — Groups related memories by namespace and synthesizes coherent summaries via LLM
- **Confidence decay** — Unaccessed memories lose ranking weight over time
- **Importance scoring** — LLM assigns 1-10 importance at creation

### Sleep-Inspired Consolidation

Raw messages flow in from conversations. Consolidation extracts signal from noise:

```
Raw Messages → grouping → LLM extraction → merge/dedup → Named Memories
```

- Tier derived from source (DM → personal, group chat → group, self-reflection → internal)
- Checks existing memories by topic + embedding similarity (>0.92), merges instead of duplicating
- LLM flags contradictions, creates `contradicts` graph edges
- Automatic entity extraction and linking during consolidation

### Three Integration Paths

| Path | Transport | Best For |
|------|-----------|----------|
| **MCP Server** | stdio / HTTP | Agent frameworks with MCP support |
| **Context-VM** | Nostr (kind 25910, NIP-59 gift wrap) | Nostr-native agents, paid memory services |
| **Library** | Direct Rust API | Custom agents, tight integration |

### Nostr Event Model

Memories are **kind 31234** addressable replaceable events. D-tag encodes visibility, scope, and topic:

```
{visibility}:{scope}:{topic}
```

```json
{
  "kind": 31234,
  "content": "{\"summary\":\"Use anyhow for app errors\",\"detail\":\"Prefer anyhow::Result…\"}",
  "tags": [
    ["d", "public::rust-error-handling"],
    ["visibility", "public"],
    ["scope", ""],
    ["model", "anthropic/claude-opus-4-6"],
    ["confidence", "0.92"],
    ["version", "1"],
    ["t", "rust"],
    ["t", "error-handling"]
  ]
}
```

**Indexed `visibility` and `scope` tags** enable relay-side filtering without prefix matching:

```json
{"kinds": [31234], "#visibility": ["group"], "#scope": ["techteam"]}
```

D-tag examples:
- `public::rust-error-handling`
- `group:techteam:deployment-process`
- `circle:a3f8b2c1:shared-notes`
- `personal:d29fe7c1…:ssh-config`
- `internal:d29fe7c1…:agent-reasoning`

### Web Dashboard

- Memory browser with search
- Consolidation status + "Run Now"
- Pruning controls with dry-run preview
- Entity and graph explorer
- Config viewer + reload

## Install

```bash
git clone https://github.com/k0sti/nomen.git
cd nomen
cargo build --release
```

No external database — SurrealDB runs embedded.

## Quick Start

```bash
# Interactive setup
nomen init

# Store a memory
nomen store "rust/error-handling" \
  --summary "Use anyhow for app errors" \
  --detail "Prefer anyhow::Result for ergonomic error propagation." \
  --tier public --confidence 0.92

# Search (hybrid vector + BM25)
nomen search "error handling"
nomen search "error handling" --graph         # + graph traversal
nomen search "error handling" --graph --hops 2

# Ingest + consolidate
nomen ingest "k0 mentioned switching to Tailscale" --source telegram --sender k0
nomen consolidate

# Entity extraction and cluster fusion
nomen entities --relations
nomen cluster --dry-run

# Start MCP server
nomen serve --stdio
nomen serve --http 127.0.0.1:3000

# Context-VM (Nostr-native)
nomen serve --stdio --context-vm --allowed-npubs <hex-pubkey>
```

## Architecture

```
              Nostr Relay (kind 31234)
                      │
                 sync (NIP-42/44)
                      │
                  ┌───┴───┐
                  │ Nomen │
                  └───┬───┘
                      │
                  SurrealDB (embedded)
                ┌─────┼─────┐
                │     │     │
              docs  vectors  graph
              BM25  HNSW    edges
                │     │     │
        ┌───────┴─────┴─────┴───────┐
        │       │       │           │
       CLI   Library  MCP Server  Context-VM
```

Nostr relay is the source of truth. SurrealDB is a local index. If local state is lost, `nomen sync` recovers everything.

## Graph Edges

| Edge | Meaning |
|------|---------|
| `mentions` | memory → entity |
| `references` | memory → memory (with relation field) |
| `contradicts` | via `references` with `relation: "contradicts"` |
| `consolidated_from` | named memory → raw messages |
| `related_to` | entity → entity (typed: `works_on`, `collaborates_with`, etc.) |
| `summarizes` | cluster summary → source memories |

## CLI Reference

| Command | Description |
|---------|-------------|
| `nomen init` | Interactive setup wizard |
| `nomen doctor` | Validate config and connectivity |
| `nomen store <topic>` | Store a memory |
| `nomen search <query> [--graph] [--hops N] [--aggregate]` | Hybrid search with optional graph expansion |
| `nomen list [--named\|--ephemeral\|--stats]` | List memories |
| `nomen delete <topic>` | Delete a memory (NIP-09 + local) |
| `nomen sync` | Sync relay → local DB |
| `nomen ingest <content>` | Ingest raw message |
| `nomen consolidate [--dry-run]` | LLM consolidation pipeline |
| `nomen entities [--kind] [--relations]` | Entity explorer |
| `nomen cluster [--dry-run] [--prefix]` | Cluster fusion |
| `nomen prune [--days N] [--dry-run]` | Prune old memories |
| `nomen group create\|list\|members` | Manage groups |
| `nomen send <content> --to <recipient>` | Send via Nostr |
| `nomen serve [--stdio\|--http ADDR]` | Start server |
| `nomen embed [--limit N]` | Generate missing embeddings |
| `nomen config` | Show config status |

## Configuration

Every resource-intensive feature can be independently toggled:

```toml
relay = "wss://your-relay.example.com"
nsec = "nsec1..."

[embedding]
provider = "openai"                # or "none" to disable
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"

[memory.consolidation]
enabled = true                     # toggle consolidation
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
api_key_env = "OPENROUTER_API_KEY"

[entities]
provider = "openrouter"            # "none" → heuristic fallback
model = "anthropic/claude-sonnet-4-6"
api_key_env = "OPENROUTER_API_KEY"

[memory.cluster]
enabled = true                     # toggle cluster fusion
min_members = 3
namespace_depth = 2

[server]
enabled = true
listen = "127.0.0.1:3000"

[contextvm]
enabled = false                    # toggle Context-VM
```

## Project Structure

```
src/
├── main.rs         CLI (clap)
├── lib.rs          Public API
├── config.rs       Config loading
├── db.rs           SurrealDB schema, queries, graph edges
├── memory.rs       Memory types, d-tag parsing (v0.1/v0.2)
├── search.rs       Hybrid search + graph expansion + aggregation
├── entities.rs     Entity extraction (heuristic + LLM + composite)
├── cluster.rs      Cluster fusion pipeline
├── consolidate.rs  Consolidation (LLM extraction, merge, dedup)
├── embed.rs        Embedding providers
├── relay.rs        Nostr relay sync (NIP-42, NIP-44)
├── access.rs       Tier enforcement
├── groups.rs       Group hierarchy + scope resolution
├── ingest.rs       Raw message ingestion
├── mcp.rs          MCP server (JSON-RPC stdio/HTTP)
├── cvm.rs          Context-VM (kind 25910, NIP-59)
├── tools.rs        Shared tool dispatch (MCP + CVM)
├── http.rs         HTTP API + dashboard
├── send.rs         Nostr messaging
├── migrate.rs      Import from other formats
└── display.rs      Terminal formatting

web/                Svelte web dashboard
docs/               Specifications
```

## Origin

**Nomen** = **No**str **Mem**ory **N**etwork.

Extracted from [Snowclaw](https://github.com/k0sti/snowclaw) as a standalone memory layer for any Nostr-aware agent.

## Specs

- [Nostr Memory Spec](docs/nostr-memory-spec.md) — event format, visibility tiers, relay queries
- [Consolidation Spec](docs/consolidation-spec.md) — pipeline, merge logic, tier derivation
- [Architecture](docs/architecture.md) — system design, data flow
- [Graph & Consolidation Roadmap](docs/graph-and-consolidation-roadmap.md) — feature implementation details

## License

MIT
