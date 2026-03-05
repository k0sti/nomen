# Nomen Audit Report

**Date:** 2026-03-05 (updated)
**Previous audit:** 2026-03-05 (commit eba8304)
**Current codebase:** commit bea4092 (main)
**Build status:** Compiles with 0 warnings, 15/15 tests pass (11 unit + 4 integration)
**Auditor:** Claude Opus 4.6 (code audit agent)

---

## Changes Since Last Audit

The previous audit (commit eba8304) identified 16 gaps and applied 7 fixes. Since then (commit bea4092):

### Fixes Applied
1. **Context-VM subscription filter** — Addressed in commit bea4092
2. **`nomen_groups` MCP tool** — Implemented in commit bea4092
3. **`CONTAINS` → `string::starts_with`** in snowclaw_adapter — Fixed in commit 06e2f73
4. **`t` (topic) and `h` tags** added to store — Fixed in commit 06e2f73
5. **Missing schema tables/indexes** (`references`, `related_to`, `raw_msg_source`, `raw_msg_sender`) — Added in commit 06e2f73
6. **Live relay subscription** (`RelayManager::subscribe()`) — Added in commit bea4092
7. **LLM provider** (`OpenAiLlmProvider`) — Implemented in commit bea4092
8. **Prune command** — Implemented in commit bea4092
9. **Integration tests** — 4 tests added (store+search, ingest+consolidate, groups, prune)

### Still Open
- Context-VM `.pubkey()` filter — partially addressed but needs verification
- HTTP MCP transport — still missing
- `snow:supersedes` tag — still not published
- Schema datetime types — still using `string`
- Scope prefix matching in search — still uses `IN` not prefix

---

## 1. Full Compliance Matrix

### 1.1 Architecture Spec (docs/architecture.md)

