# Nomen Implementation Audit

**Date:** 2026-03-06
**Auditor:** Claude Opus 4.6
**Scope:** All specs/docs vs all Rust source + web UI source

---

## Summary

Nomen is a ~7400 LOC Rust CLI/library + ~3500 LOC Svelte web UI for Nostr-native agent memory. The core architecture is sound and implements the majority of what the architecture spec describes. The codebase is well-structured with clean module boundaries. However, there are significant gaps between the vision documents (Obsidian) and the current implementation, dead dependencies in the web UI, and several spec features that exist only as stubs or are entirely missing.

**Key findings:**
- Core memory CRUD, relay sync, hybrid search, MCP, HTTP, and Context-VM all work
- Web UI has dead dependencies (applesauce-core, applesauce-signers) and hand-rolled relay/signing code
- Consolidation pipeline is basic (no NIP-09 cleanup, no dedup, no tier/time grouping)
- Discovery & Trust RFC (kind 38990, kind 1985) is entirely unimplemented
- Dreaming/Sleep-inspired memory is pure research — no code exists
- Ad-hoc npub sets (session resolution) are designed but not implemented
- Web UI is missing create-memory UI, group management, and consolidation trigger

---

## Missing Implementation (spec exists, code missing/incomplete)

### P0 — Critical Gaps

#### 1. Consolidation: No ephemeral cleanup (NIP-09 deletion)
- **Spec says** (Memory Consolidation doc, §Consolidation Process step 5): After consolidation, source events are deleted using NIP-09 (kind 5 deletion events). The spec provides exact JSON for the deletion event including `a`-tag references.
- **Code does:** `consolidate.rs` marks messages as consolidated in SurrealDB (`mark_messages_consolidated()`) but **never publishes NIP-09 deletion events** to the relay. Ephemeral events accumulate forever on the relay.
- **Impact:** Memory pollution on relay, degraded recall quality (the core problem the consolidation spec was written to solve).

#### 2. Consolidation: No intelligent grouping
- **Spec says** (Memory Consolidation doc, §Consolidation Process steps 2-3): Group ephemeral memories by sender npub + time window (private) or group_id + time window (group). Summarize each group using LLM to extract facts, preferences, decisions.
- **Code does:** `consolidate.rs:NoopLlmProvider` groups by channel only (not by time window or sender). `OpenAiLlmProvider` sends all messages in a batch to the LLM without grouping. Topics are named `consolidated/<channel>` instead of semantic topics like `user/k0/preferences`.
- **Impact:** Consolidation produces low-quality outputs that don't match the intended topic namespace convention.

#### 3. Web UI: applesauce dead dependencies
- **Spec says** (UI Spec §Architecture + §Principles): "Use applesauce — relay connections, event handling, and signing should use applesauce-core and applesauce-signers — not hand-rolled implementations."
- **Code does:** `applesauce-core` and `applesauce-signers` are in `package.json` but **never imported anywhere**. `lib/relay.ts` (594 lines) reimplements relay connection, NIP-42 AUTH, subscription management. `lib/nostr.ts` (478 lines) reimplements NIP-07 and NIP-46 signing.
- **Impact:** 1072 lines of hand-rolled code duplicating maintained library functionality. Maintenance burden, potential correctness issues.

### P1 — Important Gaps

#### 4. Create memory UI missing from web
- **Spec says** (UI Spec §Memories): "Create new memory (topic, summary, detail, tier, confidence)"
- **Code does:** Memories page has list, filter, and delete — but no create/edit form. `relay.ts` has `storeMemory()` method but no UI invokes it.
- **Impact:** Users can only create memories via CLI or MCP, not via the web dashboard.

#### 5. Consolidation trigger button missing from web
- **Spec says** (UI Spec §Chat): "Trigger consolidation button"
- **Code does:** Messages page has no consolidation button. The HTTP endpoint `POST /memory/api/consolidate` exists and works, but no UI calls it.
- **Impact:** Consolidation can only be triggered via CLI.

