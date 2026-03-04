# CLAUDE.md — Nomen CLI Implementation Guide

## Project

**Nomen** is a Rust CLI tool for managing Nostr-native memory events. This is the first prototype — it connects to a Nostr relay and lists all memory events for given nsec keys.

## Architecture

Read these docs thoroughly before implementing:
- `docs/nostr-memory-spec.md` — Full Nostr event schema for memory
- `docs/memory-tiers.md` — Three-tier visibility system + ranking

## First Prototype Scope

Build a CLI binary (`nomen`) that:

1. **Accepts CLI args:**
   - `--relay <url>` (default: `wss://zooid.atlantislabs.space`)
   - `--nsec <nsec1...>` (repeatable, one or more nsec keys)
   - Subcommand: `list` — list all memory events

2. **Connects to relay:**
   - Use `nostr-sdk` for relay connection
   - Handle NIP-42 AUTH automatically (nostr-sdk does this when you provide keys)
   - Connect with timeout (10s)

3. **Subscribes to memory events:**
   - Filter: `{"kinds": [30078], "authors": [<pubkeys-from-nsecs>]}`
   - Also fetch kind 4129 (agent lessons) if present
   - Wait for EOSE (end of stored events), then process

4. **Parses and displays:**
   - Parse `d` tag to extract topic/namespace
   - Parse `snow:tier` tag for visibility tier
   - Parse content JSON for summary/detail
   - Parse `snow:confidence`, `snow:model`, `snow:version`
   - Display in a clean table or structured format

5. **Output format:**
   ```
   Memory Events for <npub>
   ═══════════════════════════════════════════
   
   [public] rust/error-handling (v1, confidence: 0.92)
     Model: anthropic/claude-opus-4-6
     Summary: Use anyhow for application errors
     Created: 2026-02-18 14:30:00 UTC
   
   [group:techteam] project-decisions (v3, confidence: 0.88)
     Model: anthropic/claude-sonnet-4-6
     Summary: Use NIP-78 for all persistent memory
     Created: 2026-02-20 09:15:00 UTC
   
   Total: 42 memories (38 public, 3 group, 1 private)
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

### NIP-42 AUTH

nostr-sdk handles AUTH automatically when you create a `Client` with `Keys`. Just ensure you call `client.add_relay(url).await` and `client.connect().await`.

### Multiple nsec keys

Each nsec corresponds to a different agent/user identity. Subscribe to events from ALL of them in a single filter using the `authors` array.

## Build & Test

```bash
cargo check
cargo build
cargo run -- list --relay wss://zooid.atlantislabs.space --nsec nsec1...
```

## Code Style

- Use `anyhow::Result` for error handling
- Use `tracing` for logging (not `println!` for debug)
- Use `clap` derive API for CLI args
- Keep it simple — this is a prototype, not a framework

## Do NOT

- Do not implement write/publish functionality yet
- Do not implement search/ranking yet
- Do not create a library crate yet — just a binary
- Do not add unnecessary abstractions
