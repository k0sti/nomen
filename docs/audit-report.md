# Nomen Audit Report

**Date:** 2026-03-05
**Auditor:** Claude Opus 4.6 (code audit agent)
**Codebase:** commit eba8304 (main)
**Build status:** Compiles with 2 warnings, 11/11 tests pass

---

## 1. Spec Compliance

### 1.1 Architecture (docs/architecture.md)

| Spec Section | Status | Notes |
|---|---|---|
| SurrealDB embedded storage | **OK** | Uses `kv-surrealkv` as specified |
| Storage path `~/.nomen/db/` | **OK** | `db.rs:157-162` |
| Schema: `memory` table | **PARTIAL** | Missing `ephemeral` field (spec: `bool DEFAULT false`). `created_at`/`updated_at` use `string` not `datetime` (spec says `datetime`). |
| Schema: `entity` table | **PARTIAL** | `created_at` uses `string` not `datetime` |
| Schema: `scope` table | **MISSING** | Architecture spec defines a `scope` table with parent/tier/name fields; implementation uses `nomen_group` instead. Acceptable deviation — groups serve the same purpose. |
| Schema: `references` edge table | **MISSING** | Not defined in `db.rs` SCHEMA. Architecture spec defines `references` for memory-to-memory links (supports/contradicts/supersedes/elaborates). |
| Schema: `related_to` edge table | **MISSING** | Not defined in `db.rs` SCHEMA. Architecture spec defines `related_to` for entity-to-entity relationships. |
| Schema: `mentions` edge table | **OK** | Defined as SCHEMALESS in `db.rs:218` |
| Schema: `consolidated_from` edge table | **OK** | Defined as SCHEMALESS in `db.rs:219` |
| HNSW vector index | **OK** | 1536 dims, cosine, EFC 150, M 12 — matches spec |
| BM25 full-text index | **OK** | Snowball English analyzer matches spec |
| Hybrid search (vector + BM25) | **OK** | `search.rs` + `db.rs:hybrid_search` |
| Entity extraction (heuristic) | **OK** | `entities.rs` with @mentions, capitalized phrases, known entities |
| Group hierarchy (dot-separated) | **OK** | `groups.rs` with derive_parent |
| Tier enforcement | **OK** | `access.rs:can_access()` |
| NIP-42 auth | **OK** | nostr-sdk handles automatically with signer |
| NIP-44 encryption (private tier) | **OK** | `relay.rs:encrypt_private/decrypt_private`, `memory.rs:try_decrypt_content` |
| NIP-09 deletion | **OK** | `main.rs:cmd_delete` publishes kind 5 |
| NIP-78 (kind 30078) | **OK** | Used for memory events |
| Kind 4129 agent lessons | **OK** | Fetched in `relay.rs:fetch_memories` |
| Message ingestion pipeline | **OK** | `ingest.rs`, `db.rs:store_raw_message` |
| Consolidation pipeline | **OK** | `consolidate.rs` with LlmProvider trait |
| MCP server (stdio) | **OK** | `mcp.rs:serve_stdio` |
| MCP server (HTTP) | **MISSING** | Architecture spec mentions HTTP+SSE transport. Not implemented. |
| Context-VM (kind 21900/21901) | **OK** | `contextvm.rs` |
| Library crate | **OK** | `lib.rs` with `Nomen` struct |
| Snowclaw adapter | **OK** | `snowclaw_adapter.rs` with `Memory` trait impl |
| SQLite migration | **OK** | `migrate.rs` (feature-gated) |

### 1.2 Implementation Plan (docs/implementation-plan.md)

| Phase | Status | Gaps |
|---|---|---|
| Phase 1: Core Memory System | **~95%** | Missing: `ephemeral` field, `references`/`related_to` tables, `topic_filter` in SearchOptions, `scope` table (replaced by nomen_group) |
| Phase 2: Message Ingestion & Consolidation | **~85%** | Missing: `time_window` in ConsolidationConfig, retention/pruning, `--around` message query, LLM provider impl (only NoopLlmProvider), `metadata` stored as `option<string>` not `option<object>` |
| Phase 3: Nostr Relay Sync | **~90%** | Missing: live subscription (`subscribe()` method), incremental sync, d-tag ↔ scope mapping for group-scoped d-tags |
| Phase 4: MCP Server | **~70%** | Missing: HTTP transport, `nomen_groups` tool, MCP `resources/list` support |
| Phase 5: Context-VM | **~90%** | Missing: expiration tag on responses, rate limiting |
| Phase 6: Snowclaw Integration | **~85%** | Missing: `promote()` and `reindex()` trait methods |