#### 6. Ad-hoc npub sets not implemented
- **Spec says** (Architecture §Ad-hoc npub Sets, Session ID Model): "Session ID: hash(sorted npubs) — deterministic, same participants always produce the same scope." Marked as "📋 planned".
- **Code does:** `session.rs:resolve_session()` handles public, npub, named group, and nostr_group formats. There is no hash-based session resolution for ad-hoc npub sets.
- **Impact:** Multi-party DM sessions can't be scoped properly. Only binary (single npub) private sessions work.

#### 7. Group management UI missing from web
- **Spec says** (UI Spec §Groups): "Create group, add/remove members"
- **Code does:** Groups page is read-only. Shows group tree and member list, but no create/edit/add/remove functionality.
- **Impact:** Group management only possible via CLI.

#### 8. Consolidation: No --dry-run, --older-than, --tier flags
- **Spec says** (Memory Consolidation doc §CLI Commands): `nomen consolidate --dry-run`, `--older-than 30m`, `--tier private`
- **Code does:** `cmd_consolidate()` in `main.rs` accepts only `--llm-provider` and `--batch-size` flags. No dry-run, no time filter, no tier filter.
- **Impact:** Users can't preview or selectively run consolidation.

#### 9. Relay auto-reconnect missing from web
- **Spec says** (UI Spec §Reliability): "Auto-reconnect: Exponential backoff on relay disconnect. Status indicator in sidebar."
- **Code does:** `lib/relay.ts` has no reconnect logic. If WebSocket drops, the user must reload. No status indicator in sidebar.
- **Impact:** Poor user experience on unreliable connections.

### P2 — Moderate Gaps

#### 10. Discovery & Trust Indexing — entirely unimplemented
- **Spec says** (Discovery & Trust RFC): Index kind 38990 (agent capabilities) and kind 1985 (trust attestations). New SurrealDB tables (`capabilities`, `attestations`, `trust_scores`). New CLI commands (`nomen agents`, `nomen trust`). Extended search with `--type` and `--min-trust` filters.
- **Code does:** Nothing. No tables, no sync, no CLI commands, no trust scoring.
- **Impact:** Expected — spec is an RFC for future work. But it means the "agent-aware knowledge index" vision is not yet real.

#### 11. Memory list: No --named, --ephemeral, --stats flags
- **Spec says** (Memory Consolidation doc §CLI Commands): `nomen list --named`, `--ephemeral`, `--stats`
- **Code does:** `cmd_list()` accepts only `--tier` filter. No distinction between named and ephemeral memories, no consolidation stats display.
- **Impact:** Can't easily see consolidation status or filter memory types.

#### 12. Delete: No --ephemeral --older-than bulk deletion
- **Spec says** (Memory Consolidation doc §CLI Commands): `nomen delete --ephemeral --older-than 7d`
- **Code does:** `cmd_delete()` accepts a single topic or event ID. No bulk/age-based deletion.
- **Impact:** Manual cleanup only.

#### 13. GroupMembers doesn't use ProfileCard
- **Spec says** (UI Spec §Shared Components, Web UI Audit §Component Audit): GroupMembers should use ProfileCard for consistency.
- **Code does:** `GroupMembers.svelte` renders npubs as plain compressed text. Members and Agents pages use ProfileCard correctly.
- **Impact:** Inconsistent UX, no profile pictures/names in group member lists.

#### 14. NIP-46 relay list not configurable in Settings
- **Spec says** (UI Spec §Settings): "NIP-46 relay list (for remote signing)"
- **Code does:** Settings page has relay URL and API base URL only. NIP-46 relays are hardcoded in `LoginModal.svelte` (`wss://relay.nsec.app`, `wss://relay.damus.io`).
- **Impact:** Users on custom NIP-46 setups can't configure signing relays.

