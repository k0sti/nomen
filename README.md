# Nomen

<p align="center">
  <img src="nomen.png" alt="Nomen" width="200" />
</p>

**Nostr Memory Network** — a collective, tiered memory system for AI agents.

Nomen gives AI agents durable, searchable, self-sovereign memory using [Nostr](https://nostr.com) as the storage and sync layer. Memory travels with the agent's keypair — not locked to a host, a vendor, or a database. Multiple agents can share memories through relay subscriptions with cryptographic identity and tier-based visibility.

## Key Features

### Collective & Tiered Memory

Memories exist in three tiers with different visibility:

| Tier | Encryption | Visibility |
|------|-----------|------------|
| `public` | None | All agents on the relay |
| `group` | Relay-auth gated | Group members (NIP-29 `h` tag) |
| `private` | NIP-44 self-encrypt | Only the owning agent |

Guardian and agent identities are first-class — configure multiple nsecs to read across agents while controlling who writes.

**Two kinds of groups:**

- **Named groups** — defined in config with id, name, and NIP-29 relay mapping. Dot-separated hierarchy: `atlantislabs.engineering.infra` is a subgroup of `atlantislabs.engineering`
- **Npub sets** — ad-hoc groups defined as a set of member pubkeys, scoped per tier

Both support hierarchical querying — a search scoped to `atlantislabs` includes memories from `atlantislabs.engineering` and deeper subgroups.

### Semantic Search & Knowledge Graph

- **Hybrid search** — HNSW vector similarity (1536-dim embeddings) + BM25 full-text, weighted and composable
- **Entity extraction** — People, projects, concepts automatically extracted and linked
- **Graph edges** — SurrealDB native graph: `mentions`, `references`, `contradicts`, `consolidated_from`
- **Confidence decay** — Unaccessed memories lose ranking weight over time
- **Aggregated results** — Similar hits merged into coherent summaries (`--aggregate`)
- **Importance scoring** — LLM assigns 1-10 importance at creation, used in retrieval ranking

### Sleep-Inspired Consolidation

Raw messages flow in from conversations. Consolidation extracts signal from noise — like sleep consolidating short-term memory into long-term knowledge:

```
Raw Messages (high volume, ephemeral)
    │  collection → grouping → LLM extraction → merge/dedup → storage → cleanup
    ▼
Named Memories (low volume, durable, topic-keyed)
```

- **Tier derivation** — DM sources → private, group sources → group, public → public
- **Merge, don't duplicate** — Checks existing memories by topic and embedding similarity (>0.92), merges instead of creating new
- **Conflict detection** — LLM flags contradictions, creates `contradicts` graph edges
- **Entity extraction** — Automatically extracts and links entities during consolidation
- **Auto-trigger** — Tracks last run, reports when consolidation is due
- **Pruning** — Removes unaccessed/low-confidence memories after configurable age

### MCP Server & Context-VM

**MCP** (Model Context Protocol) — stdio or HTTP transport for agent frameworks:

```bash
nomen serve --stdio                    # MCP over stdio
nomen serve --http 127.0.0.1:3000     # HTTP API + web dashboard
```

Tools: `nomen_search`, `nomen_store`, `nomen_ingest`, `nomen_messages`, `nomen_entities`, `nomen_consolidate`, `nomen_delete`, `nomen_groups`

**Context-VM** — Pure Nostr request/response for agents that don't use MCP. Agents publish kind 21900 requests; Nomen responds with kind 21901. All payloads NIP-44 encrypted.

```bash
nomen serve --stdio --context-vm --allowed-npubs <hex-pubkey>
```

### Web Dashboard

Settings page doubles as an operations dashboard:

- Memory stats (total, ephemeral, entities, groups)
- Consolidation status + "Run Now" button
- Pruning controls with dry-run preview
- Config viewer + reload
- Connection management

### Interactive Setup

```bash
nomen init     # Guided setup: relay, keys, embedding, consolidation, server
nomen doctor   # Validate config, connectivity, API keys, DB
```

## Install

```bash
git clone https://github.com/k0sti/nomen.git
cd nomen
cargo build --release
```

No external database required — SurrealDB runs embedded.

## Quick Start

```bash
# Interactive setup (recommended)
nomen init

# Or manual config
mkdir -p ~/.config/nomen
cat > ~/.config/nomen/config.toml << 'EOF'
relay = "wss://your-relay.example.com"
nsec = "nsec1..."

[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"

[memory.consolidation]
enabled = true
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
api_key_env = "OPENROUTER_API_KEY"
EOF

# Store a memory
nomen store "rust/error-handling" \
  --summary "Use anyhow for application errors" \
  --detail "Prefer anyhow::Result for ergonomic error propagation." \
  --tier public --confidence 0.92

# Search
nomen search "error handling"
nomen search "error handling" --aggregate  # merge similar results

# Ingest raw messages for consolidation
nomen ingest "k0 mentioned switching to Tailscale for VPN" --source telegram --sender k0

# Run consolidation
nomen consolidate --dry-run  # preview
nomen consolidate            # extract → merge → store → cleanup

# Prune old memories
nomen prune --days 90 --dry-run

# Start server
nomen serve --http 127.0.0.1:3000
```

## Architecture

```
              Nostr Relay (custom kind 31234)
                      │
                      │ bidirectional sync
                      │
                  ┌───┴───┐
                  │ Nomen │
                  └───┬───┘
                      │
                  SurrealDB (embedded)
                ┌─────┼─────┐
                │     │     │
              docs  vectors  graph
              FTS   HNSW    edges
                │     │     │
        ┌───────┴─────┴─────┴───────┐
        │       │       │           │
       CLI   Library  MCP Server  Context-VM
      nomen  Rust crate  stdio/HTTP  Nostr-native
```

The Nostr relay is the source of truth. SurrealDB is a local performance index. If local state is lost, everything recovers from the relay via `nomen sync`.

## Nostr Event Format

Memories use custom replaceable kind **31234** with clean, purpose-built tags:

```json
{
  "kind": 31234,
  "content": "{\"summary\":\"...\",\"detail\":\"...\"}",
  "tags": [
    ["d", "user/k0/preferences"],
    ["tier", "private"],
    ["model", "anthropic/claude-sonnet-4-6"],
    ["confidence", "0.92"],
    ["importance", "7"],
    ["source", "<agent-pubkey-hex>"],
    ["version", "1"],
    ["t", "user"],
    ["t", "preferences"]
  ]
}
```

Topics use forward-slash namespaces: `user/<name>/<aspect>`, `project/<name>/<aspect>`, `group/<id>/<aspect>`, `fact/<domain>/<topic>`, `lesson/<slug>`.

See [docs/nostr-memory-spec.md](docs/nostr-memory-spec.md) and [docs/consolidation-spec.md](docs/consolidation-spec.md) for full specifications.

## Configuration

`~/.config/nomen/config.toml`:

```toml
relay = "wss://your-relay.example.com"

# Guardian identity (owner)
nsec = "nsec1..."

# Agent identities (shared memory access)
nsecs = ["nsec1...", "nsec1..."]

# Default writer identity
default_writer = "guardian"  # or "agent:0"

[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"
dimensions = 1536

[memory.consolidation]
enabled = true
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
api_key_env = "OPENROUTER_API_KEY"
interval_hours = 4
ephemeral_ttl_minutes = 60

[[groups]]
id = "myteam"
name = "My Team"
members = ["npub1...", "npub1..."]
nostr_group = "myteam"

[server]
enabled = true
listen = "127.0.0.1:3000"
```

## CLI Reference

| Command | Description |
|---------|-------------|
| `nomen init` | Interactive setup wizard |
| `nomen doctor` | Validate config and connectivity |
| `nomen list [--named\|--ephemeral\|--stats]` | List memories |
| `nomen sync` | Sync relay → local SurrealDB |
| `nomen store <topic>` | Store a memory (relay + local) |
| `nomen delete <topic>` | Delete a memory (NIP-09 + local) |
| `nomen search <query> [--aggregate]` | Hybrid semantic + full-text search |
| `nomen embed [--limit N]` | Generate missing embeddings |
| `nomen ingest <content>` | Ingest raw message |
| `nomen messages [--source\|--channel\|--sender]` | Query raw messages |
| `nomen consolidate [--dry-run]` | LLM-powered consolidation |
| `nomen entities [--kind]` | List extracted entities |
| `nomen prune [--days N] [--dry-run]` | Prune old memories |
| `nomen group create\|list\|members\|add-member\|remove-member` | Manage groups |
| `nomen send <content> --to <recipient>` | Send message via Nostr |
| `nomen serve [--stdio\|--http ADDR]` | Start MCP/HTTP server |
| `nomen config` | Show config status |

## Rust Library

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
    detail: "Prefer anyhow::Result in app code.".into(),
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

## Project Structure

```
src/
├── main.rs          # CLI (clap)
├── lib.rs           # Public API
├── kinds.rs         # Custom Nostr event kinds (31234, 31235)
├── config.rs        # Config loading + serialization
├── db.rs            # SurrealDB schema, queries, migrations
├── relay.rs         # Nostr relay sync (NIP-42, NIP-44)
├── memory.rs        # Memory types and event parsing
├── search.rs        # Hybrid search + aggregation + confidence decay
├── embed.rs         # Embedding trait + OpenAI provider
├── entities.rs      # Entity extraction + graph
├── groups.rs        # Group hierarchy + scope resolution
├── access.rs        # Tier enforcement
├── ingest.rs        # Raw message ingestion
├── consolidate.rs   # Consolidation pipeline (LLM extraction, merge, dedup)
├── mcp.rs           # MCP server (JSON-RPC stdio/HTTP)
├── contextvm.rs     # Nostr Context-VM (kind 21900/21901)
├── http.rs          # HTTP API + dashboard endpoints
├── migrate.rs       # Import from other formats
├── send.rs          # Nostr messaging (DM, group, public)
└── display.rs       # Terminal formatting

web/                 # Svelte web dashboard
docs/                # Specifications
```

## Origin

**Nomen** = **No**str **Mem**ory **N**etwork.

Extracted from [Snowclaw](https://github.com/k0sti/snowclaw) (Nostr-native AI agent) as a standalone, reusable memory layer for any Nostr-aware agent or application.

## Status

Active development. Core fully functional: relay sync, hybrid search, entity graphs, MCP server, group hierarchy, consolidation pipeline, web dashboard, interactive setup. See [docs/consolidation-spec.md](docs/consolidation-spec.md) for detailed feature status.

## License

MIT
