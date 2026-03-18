# CLAUDE.md — Nomen Implementation Guide

**Last modified:** 2026-03-18T16:45+02:00

## Project

**Nomen** is a Rust CLI and library for managing agent memory. Backed by Nostr events (kind 31234) for sync/persistence and SurrealDB (embedded) for local indexing, vector search, graph relationships, and full-text search.

## Documentation

### Definitive specs (`docs/`)
These are the canonical specifications. Code must match these.
- `docs/architecture.md` — Full system design, SurrealDB schema, data flow (v0.3)
- `docs/api-spec.md` — Canonical API v2 specification (Draft)
- `docs/nostr-memory-spec.md` — NIP event format for memory (v0.2)
- `docs/consolidation-spec.md` — Consolidation pipeline spec (v1.0)
- `docs/raw-source-event-spec.md` — Raw source events, kind 1235 (v0.1)
- `docs/room-context-spec.md` — Room context injection for integrations (v0.1)
- `docs/migration.md` — D-tag format migration (v0.1 → v0.2)

### Design & working docs (`~/Obsidian/vault/Projects/Nomen/`)
Important design documents, research, RFCs, and audit reports. Less organized than `docs/` but contains critical context. Symlinked at `obsidian/` if available.

**Latest audit:** `03-18 Full Spec Audit.md`
New audit reports should be placed here with `MM-DD` prefix and full timestamps (created + modified).

## Module Map

```
src/
├── main.rs           (2373) CLI entry, clap commands
├── lib.rs             (953) Nomen struct, public API
├── db.rs             (1700) SurrealDB schema, queries, CRUD, provider_binding
├── consolidate.rs    (1839) Raw messages → named memories, two-phase, merge, dedup
├── mcp.rs             (552) MCP server (JSON-RPC stdio, 24+ tools)
├── cvm.rs             (500) ContextVM server (CvmServer + CvmHandler) via NIP-44
├── http.rs            (376) HTTP server + web UI serving
├── socket.rs          (323) Unix/TCP socket transport + push events
├── groups.rs          (373) Group management (hierarchical, NIP-29 mapping)
├── search.rs          (574) Hybrid vector + BM25 search + graph expansion + scoring
├── config.rs          (280) TOML config (~/.config/nomen/config.toml)
├── send.rs            (249) Send messages (NIP-17 DM, NIP-29 group, kind 1)
├── memory.rs          (251) Memory parsing, d-tag v0.1/v0.2 dual format
├── relay.rs           (227) Nostr relay sync (NIP-42, NIP-44, kind 1235 raw events)
├── signer.rs          (113) NomenSigner trait + KeysSigner default impl
├── session.rs         (231) Session ID resolution → tier/scope
├── entities.rs        (380) Entity extraction + typed relationships + graph edges
├── cluster.rs         (420) Cluster fusion — namespace-grouped memory synthesis
├── embed.rs           (188) Embedding generation (OpenAI-compatible API)
├── access.rs          (132) Access control (5-tier model)
├── migrate.rs         (139) SQLite → SurrealDB migration (feature-gated)
├── display.rs          (77) Formatted CLI output
├── ingest.rs          (253) Raw message ingestion + kind 1235 builder/parser
├── tools.rs           (711) MCP tool definitions
└── kinds.rs            (23) Custom Nostr kind constants
├── api/
│   ├── dispatch.rs    (128) Action name → handler routing (28 operations)
│   ├── types.rs       (256) Canonical request/response structs, Visibility enum
│   ├── errors.rs       (97) Structured error model (7 error codes)
│   └── operations/
│       ├── memory.rs  (374) search, put, get, get_batch, list, delete
│       ├── message.rs (226) ingest, list, context, send
│       ├── maintenance.rs (208) consolidate, prepare, commit, cluster, sync, embed, prune
│       ├── room.rs    (155) resolve, bind, unbind (provider → d-tag mapping)
│       ├── group.rs   (140) list, members, create, add/remove member
│       └── entity.rs   (86) list, relationships
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

### Memory Tiers (5-tier model)

- **public** — readable by anyone, plaintext
- **group** — readable by group members (NIP-29), plaintext (relay handles access)
- **personal** — user-auditable knowledge, NIP-44 self-encrypted
- **internal** — agent-only reasoning, NIP-44 self-encrypted
- **circle** — ad-hoc npub sets with deterministic hash. NIP-44 encrypted to participant set. Not yet implemented.
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
- `1235` — Raw source event (regular, non-replaceable, append-only). Generic format for all providers.
- `1234` — Ephemeral memory (regular, future use)
- `30078` — Legacy NIP-78 (read-only compat)
- `4129` — Legacy lesson (read-only compat)

### Consolidation Pipeline

Full pipeline in `consolidate.rs`:
1. Query unconsolidated messages (with optional time/tier filters)
2. Group by sender/channel + 4-hour time windows (with forum topic partitioning)
3. LLM summarization (or noop provider for testing)
4. Merge into existing memories or create new (near-duplicate detection at cosine >0.92)
5. Store with provenance tags + `consolidated_from` graph edges
6. Extract entities from consolidated content
7. Mark raw messages as consolidated
8. Publish NIP-09 deletion events for consumed ephemeral Nostr events
9. Publish consolidated memories to relay as kind 31234 (NIP-44 encrypted for personal/internal)

**Note:** Step 8 (NIP-09 deletion) was removed (2026-03-18) per the updated consolidation spec. Both `consolidate()` and two-phase `commit()` now preserve source events.

### Two-Phase Consolidation

For external LLM-driven extraction (e.g., OpenClaw plugin):
1. `consolidate_prepare` — collects messages, groups into batches, returns structured data for agent extraction
2. Agent processes batches externally and returns extracted memories
3. `consolidate_commit` — stores memories, creates edges, marks messages consolidated (no NIP-09 deletion)

Session tracking with TTL ensures prepare/commit pairs don't leak.

### Forum Topic Partitioning

`extract_topic_suffix()` parses Telegram forum sender strings (e.g., `telegram:group:-1003821690204:topic:9225`) and appends the topic suffix to the grouping key. Each forum topic is consolidated independently.

### Room Context (Provider Binding)

Room operations (`room.resolve`, `room.bind`, `room.unbind`) map provider-specific IDs to Nomen memory d-tags via the `provider_binding` table. This enables integrations (e.g., OpenClaw) to resolve room context memories by provider chat/group ID without knowing the Nomen d-tag scheme.

### Raw Source Events (Kind 1235)

Infrastructure for publishing raw messages as append-only Nostr events:
- `build_raw_source_event()` — builds kind 1235 event from RawMessage
- `parse_raw_source_event()` — parses kind 1235 event back to RawMessage
- Relay sync fetches kind 1235 events and imports into local DB
- **Publish-on-ingest path not yet wired** — helpers exist but `ingest_message()` only stores locally

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