#### 15. Shared profile cache not implemented
- **Spec says** (UI Spec §Profile Cache): "Single shared profile store. All pages that display pubkeys read from this store — no duplicate fetches."
- **Code does:** Members page fetches profiles via relay, Agents page fetches via `fetchProfileMetadata()` per-agent. No shared profile store. `lib/nostr.ts` has a localStorage cache with 1hr TTL, but it's not integrated as a shared Svelte store.
- **Impact:** Duplicate profile fetches, inconsistent profile data across pages.

### P3 — Minor / Future

#### 16. Dreaming & Sleep-Inspired Memory — pure research
- **Spec says** (Dreaming doc): Three-phase dream cycle (NREM consolidation, REM associative dreaming, Journal). Dream memory category with confidence 0.1-0.5 and fast decay.
- **Code does:** Nothing. This is explicitly a research/design document.
- **Impact:** None now — this is aspirational architecture.

#### 17. Memory Consolidation: snow:consolidated_from, snow:consolidated_at tags
- **Spec says** (Memory Consolidation doc §Named Memory Schema): Tags `["snow:consolidated_from", "12"]` and `["snow:consolidated_at", "1772630000"]` on consolidated memories.
- **Code does:** `consolidate.rs` creates `consolidated_from` graph edges in SurrealDB but does not add these tags to the published Nostr events.
- **Impact:** Relay-side events don't carry consolidation provenance.

#### 18. Importance scoring / access tracking
- **Spec says** (Memory Survey Reflections §What Nomen Should Add): LLM-assigned importance scoring (1-10), `last_accessed`/`access_count` fields for decay calculations.
- **Code does:** `confidence` field exists but is author-assigned, not query-time. No `last_accessed` or `access_count` fields in the schema.
- **Impact:** Cannot implement decay-based retrieval ranking.

#### 19. Temporal validity fields
- **Spec says** (Memory Survey Reflections §What Nomen Should Add): `valid_from`/`valid_until` fields for time-scoped facts.
- **Code does:** Only `created_at` exists. No temporal validity.
- **Impact:** Cannot mark memories as temporally bounded.

#### 20. Conflict detection
- **Spec says** (Memory Survey Reflections §What Nomen Should Add): When new memory contradicts existing, create explicit `contradicts` edge.
- **Code does:** The `references` graph edge table exists with support for relation types, but no code creates `contradicts` edges. Version checking just keeps the latest.
- **Impact:** Silent overwrites rather than explicit conflict tracking.

---

## Incomplete Specs (code exists but spec is vague/missing)

### 1. HTTP API not fully specced
The HTTP server (`http.rs`, 404 lines) exposes 11+ endpoints, but no spec document lists the full API surface, request/response schemas, or error codes. The architecture doc mentions it briefly ("REST API for remote agents and web UIs") but provides no detail.

**Endpoints lacking specification:**
- `POST /memory/api/store` — request body schema undocumented
- `POST /memory/api/ingest` — request body schema undocumented
- `DELETE /memory/api/memories/{topic}` — deletion semantics undocumented
- `POST /memory/api/send` — request body schema undocumented
- `GET /memory/api/memories` — query parameters undocumented

### 2. Context-VM protocol not specced
`contextvm.rs` (602 lines) implements a full Nostr-native request/response protocol using kind 21900/21901 with NIP-44 encryption, rate limiting, and 8 actions. No spec document describes this protocol. The architecture doc mentions it in one line: "NIP-44 encrypted request/response events for pure-Nostr agents."

**Missing documentation:**
- Kind numbers and their semantics
- Request/response JSON schemas
- Rate limiting behavior
- Allowed npub authorization model
- Expiration tag handling

### 3. MCP tool schemas not specced
`mcp.rs` (797 lines) implements 9 MCP tools. The architecture doc lists tool names but doesn't document parameters, return types, or error handling. The tool schemas are only defined in code (JSON objects in `handle_tools_list()`).

