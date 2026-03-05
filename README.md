# Nomen

<p align="center">
  <img src="nomen.png" alt="Nomen" width="200" />
</p>

Nostr-native agent memory system — CLI and library.

Nomen provides persistent, searchable memory for AI agents using [Nostr](https://nostr.com) as the storage and sync layer. It combines NIP-78 addressable events on relays with a local [SurrealDB](https://surrealdb.com) index for hybrid semantic + full-text search, entity graphs, and group-scoped access control.

## Why

AI agents lose context between sessions. Nomen gives them durable memory that:

- **Syncs via Nostr relays** — memory travels with the agent's keypair, not locked to a host
- **Supports multi-agent trust** — agents share memories through relay subscriptions with tier-based visibility
- **Searches semantically** — hybrid vector (HNSW) + BM25 full-text search, not just keyword matching
- **Tracks entities and relationships** — extracted entities form a knowledge graph via SurrealDB's native graph edges
- **Consolidates noise into knowledge** — raw message ingestion → LLM-powered consolidation → searchable memories

## Install

```bash
git clone https://github.com/k0sti/nomen.git
cd nomen
cargo build --release
```

The binary is at `target/release/nomen`. No external database required — SurrealDB runs embedded.

## Quick Start

```bash
# Configure your relay and keys
mkdir -p ~/.config/nomen
cat > ~/.config/nomen/config.toml << 'EOF'
relay = "wss://your-relay.example.com"
nsec = "nsec1..."
EOF

# List memory events from the relay
nomen list

# Sync relay events to local SurrealDB
nomen sync

# Store a new memory (publishes to relay + stores locally)
nomen store "rust/error-handling" \
  --summary "Use anyhow for application errors" \
  --detail "In application code, prefer anyhow::Result for ergonomic error propagation." \
  --tier public \
  --confidence 0.92

# Search memories (hybrid vector + full-text)
nomen search "error handling"

# Delete a memory (NIP-09 deletion event + local removal)
nomen delete "rust/error-handling"
```

## Architecture

```
                Nostr Relay (NIP-78 kind 30078)
                        │
                        │ bidirectional sync
                        │
                    Ingestor
                  parse events
                generate embeddings
                extract entities
                        │
                    SurrealDB (embedded)
                  ┌─────┼─────┐
                  │     │     │
                docs  vectors  graph
                FTS   HNSW    edges
                  │     │     │
          ┌───────┴─────┴─────┴───────┐
          │           │               │
        CLI        Library         MCP Server
       nomen     Rust crate     stdio / HTTP
```

**Nostr relay** is the source of truth and sync mechanism. SurrealDB is the local performance index. If local state is lost, everything recovers from the relay.

### Storage

- **Relay events:** NIP-78 (kind 30078) addressable/replaceable events with custom `nomen:*` tags
- **Local index:** SurrealDB with `kv-surrealkv` backend (pure Rust, no C deps)
- **Vectors:** HNSW index (1536 dimensions, cosine similarity) for semantic search
- **Full-text:** BM25 with snowball English analyzer
- **Graph:** Native SurrealDB edges for entity relationships, memory references, consolidation provenance

### Memory Tiers

| Tier | Encryption | Visibility |
|------|-----------|------------|
| `public` | None | All agents on the relay |
| `group` | None (relay-auth gated) | Group members via NIP-29 `h` tag |
| `private` | NIP-44 (self-encrypt) | Only the owning agent |

### Group Hierarchy

Scopes use dot-separated identifiers for natural subgroup nesting:

```
""                              → public
"atlantislabs"                  → top-level group
"atlantislabs.engineering"      → subgroup
"atlantislabs.engineering.infra" → sub-subgroup
```

A query for `atlantislabs` includes memories from `atlantislabs.engineering` and deeper. Group membership is explicit per level.

## CLI Commands

| Command | Description |
|---------|-------------|
| `nomen list` | List all memory events from relay |
| `nomen sync` | Sync relay events to local SurrealDB |
| `nomen store <topic>` | Store a new memory (relay + local) |
| `nomen delete <topic>` | Delete a memory (NIP-09 + local) |
| `nomen search <query>` | Hybrid vector + full-text search |
| `nomen embed` | Generate embeddings for memories missing them |
| `nomen group create\|list\|members\|add-member\|remove-member` | Manage groups |
| `nomen ingest <content>` | Ingest a raw message for later consolidation |
| `nomen messages` | Query raw messages with filters |
| `nomen consolidate` | Run LLM-powered consolidation pipeline |
| `nomen entities` | List extracted entities |
| `nomen prune` | Delete old consolidated raw messages |
| `nomen serve` | Start MCP server (stdio) with optional Context-VM |
| `nomen config` | Show config path and status |

## Interfaces

Nomen exposes three interfaces, all backed by the same core:

### MCP Server

```bash
# stdio transport (for local agents, OpenClaw plugin)
nomen serve --stdio

# With Nostr Context-VM listener
nomen serve --stdio --context-vm --allowed-npubs <hex-pubkey>
```

**MCP tools:** `nomen_search`, `nomen_store`, `nomen_ingest`, `nomen_messages`, `nomen_entities`, `nomen_consolidate`, `nomen_delete`, `nomen_groups`

### Context-VM (Nostr-native)

Pure Nostr request/response protocol for agents that don't use MCP. Agents publish kind 21900 request events; Nomen responds with kind 21901 events. All payloads NIP-44 encrypted.

### Rust Library

```rust
use nomen::{Nomen, NewMemory};
use nomen::config::Config;
use nomen::search::SearchOptions;

let config = Config::load()?;
let nomen = Nomen::open(&config).await?;

// Store
nomen.store(NewMemory {
    topic: "rust/error-handling".into(),
    summary: "Use anyhow for application errors".into(),
    detail: "Prefer anyhow::Result for ergonomic error propagation.".into(),
    tier: "public".into(),
    confidence: 0.92,
}).await?;

// Search
let results = nomen.search(SearchOptions {
    query: "error handling".into(),
    limit: 10,
    ..Default::default()
}).await?;
```

## Configuration

`~/.config/nomen/config.toml`:

```toml
relay = "wss://your-relay.example.com"
nsec = "nsec1..."

# Optional: additional keys for multi-agent setups
# extra_nsecs = ["nsec1...", "nsec1..."]

# Embedding provider (enables semantic search)
[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"
dimensions = 1536

# Group definitions
[[groups]]
id = "myteam"
name = "My Team"
members = ["npub1...", "npub1..."]
nostr_group = "myteam"  # NIP-29 group mapping
```

## Nostr Event Format

Memories are NIP-78 (kind 30078) events with custom tags:

```json
{
  "kind": 30078,
  "content": "{\"summary\":\"...\",\"detail\":\"...\"}",
  "tags": [
    ["d", "snow:memory:rust/error-handling"],
    ["snow:tier", "public"],
    ["snow:model", "anthropic/claude-opus-4-6"],
    ["snow:confidence", "0.92"],
    ["snow:source", "<agent-pubkey-hex>"],
    ["snow:version", "1"],
    ["t", "rust"],
    ["t", "error-handling"]
  ]
}
```

See [docs/nostr-memory-spec.md](docs/nostr-memory-spec.md) for the full specification.

## Project Structure

```
src/
├── main.rs              # CLI entry point
├── lib.rs               # Library crate (pub API)
├── db.rs                # SurrealDB schema, queries, migrations
├── config.rs            # Configuration loading
├── relay.rs             # Nostr relay sync (NIP-42, NIP-44, NIP-78)
├── memory.rs            # Memory types and event parsing
├── search.rs            # Hybrid search (vector + BM25)
├── embed.rs             # Embedding trait + OpenAI provider
├── entities.rs          # Entity extraction + graph management
├── groups.rs            # Group hierarchy, membership, scope resolution
├── access.rs            # Tier enforcement, access checks
├── ingest.rs            # Raw message ingestion
├── consolidate.rs       # LLM-powered consolidation pipeline
├── mcp.rs               # MCP server (JSON-RPC over stdio)
├── contextvm.rs         # Nostr Context-VM (kind 21900/21901)
├── migrate.rs           # SQLite import (feature-gated)
├── snowclaw_adapter.rs  # Snowclaw integration (feature-gated)
└── display.rs           # Terminal output formatting

docs/
├── architecture.md      # System design, SurrealDB schema, data flow
├── nostr-memory-spec.md # NIP-78 event format specification
├── implementation-plan.md # Phased implementation plan
├── future-enhancements.md # Roadmap (messaging, sessions, UI)
└── audit-report.md      # Code audit and compliance matrix
```

## Origin

Nomen extracts and generalizes the memory system from [Snowclaw](https://github.com/k0sti/snowclaw) (Nostr-native AI agent) and ZeroClaw/OpenClaw agent runtimes. The goal is a standalone, reusable memory layer for any Nostr-aware agent.

## Status

Early development. Core functionality works — relay sync, local indexing, hybrid search, MCP server, group hierarchy, consolidation pipeline. See [docs/audit-report.md](docs/audit-report.md) for detailed status (~90% of spec implemented).

## License

MIT
