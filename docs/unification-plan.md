# Interface Unification Plan

**Date:** 2026-03-11  
**Status:** Design / TODO

## Goal

All three interfaces (CLI, MCP, Context-VM) should expose the same operations using the same underlying code paths. No interface-specific reimplementations, no silent behavioral differences.

---

## Current State

### Architecture Layers

```
┌─────────────────────────────────────────────────┐
│  Interfaces (thin adapters)                     │
│  CLI (main.rs) │ MCP (mcp.rs) │ CVM (contextvm.rs) │
├─────────────────────────────────────────────────┤
│  Core API (lib.rs → Nomen struct)               │
│  store, search, delete, ingest, consolidate,    │
│  entities, send, list_memories, count_memories  │
├─────────────────────────────────────────────────┤
│  Modules                                        │
│  db, search, relay, embed, consolidate,         │
│  entities, groups, ingest, send, session        │
└─────────────────────────────────────────────────┘
```

### Problem: Interfaces Bypass the Core API

The `Nomen` struct in `lib.rs` already has well-designed methods. But **neither MCP nor Context-VM use it**. Instead they hold raw `db` + `embedder` + `relay` handles and call module functions directly. CLI also has its own inline implementations.

| Interface | Uses `Nomen` struct? | Notes |
|-----------|---------------------|-------|
| CLI | ❌ | Each `cmd_*` builds its own relay/db handles, calls modules directly |
| MCP | ❌ | `McpServer` holds raw `db`/`embedder`/`relay`, calls modules + `store_direct` |
| Context-VM | ❌ | `ContextVmServer` holds raw `db`/`embedder`/`relay`, calls modules + `store_direct` |
| lib.rs | ✅ | `Nomen` struct has proper methods but nobody uses them |

### Specific Divergences

#### 1. Store — Three different code paths

| Interface | Code Path | Relay Publish | Encryption | Supersedes | Embedding |
|-----------|-----------|---------------|------------|------------|-----------|
| CLI `cmd_store` | 80 lines inline in main.rs | ✅ | ✅ personal/internal | ✅ finds previous event | ✅ (via relay store) |
| `Nomen::store()` | lib.rs | ✅ | ✅ personal/internal | ❌ no supersedes | ✅ |
| `Nomen::store_direct()` | lib.rs | ❌ | ❌ | ❌ | ✅ |
| MCP `tool_store` | calls `store_direct` | ❌ | ❌ | ❌ | ✅ |
| CVM `handle_store` | calls `store_direct` | ❌ | ❌ | ❌ | ✅ |

**Impact:** Memories stored via MCP/CVM never reach the relay. They're local-only.

#### 2. Delete — Two different code paths

| Interface | Code Path | Relay Delete (NIP-09) | Local DB Delete |
|-----------|-----------|----------------------|-----------------|
| CLI `cmd_delete` | inline in main.rs | ✅ publishes kind 5 | ✅ |
| MCP `tool_delete` | calls `db::delete_*` | ❌ | ✅ |
| CVM | not implemented | — | — |

**Impact:** MCP deletes are local-only; relay still has the event.

#### 3. List — CLI fetches from relay, no API equivalent

CLI `cmd_list` fetches events directly from relay and parses them. There's also `Nomen::list_memories()` in lib.rs that queries local DB — but nobody uses it from MCP/CVM.

#### 4. Missing operations in MCP/CVM

| Operation | CLI | MCP | CVM | `Nomen` method exists? |
|-----------|-----|-----|-----|----------------------|
| list | ✅ | ❌ | ❌ | ✅ `list_memories()`, `count_memories()` |
| sync | ✅ | ❌ | ❌ | ❌ (inline in main.rs) |
| embed | ✅ | ❌ | ❌ | ❌ (inline in main.rs) |
| prune | ✅ | ❌ | ❌ | ❌ (inline in main.rs, calls `db::prune_memories`) |
| delete | ✅ | ✅ | ❌ | ✅ `delete()` (DB-only, no relay) |
| config/doctor/init | ✅ | ❌ | ❌ | N/A (setup-only, fine as CLI-only) |

---

## Design

### Principle: Nomen Struct Is the Single API

All operations go through `Nomen` methods. Interfaces become thin adapters that parse input → call `Nomen` → format output.

```
CLI: parse args → Nomen::method() → colored terminal output
MCP: parse JSON-RPC → Nomen::method() → MCP text response  
CVM: decrypt NIP-44 → Nomen::method() → encrypted NIP-44 response
```

### Phase 1: Fix Core Methods in lib.rs

#### 1.1 `Nomen::store()` — Add supersedes logic

