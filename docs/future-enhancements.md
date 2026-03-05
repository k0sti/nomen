# Nomen — Future Enhancements

**Date:** 2026-03-05
**Author:** k0 (requirements) + Claude Opus 4.6 (documentation)

---

## 1. Agent Messaging Tool (`nomen_send`)

### Problem

Agents currently have no way to send messages to specific recipients through Nomen. The system ingests messages but doesn't originate them. For multi-agent coordination, agents need a tool to communicate with users (npub), groups, or publicly.

### Design

**New MCP tool:** `nomen_send`

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `recipient` | string | Yes | Target: npub (DM), group ID, or `"public"` |
| `content` | string | Yes | Message body |
| `metadata` | object | No | Platform-specific extras |

**Recipient routing:**

| Recipient | Nostr Kind | Encryption | Storage |
|-----------|-----------|------------|---------|
| `npub1...` | Kind 4 (DM) or NIP-44 wrapped | NIP-44 encrypted to recipient | `raw_message` with scope=npub, tier=private |
| `group:<id>` | Kind 9 (group chat) | None (relay-auth gated) | `raw_message` with scope=group_id, tier=group |
| `"public"` | Kind 1 (note) | None | `raw_message` with scope="", tier=public |

**Behavior:**
1. Validate recipient format
2. Build appropriate Nostr event (kind 4/9/1)
3. Publish via RelayManager
4. Store locally as `raw_message` with `source="nomen"`, proper tier/scope
5. Return event ID + delivery status

**MCP tool schema:**
```json
{
  "name": "nomen_send",
  "description": "Send a message to a specific recipient (npub, group, or public)",
  "inputSchema": {
    "type": "object",
    "properties": {
      "recipient": {
        "type": "string",
        "description": "npub1... for DM, group:<id> for group, 'public' for broadcast"
      },
      "content": { "type": "string" },
      "metadata": { "type": "object" }
    },
    "required": ["recipient", "content"]
  }
}
```

**Context-VM action:** `send` with same parameters.

---

## 2. Session ID Model

### Problem

Nomen currently treats tier and scope as separate concepts configured per-call. Agents need a single "session identifier" that automatically encodes visibility, routing, and recall context. This simplifies the API and prevents misconfiguration (e.g., storing private data with public tier).

### Design

**Session ID formats:**

| Format | Type | Tier | Scope | Example |
|--------|------|------|-------|---------|
| `npub1...` | Private session (DM) | private | `<npub hex>` | `npub1abc...` |
| `hash(sorted npubs)` | Multi-party private | private | `<hash>` | `sha256("npub1abc,npub1def")` |
| `channel:<group_name>` | Group session | group | `<group_id>` | `telegram:techteam`, `nostr:inner-circle` |
| `"public"` | Public session | public | `""` | `public` |

**Session ID determines:**
1. **Message ingestion routing** — ingest tool uses session to set tier/scope automatically
2. **Memory recall filtering** — search scoped to session's visibility level
3. **Consolidation grouping** — messages grouped by session for consolidation
4. **Memory storage tier** — new memories inherit the session's tier

**Resolution logic:**
```rust
pub struct ResolvedSession {
    pub tier: String,        // "public" | "group" | "private"
    pub scope: String,       // "" | group_id | npub_hex
    pub group_id: Option<String>,
    pub participants: Vec<String>,  // npubs in session
}

pub fn resolve_session(session_id: &str, groups: &GroupStore) -> Result<ResolvedSession> {
    if session_id == "public" {
        // Public session
    } else if session_id.starts_with("npub1") {
        // Private DM session
    } else if session_id.starts_with("channel:") {
        // Group session — look up group, resolve scope
    } else {
        // Multi-party hash — resolve from known sessions
    }
}
```

**API changes:**
- All MCP tools gain optional `session_id` parameter
- If `session_id` provided, `tier` and `scope` are derived automatically
- If both `session_id` and `tier`/`scope` provided, explicit values override
- Context-VM requests include `session_id` in params

**Storage:**
- Active sessions tracked in a `session` SurrealDB table
- Maps session_id → resolved tier/scope/participants
- Used for consolidation grouping and message routing

---

## 3. UI Architecture (MVC)

### Problem

Nomen currently only has a CLI interface. For broader adoption, it needs a visual UI (TUI or web). The UI should be testable independently from the Nomen core.

### Design: Model-View-Controller