| Spec Item | Status | Location | Notes |
|-----------|--------|----------|-------|
| SurrealDB embedded storage (kv-surrealkv) | **DONE** | `db.rs:157` | |
| Storage path `~/.nomen/db/` | **DONE** | `db.rs:157-162` | |
| Config path `~/.config/nomen/config.toml` | **DONE** | `config.rs:110-115` | XDG-compliant |
| Schema: `memory` table (SCHEMAFULL) | **DONE** | `db.rs:160-181` | All fields defined |
| Schema: `memory.ephemeral` field | **DONE** | `db.rs:176` | `bool DEFAULT false` |
| Schema: `memory.created_at` as datetime | **PARTIAL** | `db.rs:178` | Uses `string` not `datetime` |
| Schema: `memory.updated_at` as datetime | **PARTIAL** | `db.rs:179` | Uses `string` not `datetime` |
| Schema: `entity` table | **DONE** | `db.rs:183-189` | |
| Schema: `entity.created_at` as datetime | **PARTIAL** | `db.rs:188` | Uses `string` not `datetime` |
| Schema: `nomen_group` table | **DONE** | `db.rs:191-204` | Replaces spec's `scope` table |
| Schema: `nomen_group.created_at` as datetime | **PARTIAL** | `db.rs:203` | Uses `string` not `datetime` |
| Schema: `raw_message` table | **DONE** | `db.rs:206-217` | SCHEMALESS (spec says SCHEMAFULL) |
| Schema: `raw_message.metadata` as object | **PARTIAL** | `db.rs:213` | Uses `option<string>` not `option<object>` |
| Schema: `mentions` edge table | **DONE** | `db.rs:218` | SCHEMALESS |
| Schema: `consolidated_from` edge table | **DONE** | `db.rs:219` | SCHEMALESS |
| Schema: `references` edge table | **DONE** | `db.rs:220` | Added in 06e2f73 |
| Schema: `related_to` edge table | **DONE** | `db.rs:221` | Added in 06e2f73 |
| HNSW vector index (1536D, cosine) | **DONE** | `db.rs:223-224` | EFC 150, M 12 |
| BM25 full-text index | **DONE** | `db.rs:226-228` | Snowball English analyzer |
| Index: `memory_tier` | **DONE** | `db.rs:229` | |
| Index: `memory_scope` | **DONE** | `db.rs:230` | |
| Index: `memory_topic` | **DONE** | `db.rs:231` | |
| Index: `memory_d_tag` (unique) | **DONE** | `db.rs:232` | |
| Index: `entity_name` (unique) | **DONE** | `db.rs:234` | |
| Index: `group_id` (unique) | **DONE** | `db.rs:235` | |
| Index: `group_parent` | **DONE** | `db.rs:236` | |
| Index: `raw_msg_source` (unique) | **DONE** | `db.rs:237` | Added in 06e2f73 |
| Index: `raw_msg_time` | **DONE** | `db.rs:238` | |
| Index: `raw_msg_sender` | **DONE** | `db.rs:239` | Added in 06e2f73 |
| Index: `raw_msg_channel` | **DONE** | `db.rs:240` | |
| Hybrid search (vector + BM25) | **DONE** | `search.rs` + `db.rs:hybrid_search` | 70/30 default weights |
| Entity extraction (heuristic) | **DONE** | `entities.rs` | @mentions, capitalized, known entities |
| Group hierarchy (dot-separated) | **DONE** | `groups.rs` | `derive_parent()`, scope expansion |
| Tier enforcement (public/group/private) | **DONE** | `access.rs:can_access()` | |
| Scope prefix matching in search | **MISSING** | `access.rs` | Uses `IN` not `string::starts_with()` |
| NIP-42 relay auth | **DONE** | Automatic via nostr-sdk signer | |
| NIP-44 encryption (private tier) | **DONE** | `relay.rs`, `memory.rs` | Encrypt + decrypt |
| NIP-09 deletion (kind 5) | **DONE** | `main.rs:cmd_delete` | |
| NIP-29 group scoping (h tag) | **DONE** | `memory.rs:parse_tier` reads, `cmd_store` publishes | Fixed in 06e2f73 |
| Kind 30078 memory events | **DONE** | Throughout | |
| Kind 4129 agent lessons | **DONE** | `relay.rs:fetch_memories` | |
| Message ingestion pipeline | **DONE** | `ingest.rs`, `db.rs` | |
| Consolidation pipeline | **DONE** | `consolidate.rs` | NoopLlmProvider + OpenAiLlmProvider |
| MCP server (stdio) | **DONE** | `mcp.rs:serve_stdio` | JSON-RPC 2.0 |
| MCP server (HTTP) | **MISSING** | — | Architecture spec mentions HTTP+SSE |
| MCP tools: nomen_search | **DONE** | `mcp.rs` | |
| MCP tools: nomen_store | **DONE** | `mcp.rs` | |
| MCP tools: nomen_ingest | **DONE** | `mcp.rs` | |
| MCP tools: nomen_messages | **DONE** | `mcp.rs` | |
| MCP tools: nomen_entities | **DONE** | `mcp.rs` | |
| MCP tools: nomen_consolidate | **DONE** | `mcp.rs` | |
| MCP tools: nomen_delete | **DONE** | `mcp.rs` | |
| MCP tools: nomen_groups | **DONE** | `mcp.rs` | Added in bea4092 |
| Context-VM (kind 21900/21901) | **DONE** | `contextvm.rs` | |
| Library crate (pub API) | **DONE** | `lib.rs` | `Nomen` struct |
| Snowclaw adapter | **DONE** | `snowclaw_adapter.rs` | Feature-gated |
| SQLite migration | **DONE** | `migrate.rs` | Feature-gated |
| Embedding: OpenAI adapter | **DONE** | `embed.rs` | text-embedding-3-small |
| Embedding: batch processing | **DONE** | `embed.rs` | Configurable batch_size |
| LLM provider for consolidation | **DONE** | `consolidate.rs` | OpenAiLlmProvider + Noop |
| Live relay subscription | **DONE** | `relay.rs:subscribe()` | Added in bea4092 |
| Prune command | **DONE** | `main.rs:cmd_prune`, `db.rs:prune_old_messages` | Added in bea4092 |

### 1.2 Nostr Memory Spec (docs/nostr-memory-spec.md)

| Spec Item | Status | Notes |
|-----------|--------|-------|
| Kind 30078 for memories | **DONE** | |
| Content JSON (summary, detail, context) | **DONE** | |
| D-tag namespace `snow:memory:*` | **DONE** | |
| D-tag namespace `snowclaw:memory:npub:*` | **DONE** | Parsed in `memory.rs` |
| D-tag namespace `snowclaw:memory:group:*` | **DONE** | Parsed in `memory.rs` |
| D-tag namespace `snowclaw:config:*` | **DONE** | Parsed (shown separately) |
| `snow:tier` tag | **DONE** | Published + parsed |
| `snow:model` tag | **DONE** | Published + parsed |
| `snow:confidence` tag | **DONE** | Published + parsed |
| `snow:source` tag | **DONE** | Published + parsed |
| `snow:version` tag | **DONE** | Published + parsed |
| `snow:supersedes` tag | **MISSING** | Not published in `cmd_store`, not checked on ingest |
| `t` (topic) tags | **DONE** | Published in `cmd_store` (fixed in 06e2f73) |
| `h` tag for group scoping | **DONE** | Published when tier==group (fixed in 06e2f73) |
| Per-user memory schema | **DONE** | Parsed in `memory.rs` |
| Per-group memory schema | **DONE** | Parsed in `memory.rs` |
| NIP-44 for private tier | **DONE** | Encrypt on publish, decrypt on read |
| Kind 4129 agent lessons | **DONE** | Fetched and counted |