Port the supersedes-finding logic from `cmd_store` into `Nomen::store()`:
- Query local DB for existing memory with same topic
- If found, add `supersedes` tag and set version accordingly
- Publish to relay with all proper tags (t, h, supersedes, model, confidence, version)

Then delete `store_direct` and `store_direct_with_author` — they become dead code once MCP/CVM use `Nomen::store()`.

#### 1.2 `Nomen::delete()` — Add relay deletion

Current `delete()` only does local DB. Add:
- If relay is connected, find the event on relay and publish NIP-09 deletion (kind 5)
- Keep local DB deletion as-is

#### 1.3 Add missing methods to `Nomen`

```rust
/// Sync memories from relay to local DB.
pub async fn sync(&self) -> Result<SyncReport>

/// Generate embeddings for memories that lack them.
pub async fn embed(&self, limit: usize) -> Result<EmbedReport>

/// Prune old/unused memories and consolidated raw messages.
pub async fn prune(&self, days: u64, dry_run: bool) -> Result<PruneReport>

/// List memories from local DB with filters.
/// (already exists: list_memories, count_memories — extend with more filters)
pub async fn list(&self, opts: ListOptions) -> Result<ListReport>

/// Delete ephemeral (raw) messages older than a duration.
pub async fn delete_ephemeral(&self, older_than: &str) -> Result<usize>
```

**Report structs** (new):
```rust
pub struct SyncReport { pub stored: usize, pub skipped: usize, pub errors: usize }
pub struct EmbedReport { pub embedded: usize, pub total: usize }
pub struct PruneReport { pub memories_pruned: usize, pub raw_messages_pruned: usize, pub pruned: Vec<PrunedMemory> }
pub struct ListReport { pub memories: Vec<MemoryRecord>, pub stats: Option<ListStats> }
pub struct ListStats { pub total: usize, pub named: usize, pub pending: usize }
```

### Phase 2: Refactor MCP + Context-VM to Use Nomen

#### 2.1 Replace raw handles with `Nomen` instance

Both `McpServer` and `ContextVmServer` currently hold:
```rust
db: Surreal<Db>,
embedder: Box<dyn Embedder>,
relay: RelayManager,  // or Option<RelayManager>
groups: GroupStore,
```

Replace with:
```rust
nomen: Nomen,
```

All tool/action handlers become one-liners:

```rust
// Before (MCP tool_search):
let results = search::search(&self.db, self.embedder.as_ref(), &opts).await?;

// After:
let results = self.nomen.search(opts).await?;
```

#### 2.2 Add missing tools/actions

**MCP — add tools:**

| Tool | Method | Notes |
|------|--------|-------|
| `nomen_list` | `nomen.list(opts)` | Returns memories + optional stats |
| `nomen_sync` | `nomen.sync()` | Trigger relay → local sync |
| `nomen_embed` | `nomen.embed(limit)` | Generate missing embeddings |
| `nomen_prune` | `nomen.prune(days, dry_run)` | Prune old memories |

**Context-VM — add actions:**

| Action | Method | Notes |
|--------|--------|-------|
| `delete` | `nomen.delete(topic, id)` | Was missing entirely |
| `list` | `nomen.list(opts)` | New |
| `sync` | `nomen.sync()` | New |
| `embed` | `nomen.embed(limit)` | New |
| `prune` | `nomen.prune(days, dry_run)` | New |

**Context-VM groups — add write operations:**

Currently CVM `handle_groups` only lists. Add sub-actions matching MCP:
- `{ "action": "groups", "params": { "action": "create", "id": "...", "name": "..." } }`
- Same for `add_member`, `remove_member`, `members`

#### 2.3 Add `session_id` support to Context-VM

MCP has `session_id` for auto-deriving tier/scope. CVM should too — add optional `session_id` param to search/store actions, use `Nomen::resolve_session()`.

#### 2.4 Expose search tuning in MCP + CVM

Add `vector_weight`, `text_weight`, and `aggregate` params to MCP `nomen_search` and CVM `search`. These just pass through to `SearchOptions`.

### Phase 3: Refactor CLI to Use Nomen

Rewrite `cmd_*` functions to use `Nomen` methods instead of inline logic.

```rust
// Before (cmd_store — 80 lines):
async fn cmd_store(relay_url, nsecs, topic, summary, detail, tier, confidence) {
    let keys = parse_keys(nsecs)?;
    let mgr = build_relay_manager(relay_url, &keys[0]);
    mgr.connect().await?;
    // ... 70 lines of event building, tag construction, relay publish, local store ...
}

// After:
async fn cmd_store(nomen: &Nomen, topic, summary, detail, tier, confidence) {
    let d_tag = nomen.store(NewMemory { topic, summary, detail, tier, confidence, .. }).await?;
    println!("Memory stored: {} [{}]", topic, tier);
}
```