### 1.3 Nostr Memory Spec (docs/nostr-memory-spec.md)

| Spec Item | Status | Notes |
|---|---|---|
| Kind 30078 for memories | **OK** | |
| Content JSON (summary, detail, context) | **OK** | |
| D-tag namespace (`snow:memory:*`) | **OK** | |
| `snow:tier` tag | **OK** | |
| `snow:model` tag | **OK** | |
| `snow:confidence` tag | **OK** | |
| `snow:source` tag | **OK** | |
| `snow:version` tag | **OK** | |
| `snow:supersedes` tag | **MISSING** | Not published in `cmd_store`. Not checked on ingest. |
| `t` (topic) tags | **MISSING** | Not published in `cmd_store`. Spec says repeatable topic tags for relay-side filtering. |
| `h` tag for group scoping | **PARTIAL** | Read in `memory.rs:parse_tier` but not published in `cmd_store` for group-tier memories. |
| Per-user memory d-tag format | **OK** | Parsed in `memory.rs:parse_d_tag` |
| Per-group memory d-tag format | **OK** | Parsed in `memory.rs:parse_d_tag` |
| NIP-44 for private tier | **OK** | Encrypted on publish, decrypted on read |
| Kind 4129 agent lessons | **OK** | Fetched and counted |

---

## 2. Code Quality Issues

### 2.1 Warnings

1. **`db.rs:511` — `Created` struct never constructed.** The struct is defined but never used because `store_raw_message` doesn't return the created record. Dead code.

2. **`mcp.rs:26` — `jsonrpc` field never read.** The JSON-RPC request struct deserializes the field but never validates it. Should either validate `jsonrpc == "2.0"` or suppress with `#[allow(dead_code)]`.

### 2.2 Potential Panics

1. **`memory.rs:32` — `&rest[..12.min(rest.len())]`** — Safe (min guards bounds), but could be cleaner with `rest.get(..12).unwrap_or(rest)`.

2. **`display.rs:8` — `ts.as_u64() as i64`** — Safe for reasonable timestamps but could overflow for values > i64::MAX. Unlikely in practice.

### 2.3 Dead Code / Unused

1. **`db.rs:510-514`** — `Created` struct with custom deserializer is defined but never used.
2. **`memory.rs:2`** — `use nostr_sdk::prelude::nip44` — imported but only used in `try_decrypt_content`. Fine, but flagged by some linters.

### 2.4 Copy-Paste Artifacts

1. **Store logic duplicated 4 times** — `lib.rs:Nomen::store()`, `mcp.rs:tool_store()`, `contextvm.rs:handle_store()`, and `main.rs:cmd_store()` all independently construct `ParsedMemory` and call `store_memory_direct`. These should delegate to `Nomen::store()`.

2. **Embedding generation duplicated 3 times** — After storing, `lib.rs`, `mcp.rs`, and `contextvm.rs` all have identical embedding generation blocks. Should be part of `Nomen::store()`.

### 2.5 Inconsistencies

1. **`raw_message` table is `SCHEMALESS` in code** (`db.rs:199`) but the implementation-plan spec says `SCHEMAFULL`. The schema still defines fields with types, but SCHEMALESS allows arbitrary extra fields.

2. **`metadata` field type mismatch** — Spec says `option<object>`, code uses `option<string>` in db.rs schema. The `RawMessage.metadata` is `Option<String>` (JSON string), not a structured object. The metadata field in `store_raw_message` is omitted entirely from the `NewRawMessage` struct.

3. **Config path inconsistency** — CLAUDE.md says `~/.nomen/config.toml`, `config.rs:110-115` uses `~/.config/nomen/config.toml`. The XDG path is correct for the code, but docs should be updated.