### 1.3 Implementation Plan Phases

| Phase | Status | Completion | Remaining Gaps |
|-------|--------|------------|----------------|
| Phase 1: Core Memory System | **DONE** | ~98% | datetime types, scope prefix search |
| Phase 2: Message Ingestion & Consolidation | **DONE** | ~95% | `--around` query, metadata as object, time_window config |
| Phase 3: Nostr Relay Sync | **DONE** | ~95% | Live subscription done; d-tag↔scope mapping for group d-tags |
| Phase 4: MCP Server | **PARTIAL** | ~85% | HTTP transport, `resources/list`, `prompts/list` |
| Phase 5: Context-VM | **DONE** | ~92% | Expiration validation, rate limiting |
| Phase 6: Snowclaw Integration | **DONE** | ~88% | `promote()`, `reindex()` trait methods |
| Phase 7: UI | **MISSING** | 0% | Not started (see future-enhancements.md) |

### 1.4 CLI Commands

| Command | Status | Notes |
|---------|--------|-------|
| `nomen list` | **DONE** | Fetches from relay, displays formatted |
| `nomen config` | **DONE** | Shows config path and status |
| `nomen sync` | **DONE** | Relay → SurrealDB |
| `nomen store` | **DONE** | Publish + store locally |
| `nomen delete` | **DONE** | NIP-09 + local delete |
| `nomen search` | **DONE** | Hybrid vector + BM25 |
| `nomen embed` | **DONE** | Generate missing embeddings |
| `nomen group` (create/list/members/add/remove) | **DONE** | Full CRUD |
| `nomen ingest` | **DONE** | Raw message ingestion |
| `nomen messages` | **DONE** | Query with filters |
| `nomen consolidate` | **DONE** | LLM-powered consolidation |
| `nomen entities` | **DONE** | List extracted entities |
| `nomen prune` | **DONE** | Delete old consolidated messages |
| `nomen serve` | **DONE** | MCP stdio + Context-VM |

---

## 2. Code Quality

### 2.1 Build Status

- **Warnings:** 0 (main crate), 2 in test file (unused struct fields — cosmetic)
- **Tests:** 15/15 pass (11 unit + 4 integration)
- **Test coverage areas:** access control, entity extraction, group hierarchy, store+search, ingest+consolidate, groups, prune

### 2.2 Warnings Resolved Since Last Audit

| Warning | Status | Fix |
|---------|--------|-----|
| `db.rs` — `Created` struct never constructed | **FIXED** | Removed dead code |
| `mcp.rs` — `jsonrpc` field never read | **FIXED** | `#[allow(dead_code)]` |

### 2.3 Remaining Code Quality Issues

1. **Store logic duplicated 4 times** — `lib.rs`, `mcp.rs`, `contextvm.rs`, `main.rs` all independently construct `ParsedMemory`. Should delegate to `Nomen::store()`. *Medium priority.*

2. **Embedding generation duplicated 3 times** — After storing, `lib.rs`, `mcp.rs`, and `contextvm.rs` all have identical embedding blocks. Should be part of `Nomen::store()`. *Medium priority.*

3. **`raw_message` table is SCHEMALESS** — Spec says SCHEMAFULL. Fields are still typed, but SCHEMALESS allows arbitrary extras. *Low priority.*

4. **`metadata` field type mismatch** — Spec says `option<object>`, code uses `option<string>`. *Low priority.*

5. **`consolidated_from` edges not fully created** — `store_memory_direct` doesn't return SurrealDB record ID needed for edge creation. Consolidation has TODO for this. *Medium priority.*

6. **`hybrid_search` requires BM25 match** — WHERE clause always includes `content @1@ $query`, so memories with embeddings but no text match are missed. Design choice but limits recall. *Medium priority.*

7. **Snowclaw adapter trait ownership** — `NomenAdapter` implements its own `Memory` trait, not Snowclaw's actual trait. For real integration, needs to impl `snowclaw::memory::traits::Memory` directly. *Medium priority.*

---

## 3. Remaining Gaps — All Items

### High Priority