**Key refactoring:**

| CLI command | Current | After |
|-------------|---------|-------|
| `cmd_store` | Builds events inline, publishes, stores to DB | `nomen.store()` |
| `cmd_delete` | Finds event on relay, publishes kind 5, deletes DB | `nomen.delete()` |
| `cmd_search` | Calls `search::search` directly | `nomen.search()` |
| `cmd_list` | Fetches from relay directly | `nomen.list()` |
| `cmd_sync` | Inline relay fetch + DB upsert | `nomen.sync()` |
| `cmd_embed` | Inline embedding loop | `nomen.embed()` |
| `cmd_prune` | Calls `db::prune_memories` | `nomen.prune()` |
| `cmd_send` | Calls `send::send_message` | `nomen.send()` |
| `cmd_consolidate` | Calls `consolidate::consolidate` | `nomen.consolidate()` |
| `cmd_ingest` | Calls `ingest::ingest_message` | `nomen.ingest_message()` |
| `cmd_messages` | Calls `ingest::get_messages` | `nomen.get_messages()` |
| `cmd_entities` | Calls `db::list_entities` | `nomen.entities()` |
| `cmd_group` | Calls `groups::*` directly | Add `nomen.group_*()` methods |

**CLI main.rs init pattern:**

```rust
// Build Nomen once at startup
let config = load_config(&cli)?;
let nomen = Nomen::open(&config).await?;

// If command needs relay, connect
if needs_relay(&cli.command) {
    nomen.connect_relay().await?;
}

match cli.command {
    Command::Store { topic, summary, .. } => cmd_store(&nomen, &topic, &summary, ..).await?,
    Command::Search { query, .. } => cmd_search(&nomen, &query, ..).await?,
    // ...
}
```

This requires adding `Nomen::connect_relay()` or handling relay setup in `Nomen::open_with_relay()`.

### Phase 4: CLI-Only Commands (No API Equivalent Needed)

These remain CLI-only as they're interactive setup/diagnostic tools:

| Command | Reason |
|---------|--------|
| `init` | Interactive wizard, requires terminal |
| `doctor` | Diagnostic output, requires terminal |
| `config` | Prints config path, trivial |

---

## Implementation Order

```
Phase 1: Core methods (lib.rs)                    ~2-3 hours
  1.1  Nomen::store() — add supersedes + relay publish ← fixes the biggest bug
  1.2  Nomen::delete() — add relay NIP-09 deletion
  1.3  Add sync(), embed(), prune(), list(), delete_ephemeral()
  1.4  Add group management methods
  1.5  Add report structs

Phase 2: MCP + Context-VM refactor                ~2-3 hours
  2.1  Replace raw handles with Nomen instance
  2.2  Add missing tools/actions (list, sync, embed, prune, delete for CVM)
  2.3  Add session_id to Context-VM
  2.4  Expose search tuning params
  2.5  Add write operations to CVM groups

Phase 3: CLI refactor                             ~2-3 hours
  3.1  Build Nomen once at startup
  3.2  Rewrite cmd_* to use Nomen methods
  3.3  Remove duplicated logic from main.rs

Phase 4: Testing + docs                          ~1 hour
  4.1  Update api-reference.md — all ✅ across the board
  4.2  Test each operation via all three interfaces
  4.3  Verify relay publish works from MCP/CVM store
```

**Total estimate:** ~8-10 hours

## Migration Notes

- `store_direct` and `store_direct_with_author` can be deprecated after Phase 2 and removed after Phase 3
- No config changes needed — all interfaces already share the same config
- No DB schema changes
- Relay behavior change: MCP/CVM store will start publishing to relay (new behavior, previously silent)
- CVM will gain 5 new actions (delete, list, sync, embed, prune) + groups write ops

## Testing Checklist

For each operation, verify identical behavior across all interfaces:

- [ ] `store` — memory appears in both local DB and relay
- [ ] `search` — same results with same params
- [ ] `delete` — removes from both local DB and relay (NIP-09)
- [ ] `list` — returns same memories
- [ ] `sync` — relay → DB sync works
- [ ] `embed` — generates embeddings
- [ ] `prune` — removes old memories
- [ ] `ingest` — stores raw message
- [ ] `messages` — returns same messages
- [ ] `entities` — returns same entities
- [ ] `consolidate` — produces same output
- [ ] `groups` — CRUD operations work across all interfaces
- [ ] `send` — message delivered via all interfaces