```
┌─────────────────────────────────────────────┐
│                   View                       │
│  ┌─────────────┐  ┌──────────────────────┐  │
│  │  TUI (ratatui)│  │  Web SPA (future)  │  │
│  └──────┬───────┘  └──────────┬──────────┘  │
│         │                     │              │
│  ┌──────▼─────────────────────▼──────────┐  │
│  │           Controller Layer             │  │
│  │  Command handlers: map user actions    │  │
│  │  to Nomen API calls                    │  │
│  └──────────────────┬────────────────────┘  │
│                     │                        │
│  ┌──────────────────▼────────────────────┐  │
│  │        Model (Nomen library crate)     │  │
│  │  nomen::Nomen — search, store, ingest  │  │
│  │  Already exists in src/lib.rs          │  │
│  └───────────────────────────────────────┘  │
└─────────────────────────────────────────────┘
```

**Components:**

| Layer | Crate | Responsibility |
|-------|-------|---------------|
| **Model** | `nomen` (lib) | Data operations: store, search, ingest, consolidate, groups, entities |
| **Controller** | `nomen-ui-core` | Command handlers that map user actions to Nomen API calls. Framework-agnostic. |
| **View: TUI** | `nomen-tui` | Terminal UI using `ratatui`. Renders memory lists, search results, group trees. |
| **View: Web** | `nomen-web` (future) | Web SPA (Leptos/Dioxus) for browser-based interaction. |

**Controller commands:**
```rust
pub enum UiCommand {
    Search { query: String, scope: Option<String> },
    StoreMemory { topic: String, summary: String, detail: String },
    ListMemories { tier: Option<String> },
    IngestMessage { source: String, content: String },
    Consolidate { channel: Option<String> },
    ManageGroup { action: GroupAction },
    ViewEntity { name: String },
    DeleteMemory { topic: String },
}
```

### Test Flows

Each test flow should be independently testable at the controller layer (no UI rendering needed):

#### Flow 1: Store → Search → Retrieve
```
1. Controller::store_memory(topic, summary, detail)
2. Controller::search(query) → verify result contains stored memory
3. Controller::get_memory(topic) → verify full content
```

#### Flow 2: Ingest → Consolidate → Verify
```
1. Controller::ingest_messages(source, messages[])
2. Controller::consolidate(channel)
3. Controller::list_memories() → verify consolidated memories exist
4. Controller::get_messages(consolidated_only=true) → verify originals marked
```

#### Flow 3: Group lifecycle
```
1. Controller::create_group(id, name, members)
2. Controller::add_member(group_id, npub)
3. Controller::store_memory(topic, summary, tier=group, scope=group_id)
4. Controller::search(query, scope=group_id) → verify group-scoped result
5. Controller::search(query, scope=other_group) → verify no result
```

#### Flow 4: MCP tool call verification
```
1. Send JSON-RPC request to MCP server (mock stdio)
2. Verify response format matches MCP 2.0 spec
3. Verify tool results contain expected data
4. Verify error responses have correct error codes
```

#### Flow 5: Context-VM request verification
```
1. Create NIP-44 encrypted request event (kind 21900)
2. Submit to Context-VM handler
3. Verify response event (kind 21901) is encrypted
4. Decrypt and verify response matches expected format
```

---

## 4. Prioritized Roadmap

### v0.2 — Messaging & Session Model

**Goal:** Enable agent-to-agent and agent-to-user communication with automatic scope management.

| Task | Priority | Est. Effort | Dependencies |
|------|----------|-------------|--------------|
| Implement `nomen_send` MCP tool | High | 2 days | RelayManager.publish() |
| Implement `nomen_send` Context-VM action | High | 1 day | nomen_send MCP |
| Session ID resolution logic | High | 2 days | GroupStore |
| Add `session_id` parameter to all MCP tools | High | 1 day | Session ID resolution |
| Add `session` SurrealDB table | Medium | 0.5 day | Schema |
| Fix scope prefix matching in search | High | 0.5 day | — |
| Fix Context-VM subscription filter (`.pubkey()` → `p` tag) | High | 0.5 day | — |
| Add `snow:supersedes` tag to published events | Medium | 0.5 day | — |
| Implement real LLM provider (OpenRouter/Anthropic) | High | 2 days | consolidate.rs |
| Add live relay subscription for incremental sync | High | 1.5 days | relay.rs |
| Schema: change `created_at`/`updated_at` to `datetime` | Medium | 1 day | db.rs migration |

**Deliverables:** Agents can send messages, sessions auto-determine scope, real consolidation works.

### v0.3 — UI + Test Flows

**Goal:** Visual interface and comprehensive test coverage.