| # | Gap | Impact | Status |
|---|-----|--------|--------|
| 1 | Scope prefix matching in search | Hierarchy queries broken (`atlantislabs` won't match `atlantislabs.engineering`) | **MISSING** |
| 2 | HTTP transport for MCP server | Remote agents can't connect | **MISSING** |
| 3 | `snow:supersedes` tag | Version chain not tracked in Nostr events | **MISSING** |
| 4 | Schema datetime types | Loss of temporal query operations | **PARTIAL** (string works but suboptimal) |
| 5 | Context-VM expiration validation | Could process expired requests | **MISSING** |

### Medium Priority

| # | Gap | Impact | Status |
|---|-----|--------|--------|
| 6 | Store logic deduplication (4 copies) | Maintenance burden, divergence risk | **MISSING** |
| 7 | Embedding deduplication (3 copies) | Same as above | **MISSING** |
| 8 | `consolidated_from` edge creation | Graph queries for consolidation provenance don't work | **PARTIAL** (TODO in code) |
| 9 | Context-VM rate limiting | DoS risk from aggressive agents | **MISSING** |
| 10 | `--around` message query | Can't view context around a specific message | **MISSING** |
| 11 | `metadata` field as object (not string) | Schema mismatch with spec | **MISSING** |
| 12 | `raw_message` SCHEMAFULL enforcement | Allows arbitrary fields | **MISSING** |
| 13 | JSON-RPC `jsonrpc` field validation | Technically non-compliant with JSON-RPC 2.0 | **MISSING** |

### Low Priority

| # | Gap | Impact | Status |
|---|-----|--------|--------|
| 14 | Snowclaw `promote()` trait method | Can't promote memory tier | **MISSING** |
| 15 | Snowclaw `reindex()` trait method | Can't trigger reindexing | **MISSING** |
| 16 | MCP `resources/list` method | Limits MCP discoverability | **MISSING** |
| 17 | MCP `prompts/list` method | Limits MCP discoverability | **MISSING** |
| 18 | D-tag ↔ scope mapping for group d-tags | Group d-tags may not round-trip cleanly | **MISSING** |
| 19 | Ephemeral field not used in queries | No differentiation between ephemeral and named memories | **MISSING** |

---

## 4. Testing Coverage

### Current Tests

| Test | Module | What It Covers |
|------|--------|---------------|
| `test_can_access_public` | access | Public tier allows all |
| `test_can_access_private` | access | Private tier restricts to owner |
| `test_can_access_group` | access | Group tier checks membership |
| `test_build_scope_filter` | access | Scope expansion for queries |
| `test_extract_mentions` | entities | @mention extraction |
| `test_extract_capitalized_phrases` | entities | NER-style extraction |
| `test_extract_known_entities` | entities | Known entity matching |
| `test_is_member` | groups | Direct membership check |
| `test_expand_scopes` | groups | Hierarchical scope expansion |
| `test_derive_parent` | groups | Dot-separated parent derivation |
| `test_nostr_group_mapping` | groups | NIP-29 ↔ hierarchy mapping |
| `test_store_and_search` | integration | Store → search → delete flow |
| `test_ingest_and_consolidate` | integration | Ingest → consolidate → verify |
| `test_groups` | integration | Group CRUD lifecycle |
| `test_prune` | integration | Prune old consolidated messages |

### Missing Test Coverage

| Area | Priority | Notes |
|------|----------|-------|
| `memory.rs` — parse_event, parse_d_tag, parse_tier | High | Core parsing logic untested |
| `search.rs` — hybrid search | High | Only tested via integration |
| `consolidate.rs` — LLM provider logic | Medium | Only NoopLlmProvider tested |
| `embed.rs` — OpenAI adapter | Medium | Requires API key or mock |
| `config.rs` — Config::load, all_nsecs | Low | File I/O testing |
| `mcp.rs` — protocol conformance | Medium | No MCP round-trip tests |
| `contextvm.rs` — request/response cycle | Medium | No Context-VM round-trip tests |
| `relay.rs` — publish, subscribe | Low | Requires relay connection |
| Edge cases: empty queries, Unicode, long content | Medium | No boundary tests |
| Concurrent write conflicts | Low | Race condition testing |

---

## 5. Summary

### Overall Status: ~90% Complete

The codebase has matured significantly since the initial audit. Key improvements:
- All 8 MCP tools implemented (including `nomen_groups`)
- Real LLM provider for consolidation (OpenAiLlmProvider)
- Live relay subscription
- Prune command
- Integration test suite
- Topic tags and h-tag publishing fixed

**Remaining critical gaps:** scope prefix matching in search, HTTP MCP transport, `snow:supersedes` tag.

**Recommended next focus:** See `docs/future-enhancements.md` for prioritized v0.2 roadmap (messaging, session IDs, remaining fixes).