### 4. Snowclaw adapter not specced
`snowclaw_adapter.rs` (100+ lines) implements a `Memory` trait bridge to Snowclaw's memory interface. Feature-gated behind `snowclaw`. No spec describes the integration contract, category mapping, or tier behavior.

### 5. Web UI data flow between relay and HTTP API
The web UI uses both direct relay WebSocket connections (for memories, profiles, groups, messages) and HTTP REST API (for search, entities, consolidation). No spec clearly delineates which operations use which path and why. The UI Spec's data sources table is the closest, but doesn't explain the reasoning or edge cases.

---

## Implementation Bugs / Deviations

### 1. Messages page shows NIP-29 group messages, not raw_messages
- **Spec says** (UI Spec §Chat): Shows ingested messages from `raw_message` table with consolidation status and trigger.
- **Code does:** Messages page fetches NIP-29 kind 9 group chat messages directly from relay. These are Nostr group messages, not the `raw_message` records from `POST /memory/api/ingest`. The consolidation badge and filter concepts don't apply to NIP-29 messages.
- **Deviation type:** Conceptual mismatch — two different data models conflated.

### 2. Architecture doc line counts are slightly off
The module map in `docs/architecture.md` lists line counts that are now stale:
- `http.rs` listed as 376, actual is 404
- `snowclaw_adapter.rs` listed as 456, actual appears shorter (feature-gated, partial)
- Total listed as ~7400 LOC — likely still approximately correct.
- **Minor:** These are informational, not functional.

### 3. Config doc says `memory.consolidation` section; code uses `consolidation`
- **Spec says** (Memory Consolidation doc §Config): `[memory.consolidation]` with fields `enabled`, `interval_hours`, `ephemeral_ttl_minutes`, `max_ephemeral_count`, `dry_run`.
- **Code does:** `config.rs` has `[consolidation]` section (not `[memory.consolidation]`) with only `provider`, `model`, `api_key_env`, `base_url` fields. No `enabled`, `interval_hours`, `ephemeral_ttl_minutes`, `max_ephemeral_count`, or `dry_run`.
- **Deviation type:** Schema mismatch between spec and implementation.

### 4. CLAUDE.md says "Do not create a library crate yet" — but lib.rs exists
- **CLAUDE.md** (§Do NOT): "Do not create a library crate yet — just a binary"
- **Code does:** `Cargo.toml` defines both `[lib]` and `[[bin]]`. `lib.rs` exposes `Nomen` struct as public API.
- **Status:** CLAUDE.md is outdated — the library crate was clearly an intentional architectural evolution. CLAUDE.md should be updated.

### 5. Embedding dimensions hardcoded to 1536 in schema
- **db.rs schema:** `DEFINE INDEX idx_memory_embedding ON memory FIELDS embedding HNSW DIMENSION 1536 DIST COSINE`
- **config.rs:** `dimensions` is configurable (default 1536 but can be overridden)
- **Issue:** If someone configures a different embedding model with different dimensions (e.g., 768 for smaller models), the HNSW index dimension is hardcoded and will reject the embeddings.

### 6. NIP-17 DM handling uses gift_wrap from nostr-sdk
- **Code does:** `send.rs:send_dm()` uses `client.gift_wrap()` which handles NIP-17 correctly.
- **Spec says:** Nothing specific about the implementation path.
- **Status:** Working correctly, just noting the implementation choice is sound.

---

## Spec Completeness Assessment

### Well-Specced Areas (sufficient to guide a developer)

| Area | Spec Quality | Notes |
|------|-------------|-------|
| Nostr Memory Event Format | Excellent | `nostr-memory-spec.md` is thorough with JSON examples, tag tables, content schemas |
| Architecture Overview | Good | `architecture.md` gives clear module map, data flow, and tier model |
| Memory Tiers | Good | Clear table with encryption and access rules |
| Groups (Named) | Good | Config format, CLI commands, NIP-29 mapping |
| Memory Consolidation | Good | Clear process, content schema, event lifecycle, CLI commands |
| Web UI Structure | Good | Pages, components, data sources table, state management |
| Discovery & Trust RFC | Excellent | Full SurrealDB schema, CLI commands, sync behavior, trust formula |