| Task | Priority | Est. Effort | Dependencies |
|------|----------|-------------|--------------|
| Create `nomen-ui-core` controller crate | High | 3 days | lib.rs API stable |
| Implement TUI with `ratatui` | High | 5 days | Controller |
| TUI: Memory list view with filtering | High | 2 days | TUI scaffold |
| TUI: Search interface with live results | High | 2 days | TUI scaffold |
| TUI: Group management view | Medium | 1.5 days | TUI scaffold |
| TUI: Message timeline view | Medium | 1.5 days | TUI scaffold |
| Integration tests: all 5 test flows | High | 3 days | Controller |
| Unit tests: memory.rs, search.rs, consolidate.rs | High | 2 days | — |
| MCP protocol conformance tests | Medium | 1.5 days | — |
| Context-VM round-trip tests | Medium | 1.5 days | — |
| HTTP transport for MCP server | Medium | 2 days | mcp.rs |

**Deliverables:** TUI working, test coverage >80%, HTTP MCP transport.

### v0.4 — Production Hardening

**Goal:** Reliability, observability, and operational maturity.

| Task | Priority | Est. Effort | Dependencies |
|------|----------|-------------|--------------|
| Connection resilience (auto-reconnect relays) | High | 2 days | relay.rs |
| Exponential backoff for API calls (embedding, LLM) | High | 1 day | embed.rs, consolidate.rs |
| Structured error types (replace anyhow in lib) | Medium | 2 days | All modules |
| Metrics/observability (tracing spans, counters) | Medium | 2 days | tracing |
| Rate limiting for Context-VM | High | 1 day | contextvm.rs |
| Expiration validation on Context-VM requests | Medium | 0.5 day | contextvm.rs |
| Config validation on startup | Medium | 1 day | config.rs |
| Graceful shutdown (flush pending, close DB) | High | 1 day | main.rs |
| Database compaction/vacuum scheduling | Medium | 0.5 day | db.rs |
| `--around` message query | Low | 1 day | db.rs |
| LLM-powered entity extraction (Phase 2) | Medium | 3 days | entities.rs |
| Conflict detection (`contradicts` edges) | Medium | 2 days | db.rs, consolidate.rs |
| Temporal validity fields (`valid_from`/`valid_until`) | Low | 1 day | Schema |
| Access tracking (`last_accessed`, `access_count`) | Low | 1 day | Schema |

**Deliverables:** Production-ready daemon, auto-recovery, observability.

### v1.0 — Feature Complete

**Goal:** All spec items implemented, documented, and tested.

| Task | Priority | Est. Effort | Dependencies |
|------|----------|-------------|--------------|
| Web UI (Leptos/Dioxus SPA) | Medium | 10 days | Controller stable |
| Multi-relay federation | Medium | 3 days | relay.rs |
| Snowclaw `promote()` and `reindex()` | Low | 1 day | snowclaw_adapter.rs |
| `resources/list` and `prompts/list` MCP methods | Low | 1 day | mcp.rs |
| Plugin system for custom embedders | Low | 2 days | embed.rs |
| Import/export (JSON, SQLite) | Low | 2 days | lib.rs |
| Man page and shell completions | Low | 1 day | clap |
| Comprehensive documentation (API docs, user guide) | High | 3 days | — |
| Performance benchmarks | Medium | 2 days | — |
| Security audit (NIP-44 handling, input sanitization) | High | 2 days | — |

**Deliverables:** Full spec compliance, multi-interface, documented, benchmarked.

---

## 5. Research-Informed Enhancements (from Academic Survey)

Based on the agentic memory research landscape (Hu et al., 2025):

| Enhancement | Source | Priority | Notes |
|-------------|--------|----------|-------|
| Importance scoring at creation (1-10) | Mem0, MemGPT | v0.4 | Used in search ranking alongside confidence |
| Temporal validity (`valid_from`/`valid_until`) | Survey consensus | v0.4 | Memories can expire or have future activation |
| Access tracking for decay | MemGPT (OS-inspired) | v0.4 | `last_accessed` + `access_count` for relevance decay |
| LLM-powered fact extraction at ingest | Mem0, A-MEM | v0.4 | Replace heuristic entity extraction |
| Contradiction detection | G-Memory | v0.4 | Create `contradicts` edges during consolidation |
| Aggregated search (merge related hits) | Survey consensus | v1.0 | Post-process search to cluster related memories |
| Operation logging (audit trail) | Collaborative Memory | v1.0 | Track all operations for debugging and training |
