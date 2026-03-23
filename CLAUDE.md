# CLAUDE.md — Nomen Implementation Guide

**Last modified:** 2026-03-14T00:00+02:00

## Project

**Nomen** is a Rust CLI and library for managing agent memory. Backed by Nostr events (kind 31234) for sync/persistence and SurrealDB (embedded) for local indexing, vector search, graph relationships, and full-text search.

## Documentation

### Definitive specs (`docs/`)
These are the canonical specifications. Code must match these.
- `docs/architecture.md` — Full system design, SurrealDB schema, data flow
- `docs/nostr-memory-spec.md` — NIP event format for memory (v0.2)
- `docs/consolidation-spec.md` — Consolidation pipeline spec (v1.0)
- `docs/migration.md` — D-tag format migration (v0.1 → v0.2)

### Design & working docs (`~/Obsidian/vault/Projects/Nomen/`)
Important design documents, research, RFCs, and audit reports. Less organized than `docs/` but contains critical context. Symlinked at `obsidian/` if available.

**Latest audit:** `03-14 Full Spec Audit.md`
New audit reports should be placed here with `MM-DD` prefix and full timestamps (created + modified).

## Module Map

```
src/
├── main.rs           CLI entry, clap commands
├── lib.rs            Nomen struct, public API
├── db.rs             SurrealDB schema, queries, CRUD
├── consolidate.rs    Raw messages → named memories, merge, dedup, relay publish
├── mcp.rs            MCP server (JSON-RPC stdio, 9+ tools)
├── cvm.rs            ContextVM server (CvmServer + CvmHandler) via NIP-44
├── http.rs           HTTP server + web UI serving
├── groups.rs         Group management (hierarchical, NIP-29 mapping)
├── search.rs         Hybrid vector + BM25 search + scoring
├── config.rs         TOML config (~/.config/nomen/config.toml)
├── send.rs           Send messages (NIP-17 DM, NIP-29 group, kind 1)
├── memory.rs         Memory parsing, d-tag v0.1/v0.2 dual format
├── relay.rs          Nostr relay sync (NIP-42, NIP-44)
├── signer.rs         NomenSigner trait + KeysSigner default impl
├── session.rs        Session ID resolution → tier/scope
├── entities.rs       Entity extraction + graph edges
├── embed.rs          Embedding generation (OpenAI-compatible API)
├── access.rs         Access control (4-tier model)
├── migrate.rs        SQLite → SurrealDB migration (feature-gated)
├── display.rs        Formatted CLI output
├── ingest.rs         Raw message ingestion
└── kinds.rs          Custom Nostr kind constants
```

Web UI lives in `web/` (Svelte). The Rust HTTP server serves the built UI from `web/dist/`.

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

[memory.consolidation]
enabled = true
interval_hours = 4
ephemeral_ttl_minutes = 60
max_ephemeral_count = 200
provider = "openrouter"
model = "anthropic/claude-sonnet-4-6"
api_key_env = "OPENROUTER_API_KEY"

[[groups]]
id = "atlantislabs.engineering"
name = "Engineering"
members = ["npub1abc..."]
nostr_group = "techteam"
relay = "wss://zooid.atlantislabs.space"
```

## Key Implementation Notes

### NomenSigner Trait

Nomen does **not** hold raw keys. All signing and encryption is delegated to a `NomenSigner` trait (`src/signer.rs`), allowing callers (e.g. Snowclaw) to plug in their own key management.

```rust
#[async_trait]
pub trait NomenSigner: Send + Sync {
    async fn sign_event(&self, unsigned: UnsignedEvent) -> Result<Event>;
    fn public_key(&self) -> PublicKey;
    fn encrypt(&self, content: &str) -> Result<String>;       // NIP-44 self-encrypt
    fn decrypt(&self, encrypted: &str) -> Result<String>;     // NIP-44 self-decrypt
    fn encrypt_to(&self, content: &str, recipient: &PublicKey) -> Result<String>;
    fn decrypt_from(&self, encrypted: &str, sender: &PublicKey) -> Result<String>;
    fn secret_key(&self) -> Option<&SecretKey>;  // None for remote signers
}
```

- **`KeysSigner`** — default implementation wrapping `nostr_sdk::Keys`, used by CLI and tests.
- **`RelayManager`** takes `Arc<dyn NomenSigner>`, exposes `.signer()` and `.public_key()`.
- **`Config::build_signer()`** creates a `KeysSigner` from the first nsec in config (returns `None` if no nsec).
- CLI reads nsec from config/flags → wraps in `KeysSigner` → passes to `RelayManager`.
- Library consumers implement `NomenSigner` with their own key management.

### Memory Tiers (4-tier model)

- **public** — readable by anyone, plaintext
- **group** — readable by group members (NIP-29), plaintext (relay handles access)
- **personal** — user-auditable knowledge, NIP-44 self-encrypted
- **internal** — agent-only reasoning, NIP-44 self-encrypted
- **circle** — ad-hoc npub sets with deterministic hash. NIP-44 encrypted to participant set. Not yet implemented — requires MLS key agreement (e.g., Marmot / NIP-104) or similar group encryption scheme for practical use.
- Legacy `private` is accepted on read and normalized to `personal`

### D-Tag Format (v0.2)

D-tags encode `{visibility}:{context}:{topic}`:
- `public::rust-error-handling`
- `group:techteam:deployment-process`
- `personal:{hex-pubkey}:ssh-config`
- `internal:{hex-pubkey}:agent-reasoning`

The parser (`memory.rs`) supports dual-format read (v0.1 prefixes + v0.2 format). New writes use v0.2 only. See `docs/migration.md`.

### Event Kinds

Defined in `kinds.rs`:
- `31234` — Named/consolidated memory (replaceable, d-tag addressable)
- `31235` — Agent lesson (replaceable)
- `1234` — Ephemeral memory (regular, future use)
- `30078` — Legacy NIP-78 (read-only compat)
- `4129` — Legacy lesson (read-only compat)

### Consolidation Pipeline

Full pipeline in `consolidate.rs`:
1. Query unconsolidated messages (with optional time/tier filters)
2. Group by sender/channel + 4-hour time windows
3. LLM summarization (or noop provider for testing)
4. Merge into existing memories or create new (near-duplicate detection at cosine >0.92)
5. Store with provenance tags + `consolidated_from` graph edges
6. Extract entities from consolidated content
7. Mark raw messages as consolidated
8. Publish NIP-09 deletion events for consumed ephemeral Nostr events
9. Publish consolidated memories to relay as kind 31234 (NIP-44 encrypted for personal/internal)

### Search Scoring

```
score = semantic_similarity × 0.4 + text_match × 0.3 + recency × 0.15 + importance × 0.15
```

Confidence decay: `decay = 1.0 - (days_since_access / 365) × 0.8`, clamped to `[0.2, 1.0]`.

### HNSW Dimensions

Configured dynamically from `[embedding].dimensions`. Default 1536 (OpenAI text-embedding-3-small).

## Build & Test

```bash
cargo build
cargo test
```

## Code Style

- `anyhow::Result` for error handling
- `tracing` for logging (not `println!`)
- `clap` derive API for CLI args
- Keep it simple — avoid unnecessary abstractions
