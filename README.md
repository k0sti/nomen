# Nomen — Nostr Memory System

A standalone Rust toolkit for managing Nostr-native memory events. Extracted from the Snowclaw/ZeroClaw agent memory system.

## Overview

Nomen provides a CLI and library for working with NIP-78 (kind 30078) memory events on Nostr relays. It supports multi-agent memory with trust-ranked search, visibility tiers, and conflict resolution.

## Project Structure

```
nomen/
├── Cargo.toml          # Workspace root
├── crates/
│   └── nomen-core/     # Core types, event schema, ranking
├── src/
│   └── main.rs         # CLI binary
├── docs/
│   ├── nostr-memory-spec.md    # Full Nostr event specification
│   └── memory-tiers.md         # Memory tier system description
└── README.md
```

## Quick Start

```bash
# List all memory events for given nsec keys from a relay
nomen list --relay wss://zooid.atlantislabs.space --nsec nsec1...

# Search memories
nomen search "error handling" --relay wss://zooid.atlantislabs.space --nsec nsec1...

# Show memory details
nomen get <event-id>
```

## Origin

This project extracts and generalizes the memory system from:
- `snow-memory` crate (Snowclaw)
- `NostrMemory` backend (ZeroClaw)
- Memory-Context-Spec and Nostr-Events-Spec documentation

## License

MIT OR Apache-2.0
