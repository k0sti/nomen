# CLAUDE.md — Nomen Implementation Guide

## Project

**Nomen** is a Rust CLI and library for managing agent memory. Backed by Nostr events (NIP-78) for sync/persistence and SurrealDB (embedded) for local indexing, vector search, graph relationships, and full-text search.

## Architecture

Read these docs before implementing:
- `docs/architecture.md` — Full system design, SurrealDB schema, data flow
- `docs/nostr-memory-spec.md` — NIP-78 event format for memory

Working documents (research, specs) are in Obsidian: `~/Obsidian/vault/Projects/Nomen/`

## Module Map

```
src/
├── main.rs           CLI entry, clap commands
├── lib.rs            Nomen struct, public API (library crate)
├── db.rs             SurrealDB schema, queries, CRUD
├── search.rs         Hybrid vector + BM25 search
├── relay.rs          Nostr relay sync (NIP-78, NIP-42)
├── memory.rs         Memory parsing from Nostr events
├── send.rs           Send messages (NIP-17 DM, NIP-29 group, kind 1)
├── session.rs        Session ID resolution → tier/scope
├── mcp.rs            MCP server (JSON-RPC stdio)
├── http.rs           HTTP server + web UI serving
├── contextvm.rs      Nostr-native request/response (NIP-44)
├── ingest.rs         Raw message ingestion
├── consolidate.rs    Raw messages → named memories (with NIP-09 cleanup)
├── embed.rs          Embedding generation (OpenAI-compatible API)
├── entities.rs       Entity extraction + graph edges
├── groups.rs         Group management (hierarchical, NIP-29 mapping)
├── config.rs         TOML config (~/.config/nomen/config.toml)
├── access.rs         Access control (tier-based)
├── display.rs        Formatted CLI output
├── migrate.rs        SQLite → SurrealDB migration (feature-gated)
└── snowclaw_adapter.rs  Snowclaw integration bridge (feature-gated)
```

## CLI Commands

```
nomen list [--named] [--ephemeral] [--stats]
nomen store <topic> --summary "..." [--detail "..."] [--tier public] [--confidence 0.8]
nomen delete [<topic>] [--id <event-id>] [--ephemeral --older-than 7d]
nomen search <query> [--tier ...] [--limit 10]
nomen sync
nomen send <content> --to <recipient> [--channel nostr]
nomen ingest <content> --source cli --sender local [--channel ...]
nomen messages [--source ...] [--channel ...] [--sender ...] [--around <id>]
nomen consolidate [--dry-run] [--older-than 30m] [--tier private] [--batch-size 50]
nomen entities [--kind person]
nomen prune [--days 30]
nomen embed [--limit 100]
nomen group create/list/members/add/remove
nomen serve [--stdio] [--http :3000] [--context-vm] [--static-dir ...]
nomen config
```

## Config

Config file: `~/.config/nomen/config.toml`

```toml
relay = "wss://zooid.atlantislabs.space"
nsec = "nsec1..."

[embedding]
provider = "openai"
model = "text-embedding-3-small"
api_key_env = "OPENAI_API_KEY"
dimensions = 1536

# Spec-compliant consolidation config
[memory.consolidation]
enabled = true
interval_hours = 4
ephemeral_ttl_minutes = 60
max_ephemeral_count = 200
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
api_key_env = "OPENROUTER_API_KEY"

# Backward-compatible top-level consolidation (legacy)
# [consolidation]
# provider = "openrouter"
# model = "anthropic/claude-sonnet-4-6"
# api_key_env = "OPENROUTER_API_KEY"

[[groups]]
id = "atlantislabs.engineering"
name = "Engineering"
members = ["npub1abc..."]
nostr_group = "techteam"
relay = "wss://zooid.atlantislabs.space"
```

## Key Implementation Notes

### Memory Event Parsing

The d-tag prefix is `snow:memory:` for collective memories. Other prefixes:
- `snowclaw:memory:npub:` — per-user memories
- `snowclaw:memory:group:` — per-group memories
- `snowclaw:config:` — dynamic config (show separately or skip)

### Content JSON

```rust
#[derive(Deserialize)]
struct MemoryContent {
    summary: String,
    detail: String,
    context: Option<String>,
}
```

Some older entries may have different content formats (plain JSON objects for per-user/per-group memory). Handle gracefully — show raw content if parsing fails.

### Consolidation Pipeline

The consolidation pipeline (`consolidate.rs`) converts raw messages into named memories:
1. Query unconsolidated messages (with optional time/tier filters)
2. Group by sender/channel + 4-hour time windows
3. Send each group to LLM for summarization (or noop provider for testing)
4. Store consolidated memories with `snow:consolidated_from` and `snow:consolidated_at` tags
5. Create `consolidated_from` graph edges in SurrealDB
6. Mark raw messages as consolidated
7. Publish NIP-09 deletion events for consumed ephemeral Nostr events

### HNSW Dimensions

The HNSW vector index dimension is configured dynamically from `[embedding].dimensions` in config.
`init_db_with_dimensions()` is used by all config-aware callers. Default is 1536 (OpenAI text-embedding-3-small).

### NIP-42 AUTH

nostr-sdk handles AUTH automatically when you create a `Client` with `Keys`.

### Multiple nsec keys

Each nsec corresponds to a different agent/user identity. Subscribe to events from ALL of them in a single filter using the `authors` array.

## Build & Test

```bash
# Inside nix-shell or with cargo available:
nix-shell -p cargo rustc pkg-config openssl --run "cargo build"
nix-shell -p cargo rustc pkg-config openssl --run "cargo test"

# Or if cargo is in PATH:
cargo build
cargo test
```

## Code Style

- Use `anyhow::Result` for error handling
- Use `tracing` for logging (not `println!` for debug)
- Use `clap` derive API for CLI args
- Keep it simple — avoid unnecessary abstractions
- Web UI code lives in `web/` — managed by a separate agent

## Web UI

The web UI is a Svelte app in `web/`. **Do not modify web/ files** — it is managed by a separate agent. The Rust HTTP server serves the built UI from `web/dist/`.