### 2.6 Missing Error Context

1. **`db.rs:287`** — `result.check()?.take(0)?` — If the version check query fails, the error message won't indicate which d_tag failed.

---

## 3. SurrealDB Issues

### 3.1 Schema Type Mismatches

1. **`created_at` / `updated_at` on `memory` table** — Schema defines `TYPE string` (`db.rs:178-179`), architecture spec says `TYPE datetime`. Using strings works but loses SurrealDB's datetime operations (comparisons, ranges). **Not a runtime bug but a spec deviation.**

2. **`created_at` on `entity` table** — Same issue: `TYPE string` vs spec `TYPE datetime`.

3. **`created_at` on `nomen_group` table** — Same issue.

4. **`members` on `nomen_group`** — Schema says `TYPE array` (`db.rs:193`), implementation plan says `TYPE array<string>`. SurrealDB v2 accepts both, but the typed version is stricter. Minor.

### 3.2 Query Issues

1. **`db.rs:602` — `UPDATE $id SET consolidated = true`** — Uses `$id` as a parameter bound to a `Thing`. In SurrealDB v2, `UPDATE $id` should work with a Thing parameter, but the bind uses `surrealdb::sql::Thing::from(("raw_message", id.as_str()))`. This is correct syntax.

2. **`db.rs:405` — `embedding IS NONE`** — SurrealDB v2 uses `IS NONE` for checking unset optional fields. This is correct.

3. **`hybrid_search` requires BM25 match** — The WHERE clause always includes `content @1@ $query` (line 423). This means the hybrid search will only return memories that have a BM25 text match, even if they have high vector similarity. Memories with embeddings but no text match will be missed. This is a significant search limitation. The spec hybrid query also has this issue, so it's a design choice, but it should be documented.

4. **`snowclaw_adapter.rs:357` — `WHERE topic CONTAINS $prefix`** — `CONTAINS` in SurrealDB checks if an array contains a value. For string prefix matching, should use `string::starts_with(topic, $prefix)`. **This is a bug — `list()` by category will not work correctly.**

5. **`groups.rs:188` — `SELECT * FROM nomen_group ORDER BY id`** — The Group struct has `id: String` but SurrealDB assigns a `Thing` record ID (`nomen_group:xxx`). The `id` field in the struct is a custom field, not the SurrealDB record ID. This works because Group derives Serialize/Deserialize and SurrealDB maps the `id` field from the custom data. But the SurrealDB record ID (`id` column) shadows the custom `id` field. **Potential deserialization issue** — the `id` field may deserialize as the SurrealDB Thing instead of the custom string field.

6. **`consolidate.rs:196-201` — consolidated_from edges not created.** The code has a TODO comment noting that edges can't be created because `store_memory_direct` doesn't return the SurrealDB record ID. This is a known gap.

### 3.3 Missing Indexes

1. **`raw_message` table** — Architecture spec defines `DEFINE INDEX raw_msg_source ON raw_message FIELDS source, source_id UNIQUE` and `DEFINE INDEX raw_msg_sender ON raw_message FIELDS sender`. Code only has `raw_msg_time` and `raw_msg_channel` indexes. Missing the source+source_id unique index means duplicate messages from the same platform can be inserted.

---

## 4. Nostr Protocol Compliance

### 4.1 Event Structure (cmd_store)

The `cmd_store` function (`main.rs:439-516`) builds NIP-78 events correctly, but:

1. **Missing `t` (topic) tags** — Spec requires repeatable `t` tags for relay-side topic filtering. The code only adds `d`, `snow:tier`, `snow:model`, `snow:confidence`, `snow:source`, `snow:version`.

2. **Missing `h` tag for group tier** — When `tier == "group"`, the event should include `["h", "<nostr_group_id>"]` for NIP-29 relay scoping. The code doesn't add this.

3. **Missing `snow:supersedes` tag** — When updating an existing memory (same d-tag), the previous event ID should be referenced.

### 4.2 Relay Manager

1. **`relay.rs:91-105` — `fetch_memories`** — Correctly fetches kind 30078 + 4129. Uses `.authors()` filter. Good.

