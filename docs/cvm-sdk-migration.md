# Nomen → rust-contextvm-sdk Migration Design

**Date:** 2026-03-11
**Status:** Draft

## Summary

Replace nomen's custom ContextVM implementation (`src/contextvm.rs`, custom kinds 21900/21901, NIP-44 direct encryption) with the standardized `rust-contextvm-sdk` crate, which implements the full ContextVM protocol (kind 25910, NIP-59 gift wrap, server announcements, discovery).

## Current State

### What nomen has today

**`src/contextvm.rs`** — Custom request/response server:
- Kinds 21900 (request) / 21901 (response) — **non-standard**, nomen-specific
- NIP-44 direct encryption (no gift wrapping)
- Custom tags: `t:nomen-request`, `t:nomen-response`
- `ContextVmServer` struct with rate limiting, ACL (allowed npubs), action dispatch
- 13 actions: search, store, ingest, entities, consolidate, messages, groups, send, delete, list, sync, embed, prune

**`src/mcp.rs`** — MCP JSON-RPC over stdio:
- Full MCP tools/list + tools/call implementation
- Same 13 tools as contextvm.rs (nomen_search, nomen_store, etc.)
- Runs as stdio server for local agents

**`src/relay.rs`** — Nostr relay management:
- `RelayManager` wrapping nostr-sdk Client
- Publish, subscribe, NIP-44 encrypt/decrypt
- Uses nostr-sdk 0.39

### What rust-contextvm-sdk provides

- Standard ContextVM protocol (kind 25910 ephemeral, kind 1059 gift wrap)
- `NostrServerTransport` — handles subscriptions, sessions, encryption negotiation
- `NostrMCPGateway` — bridges any MCP server to Nostr (exposes tools over the network)
- `NostrMCPProxy` — connects to remote CVM servers as local MCP
- Server announcements (kind 11316) + capability listings (11317-11320)
- Discovery module for finding servers on relays
- NIP-44 + NIP-59 encryption, session management
- Uses nostr-sdk 0.43

## Design

### Dependency Setup

```toml
# Cargo.toml
[dependencies]
contextvm-sdk = { git = "https://github.com/k0sti/rust-contextvm-sdk", branch = "main" }
```

### Architecture

```
Before:                              After:
┌──────────────┐                    ┌──────────────┐
│ contextvm.rs │ kind 21900/01      │   Deleted    │
│ (custom)     │ NIP-44 only        │              │
└──────┬───────┘                    └──────────────┘
       │
┌──────┴───────┐                    ┌──────────────┐
│   mcp.rs     │ stdio JSON-RPC     │   mcp.rs     │ stdio JSON-RPC (unchanged)
│              │                    │              │
└──────┬───────┘                    └──────┬───────┘
       │                                   │
       ├─── Nomen core ◄──────────────────►├─── Nomen core
       │                                   │
┌──────┴───────┐                    ┌──────┴───────┐
│  relay.rs    │ nostr-sdk 0.39     │  relay.rs    │ nostr-sdk 0.43 (bump)
└──────────────┘                    └──────────────┘
                                           │
                                    ┌──────┴───────┐
                                    │  cvm.rs      │ NEW — thin adapter
                                    │  (SDK-based) │
                                    └──────────────┘
                                           │
                                    ┌──────┴───────┐
                                    │ contextvm-sdk│
                                    │ Gateway +    │
                                    │ Transport    │
                                    └──────────────┘
```

### New Module: `src/cvm.rs`

Replaces `contextvm.rs`. Thin adapter that:

1. Creates a `NostrMCPGateway` from the SDK
2. Registers nomen's tools as MCP tool handlers
3. Publishes server announcement (kind 11316) + tools list (kind 11317)
4. Handles incoming CVM requests via the SDK's transport layer