### Under-Specced Areas (needs more detail)

| Area | Gap | Recommendation |
|------|-----|----------------|
| HTTP API | No endpoint reference | Write an OpenAPI spec or at minimum a table of endpoints, methods, request/response schemas |
| Context-VM Protocol | No spec at all | Write a protocol spec: kind numbers, JSON schemas, auth model, rate limits |
| MCP Tool Schemas | Only tool names listed | Document parameters, return types, and error codes for each tool |
| Snowclaw Integration | No spec | Document the Memory trait contract, category mapping, and tier behavior |
| Error Handling Strategy | Not documented | Document expected error types, retry behavior, and user-facing error messages |
| Deployment / Operations | Not documented | Document how to run in production: config, relay setup, embedding API keys, systemd units |

### Missing Specs Entirely

| Topic | Status | Notes |
|-------|--------|-------|
| NIP-09 deletion behavior | Not specced | When/how deletion events are published, what gets cleaned up locally vs relay |
| Search ranking algorithm | Not specced | Vector weight, text weight, confidence boost, recency — all implementation details |
| Access control policy | Not specced | `access.rs` implements tier-based checks but no spec defines the policy |
| Migration guide | Not specced | `migrate.rs` handles SQLite → SurrealDB but no user-facing docs |
| Multi-relay support | Not specced | Code supports one relay at a time; specs mention "configured relays" (plural) in the Discovery RFC |

---

## Recommendations

### Priority 1: Fix consolidation pipeline (addresses P0 #1, #2, #8)

The consolidation pipeline is the feature that transforms Nomen from a memory dump into a knowledge system. Current implementation is minimal.

1. Add NIP-09 deletion after consolidation (publish kind 5 events for consumed ephemerals)
2. Implement time-window grouping (sender + 4hr blocks for private, group_id + 4hr blocks for group)
3. Add semantic topic naming (not `consolidated/<channel>` but `user/<name>/<aspect>`)
4. Add `--dry-run`, `--older-than`, `--tier` flags to CLI
5. Add `snow:consolidated_from` and `snow:consolidated_at` tags to published events

### Priority 2: Resolve web UI dependency situation (addresses P0 #3)

Pick one path:
- **Option A:** Refactor `lib/relay.ts` and `lib/nostr.ts` to use applesauce-core and applesauce-signers (as the UI Spec mandates). Delete 1072 lines of hand-rolled code.
- **Option B:** Remove applesauce-core and applesauce-signers from `package.json` and update the UI Spec to reflect the hand-rolled approach. Add reconnect logic to `lib/relay.ts`.

### Priority 3: Web UI feature completion (addresses P1 #4, #5, #7)

1. Add create-memory form to Memories page (invoke `relay.storeMemory()`)
2. Add consolidation trigger button to Messages page (call `POST /memory/api/consolidate`)
3. Add group create/edit/member-management to Groups page
4. Switch GroupMembers to use ProfileCard component
5. Add relay connection status indicator to sidebar

### Priority 4: Spec the unspecced (addresses Incomplete Specs section)

1. Write HTTP API reference (endpoint table with request/response schemas)
2. Write Context-VM protocol spec (kind numbers, auth model, JSON schemas)
3. Document MCP tool parameters and return types
4. Update CLAUDE.md to reflect current state (library crate exists, first prototype scope is exceeded)

### Priority 5: Ad-hoc npub sets (addresses P1 #6)

Implement hash-based session resolution for multi-party DM conversations:
1. Add `resolve_session()` case for `sha256:` prefixed session IDs
2. Add session table lookup by hash
3. Add NIP-17 multi-party DM handling

### Priority 6: Discovery & Trust RFC — Phase 1 (addresses P2 #10)