2. **`relay.rs:108-149` — `publish`** — Correctly inspects `Output` for accepted/rejected. Good.

3. **`relay.rs:117-121` — `output.success`** — This accesses `.success` and `.failed` fields directly. Need to verify these exist in nostr-sdk 0.39. The `SendEventOutput` struct may have changed names across versions.

### 4.3 Context-VM

1. **Event kinds** — Uses 21900/21901 as specified in implementation plan. Good.

2. **`contextvm.rs:100` — `.pubkey(our_pubkey)`** — The filter uses `.pubkey()` which is a NIP-03 delegatee filter, not a `#p` tag filter. Should use `.custom_tag(SingleLetterTag::lowercase(Alphabet::P), our_pubkey.to_hex())` or equivalent to filter for events tagged with `["p", our_pubkey]`. **This is likely a bug** — events may not be properly filtered.

3. **Missing expiration tag on responses** — Spec says requests have expiration tags, but responses don't include them. Requests also don't validate the expiration tag.

4. **Missing `"expiration"` tag in subscription filter** — Could subscribe to expired requests.

---

## 5. MCP Protocol Compliance

### 5.1 JSON-RPC 2.0

1. **`mcp.rs:26` — `jsonrpc` field not validated.** Should verify `jsonrpc == "2.0"`. Currently just deserializes and ignores. Minor issue but technically non-compliant.

2. **`mcp.rs:605-612` — Notification handling.** The code checks `req.id.is_none() || req.method.starts_with("notifications/")`. This is correct — JSON-RPC 2.0 notifications have no `id`. However, `notifications/initialized` has an `id` in many MCP clients. The current code sends a response for it (line 211-214), which is correct behavior.

### 5.2 MCP Protocol

1. **Missing `nomen_groups` tool** — Architecture spec lists it as a tool. Not implemented.

2. **Tool error responses** — The code returns tool errors as successful JSON-RPC responses with `isError: true` in the content. This is correct MCP behavior — tool execution errors are not JSON-RPC errors.

3. **Missing `resources/list` method** — MCP spec supports resource listing. Not critical but limits discoverability.

4. **Missing `prompts/list` method** — MCP spec supports prompt listing. Not critical.

5. **MCP protocol version** — Uses `"2024-11-05"` which is current. Good.

---

## 6. Snowclaw Adapter Compliance

### 6.1 Trait Method Mapping

| Snowclaw Method | NomenAdapter | Status |
|---|---|---|
| `name()` | Returns `"nomen"` | **OK** |
| `store(key, content, category, session_id)` | Delegates to `Nomen::store()` | **OK** |
| `recall(query, limit, session_id, context)` | Delegates to `Nomen::search()` | **OK** |
| `get(key)` | Direct SurrealDB query | **OK** |
| `list(category, session_id)` | Direct SurrealDB query | **BUG** — uses `CONTAINS` instead of `string::starts_with` |
| `forget(key)` | Delegates to `Nomen::delete()` | **OK** |
| `count()` | `SELECT count()` query | **OK** |
| `store_with_tier(key, content, category, tier)` | Delegates to `Nomen::store()` with tier mapping | **OK** |
| `promote(key, new_tier)` | **MISSING** | Not implemented. Snowclaw trait has default impl that errors. |
| `health_check()` | `RETURN true` query | **OK** |
| `recent_group_messages(group_id, limit)` | Delegates to `Nomen::get_messages()` | **OK** |
| `reindex(progress_callback)` | **MISSING** | Not implemented. Snowclaw trait has default impl that errors. |

### 6.2 Type Compatibility

1. **`MemoryEntry`** — Nomen defines its own copy (`snowclaw_adapter.rs:21-30`) that mirrors Snowclaw's. This works because Nomen's trait also defines its own `Memory` trait. However, this means Nomen's `NomenAdapter` implements Nomen's *own* `Memory` trait, not Snowclaw's. **Integration issue** — for actual Snowclaw integration, the adapter needs to implement `snowclaw::memory::traits::Memory`, not `nomen::snowclaw_adapter::Memory`.

2. **`MemoryTier`** — Uses `snow_memory::types::MemoryTier` from the `snow-memory` crate. This is the correct import.

