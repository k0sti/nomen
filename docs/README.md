# Nomen Documentation

Nostr-native agent memory system.

## Specs

These describe the current system. Together they should be sufficient to recreate the codebase.

| Doc | Contents |
|---|---|
| [spec/overview.md](spec/overview.md) | System purpose, data flow, crate structure, storage |
| [spec/data-model.md](spec/data-model.md) | Memory events (31234), collected messages (30100), tags, NIP alignment |
| [spec/api.md](spec/api.md) | Canonical API reference — all operations, params, responses |
| [spec/consolidation.md](spec/consolidation.md) | Pipeline: collection → grouping → extraction → merge → storage → cleanup |
| [spec/identity.md](spec/identity.md) | Multi-user identity, access control, groups, encryption |
| [spec/transport.md](spec/transport.md) | MCP, HTTP, ContextVM, socket — how they map to dispatch |
| [spec/security.md](spec/security.md) | Auth, encryption model, key management |
| [spec/filesystem.md](spec/filesystem.md) | Bidirectional filesystem sync (markdown ↔ memories) |

## Design

Forward-looking or conceptual docs that inform direction.

| Doc | Contents |
|---|---|
| [design/dreaming.md](design/dreaming.md) | Sleep-inspired associative memory discovery |
| [design/circles.md](design/circles.md) | Circle encryption design |