Start with read-only indexing:
1. Add `capabilities` and `attestations` tables to SurrealDB schema
2. Add kind 38990 + 1985 to relay sync filter
3. Add `nomen agents list/find/show` CLI commands
4. Add `nomen trust score/attestations` CLI commands

### Priority 7: Hardening

1. Make HNSW dimension configurable (match embedding config to schema)
2. Add relay auto-reconnect with exponential backoff to web UI
3. Add shared profile cache store
4. Add NIP-46 relay configuration to Settings page
5. Add global error boundary to web UI

---

## Appendix: File Coverage Matrix

### Rust Source Files (21 files, ~7400 LOC)

| File | Specced | Implemented | Tests | Notes |
|------|---------|-------------|-------|-------|
| main.rs | ✅ | ✅ | ❌ | CLI entry, all commands work |
| lib.rs | ✅ | ✅ | ❌ | Public API facade |
| db.rs | ✅ | ✅ | ❌ | SurrealDB schema + CRUD, hardcoded HNSW dims |
| search.rs | ✅ | ✅ | ❌ | Hybrid search works |
| relay.rs | ✅ | ✅ | ❌ | NIP-42/44 via nostr-sdk |
| memory.rs | ✅ | ✅ | ❌ | Event parsing works |
| send.rs | ✅ | ✅ | ✅ | NIP-17 DM, NIP-29 group, kind 1 |
| session.rs | ✅ | ⚠️ | ✅ | Missing ad-hoc npub sets |
| mcp.rs | ⚠️ | ✅ | ❌ | 9 tools, no spec for schemas |
| http.rs | ⚠️ | ✅ | ❌ | 11+ endpoints, no API spec |
| contextvm.rs | ⚠️ | ✅ | ❌ | Full protocol, no spec doc |
| ingest.rs | ✅ | ✅ | ❌ | Simple passthrough |
| consolidate.rs | ⚠️ | ⚠️ | ❌ | Missing NIP-09, grouping, topic naming |
| embed.rs | ✅ | ✅ | ❌ | OpenAI-compatible + noop |
| entities.rs | ✅ | ✅ | ✅ | Heuristic extraction |
| groups.rs | ✅ | ✅ | ✅ | Hierarchical, NIP-29 mapping |
| config.rs | ⚠️ | ✅ | ❌ | Doesn't match consolidation spec fields |
| access.rs | ⚠️ | ✅ | ✅ | Works but no policy spec |
| display.rs | ✅ | ✅ | ❌ | CLI formatting |
| migrate.rs | ⚠️ | ✅ | ❌ | Feature-gated, no docs |
| snowclaw_adapter.rs | ❌ | ✅ | ❌ | Feature-gated, no spec |

### Web UI Files (33 files, ~3500 LOC)

| Area | Specced | Implemented | Notes |
|------|---------|-------------|-------|
| Memories page | ✅ | ⚠️ | Missing create UI |
| Search page | ✅ | ✅ | Works as specced |
| Messages page | ✅ | ⚠️ | Shows NIP-29 not raw_messages; no consolidation trigger |
| Members page | ✅ | ✅ | Works as specced |
| Groups page | ✅ | ⚠️ | Read-only, no management UI |
| Agents page | ✅ | ✅ | Works as specced |
| Settings page | ✅ | ⚠️ | Missing NIP-46 relay config |
| Landing page | ✅ | ✅ | Works |
| ProfileCard | ✅ | ✅ | Well-designed shared component |
| LoginModal | ✅ | ✅ | NIP-07 + NIP-46 with QR |
| Relay client | ✅ | ⚠️ | Hand-rolled, should use applesauce |
| Auth/signing | ✅ | ⚠️ | Hand-rolled, should use applesauce |
| Auto-reconnect | ✅ | ❌ | Not implemented |
| Error boundary | ✅ | ❌ | Not implemented |
| Shared profile cache | ✅ | ❌ | Not implemented |

---

*Generated 2026-03-06 by automated audit. All source files read in full.*