---

## 7. Testing Gaps

### 7.1 No Integration Tests

- No tests for SurrealDB operations (store, search, hybrid search)
- No tests for relay operations
- No tests for MCP server
- No tests for Context-VM

### 7.2 Missing Unit Tests

- `memory.rs` — no tests for `parse_event`, `parse_d_tag`, `parse_tier`, `try_decrypt_content`
- `search.rs` — no tests
- `consolidate.rs` — no tests
- `ingest.rs` — no tests
- `embed.rs` — no tests
- `config.rs` — no tests for `Config::load`, `all_nsecs`, `build_embedder`
- `db.rs` — no tests (all functions require SurrealDB instance)

### 7.3 Edge Cases Not Covered

1. Empty query string in search
2. Unicode handling in entity extraction
3. SurrealDB connection failure recovery
4. Relay connection timeout
5. Embedding API failure during store
6. Concurrent write conflicts on same d-tag
7. Very long content strings
8. Invalid NIP-44 encrypted content

---

## 8. Fixes Applied

### Fix 1: Remove dead `Created` struct (db.rs)
Removed unused `Created` struct and its imports.

### Fix 2: Suppress `jsonrpc` field warning (mcp.rs)
Added `#[allow(dead_code)]` annotation.

### Fix 3: Fix `CONTAINS` → `string::starts_with` in snowclaw_adapter.rs
Fixed the `list()` method to use correct SurrealDB string prefix matching.

### Fix 4: Add missing `references` and `related_to` tables to schema (db.rs)
Added the two missing edge tables from the architecture spec.

### Fix 5: Add `raw_msg_source` and `raw_msg_sender` indexes (db.rs)
Added missing indexes from the architecture spec.

### Fix 6: Add `t` (topic) tags and `h` tag to cmd_store (main.rs)
Store command now publishes topic tags for relay-side filtering and the `h` tag for group-scoped memories.

### Fix 7: Store metadata in raw_message (db.rs)
Added metadata field to `NewRawMessage` struct so it's not silently dropped.

---

## 9. Remaining Gaps (Need Separate Work)

### High Priority

1. **MCP `nomen_groups` tool** — Listed in spec, not implemented.
2. **HTTP transport for MCP** — Architecture spec mentions it; only stdio exists.
3. **Live relay subscription** — `RelayManager` has no `subscribe()` for incremental sync.
4. **Consolidated_from edge creation** — `store_memory_direct` doesn't return SurrealDB record IDs.
5. **LLM provider implementation** — Only `NoopLlmProvider` exists; need OpenRouter/Anthropic impl.
6. **Context-VM subscription filter** — `.pubkey()` may not filter on `p` tags correctly.

### Medium Priority

7. **Schema datetime types** — `created_at`/`updated_at` should be `datetime` not `string`.
8. **`ephemeral` field on memory table** — Spec defines it, not implemented.
9. **`snow:supersedes` tag** — Not published or checked.
10. **Retention/pruning** — No mechanism to prune old consolidated raw messages.
11. **Rate limiting** — Context-VM has no rate limiting per npub.
12. **Scope prefix matching in search** — Architecture says `atlantislabs` search should include `atlantislabs.engineering`. Current scope filter uses `IN` not prefix matching.

### Low Priority

13. **Nomen's own Memory trait vs Snowclaw's** — For real integration, adapter should impl Snowclaw's trait directly.
14. **`promote()` and `reindex()` trait methods** — Not implemented (default error impls exist).
15. **Config path docs** — CLAUDE.md says `~/.nomen/config.toml` but code uses `~/.config/nomen/config.toml`.
16. **Message `--around` query** — Listed in architecture spec, not implemented.

---

## 10. Suggested Next Priorities

1. **Fix Context-VM subscription filter** — Likely broken, quick fix
2. **Implement `nomen_groups` MCP tool** — Small, completes the MCP tool set
3. **Add LLM provider** (Anthropic or OpenRouter) — Unlocks real consolidation
4. **Live subscription for incremental sync** — Important for daemon mode
5. **Integration tests** — SurrealDB-backed tests for critical paths (store, search, consolidation)