```rust
// src/cvm.rs — conceptual sketch

use contextvm_sdk::{
    gateway::NostrMCPGateway,
    signer,
    core::types::ServerInfo,
};

pub struct CvmServer {
    nomen: Nomen,
    gateway: NostrMCPGateway,
}

impl CvmServer {
    pub async fn new(
        nomen: Nomen,
        keys: Keys,
        relay_url: &str,
        encryption_mode: EncryptionMode,
    ) -> Result<Self> {
        let server_info = ServerInfo {
            name: "nomen".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            ..Default::default()
        };

        // The SDK's gateway handles:
        // - Subscribing to kind 25910 messages tagged to our pubkey
        // - Session management (multi-client)
        // - NIP-44/NIP-59 encryption negotiation
        // - Server announcements on relay
        let gateway = NostrMCPGateway::new(
            keys,
            &[relay_url],
            server_info,
            encryption_mode,
        ).await?;

        Ok(Self { nomen, gateway })
    }

    pub async fn run(&self) -> Result<()> {
        // Gateway dispatches incoming MCP tool calls to our handler
        self.gateway.serve(|method, params| {
            self.handle_tool_call(method, params)
        }).await
    }
}
```

### Tool Registration

The SDK gateway exposes MCP tools over Nostr. Nomen registers its tools using the same tool definitions already in `mcp.rs`:

- Reuse the `tools_list()` function from mcp.rs (or extract to shared module)
- Reuse the tool handler implementations (extract from `McpServer` into standalone functions)
- Both stdio MCP and CVM-over-Nostr call the same handlers

**Refactoring:** Extract tool definitions + handlers from `mcp.rs` into `src/tools.rs`:

```
src/tools.rs      — tool schemas + handler fns (shared)
src/mcp.rs        — stdio JSON-RPC wrapper (uses tools.rs)
src/cvm.rs        — SDK gateway wrapper (uses tools.rs)
```

### Relay Module Changes

`relay.rs` needs nostr-sdk bump 0.39 → 0.43. The SDK uses 0.43 so they must align.

Breaking changes to audit:
- `nostr-sdk` 0.40+ API changes (filter builder, event builder, tag API)
- The SDK manages its own relay connections; nomen's `RelayManager` continues to handle non-CVM relay operations (publishing memories, syncing, etc.)

### ACL & Rate Limiting

The current `contextvm.rs` has:
- `allowed_npubs: HashSet<String>` — whitelist
- `RateLimiter` — per-npub, N requests/minute

The SDK doesn't include ACL/rate limiting (it's application-level). **Keep these in `cvm.rs`** as middleware wrapping the gateway's incoming requests.

### Session Resolution

Current `contextvm.rs` passes `session_id` through to `Nomen::resolve_session()`. This stays — it's nomen business logic, not protocol.

### Config Changes

```toml
# nomen config additions
[contextvm]
enabled = true
relay = "wss://zooid.atlantislabs.space"
encryption = "optional"   # "optional" | "required" | "disabled"
allowed_npubs = [...]
rate_limit = 30           # requests/minute/npub
announce = true           # publish server announcement
```

### Migration Path

1. **Phase 1: Shared tools** — Extract tool defs/handlers from `mcp.rs` → `tools.rs`
2. **Phase 2: nostr-sdk bump** — 0.39 → 0.43, fix all breaking changes in `relay.rs` + other Nostr code
3. **Phase 3: SDK integration** — Add `contextvm-sdk` dep, create `cvm.rs`, wire up gateway
4. **Phase 4: Cleanup** — Delete `contextvm.rs`, update `main.rs` serve command to use `cvm.rs`
5. **Phase 5: Announce** — Enable server announcements, test discovery from TS SDK clients

### Breaking Changes

- **Protocol break:** kind 21900/21901 → kind 25910. Any existing CVM clients using the old protocol need updating.
- **Encryption:** NIP-44 direct → NIP-44 + optional NIP-59 gift wrap. More secure by default.
- **Tags:** Custom `t:nomen-request` → standard ContextVM tags.

Since nomen's CVM interface is internal (used only by k0's agents), this is a clean break with no external compatibility concerns.

### What Stays Unchanged

- `mcp.rs` stdio server (still works for local agents)
- `http.rs` HTTP/dashboard server
- All nomen core logic (search, store, ingest, consolidate, etc.)
- `relay.rs` for non-CVM relay operations (just bumped to 0.43)
- CLI commands

## Open Questions

1. **Shared relay connection?** Should nomen's `RelayManager` and the SDK share a nostr-sdk `Client`, or run independent connections? Independent is simpler; shared saves a connection.
2. **MCP type alignment:** The SDK defines its own JSON-RPC types. Should nomen's `mcp.rs` adopt them too, or keep its own? Adopting reduces duplication.
3. **Discovery scope:** Should nomen announce all tools publicly, or only to encrypted sessions? Configurable per-tool?
