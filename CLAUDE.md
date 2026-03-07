# CLAUDE.md — Nomen Implementation Guide

## Project

**Nomen** is a Rust CLI and library for managing agent memory. Backed by Nostr events (kind 31234) for sync/persistence and SurrealDB (embedded) for local indexing, vector search, graph relationships, and full-text search.

## Architecture

Read these docs before implementing:
- `docs/architecture.md` — Full system design, SurrealDB schema, data flow
- `docs/nostr-memory-spec.md` — NIP event format for memory (v0.2)
- `docs/migration.md` — D-tag format migration (v0.1 → v0.2)

Working documents (research, specs) are in Obsidian: `~/Obsidian/vault/Projects/Nomen/`

## Module Map (~9,400 LOC Rust, 21 files)

```
src/
├── main.rs           CLI entry, clap commands (1911 LOC)
├── lib.rs            Nomen struct, public API (284 LOC)
├── db.rs             SurrealDB schema, queries, CRUD (1269 LOC)
├── consolidate.rs    Raw messages → named memories, merge, dedup (1130 LOC)
├── mcp.rs            MCP server (JSON-RPC stdio, 9+ tools) (793 LOC)
├── contextvm.rs      Nostr-native request/response via NIP-44 (599 LOC)
├── http.rs           HTTP server + web UI serving (596 LOC)
├── groups.rs         Group management (hierarchical, NIP-29 mapping) (373 LOC)
├── search.rs         Hybrid vector + BM25 search + scoring (342 LOC)
├── config.rs         TOML config (~/.config/nomen/config.toml) (292 LOC)
├── send.rs           Send messages (NIP-17 DM, NIP-29 group, kind 1) (249 LOC)
├── memory.rs         Memory parsing, d-tag v0.1/v0.2 dual format (238 LOC)
├── relay.rs          Nostr relay sync (NIP-42, NIP-44) (235 LOC)
├── session.rs        Session ID resolution → tier/scope (232 LOC)
├── entities.rs       Entity extraction + graph edges (212 LOC)
├── embed.rs          Embedding generation (OpenAI-compatible API) (188 LOC)
├── access.rs         Access control (4-tier model) (152 LOC)
├── migrate.rs        SQLite → SurrealDB migration (feature-gated) (136 LOC)
├── display.rs        Formatted CLI output (77 LOC)
├── ingest.rs         Raw message ingestion (63 LOC)
└── kinds.rs          Custom Nostr kind constants (19 LOC)
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
nomen consolidate [--dry-run] [--older-than 30m] [--tier personal] [--batch-size 50]
nomen entities [--kind person]
nomen prune [--days 30]
nomen embed [--limit 100]
nomen group create/list/members/add/remove
nomen serve [--stdio] [--http :3000] [--context-vm] [--static-dir ...]
nomen config
nomen init
nomen doctor
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

### Memory Tiers (4-tier model)

- **public** — readable by anyone
- **group** — readable by group members (NIP-29)
- **personal** — user-auditable knowledge, encrypted (NIP-44 self-encrypt)
- **internal** — agent-only reasoning, encrypted (NIP-44 self-encrypt)
- Legacy `private` is accepted on read and normalized to `personal`

### D-Tag Format (v0.2)

D-tags encode `{visibility}:{context}:{topic}`:
- `public::rust-error-handling`
- `group:techteam:deployment-process`
- `personal:{hex-pubkey}:ssh-config`
- `internal:{hex-pubkey}:agent-reasoning`

The parser (`memory.rs`) supports dual-format read (v0.1 prefixes + v0.2 format). New writes use v0.2 format only. See `docs/migration.md`.

### Event Kinds

Defined in `kinds.rs`:
- `31234` — Named/consolidated memory (replaceable, d-tag addressable)
- `31235` — Agent lesson (replaceable)
- `1234` — Ephemeral memory (regular, future use)
- `30078` — Legacy NIP-78 (read-only compat)
- `4129` — Legacy lesson (read-only compat)

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
4. Merge into existing memories or create new ones (with near-duplicate detection)
5. Store consolidated memories with provenance tags
6. Create `consolidated_from` graph edges in SurrealDB
7. Extract entities from consolidated content
8. Mark raw messages as consolidated
9. Publish NIP-09 deletion events for consumed ephemeral Nostr events
10. Publish consolidated memories to relay

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
