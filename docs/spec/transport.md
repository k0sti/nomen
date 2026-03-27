# Transport Layer

All transports route through the same `api::dispatch()` function. Canonical operations are transport-independent. Each transport may expose additional transport-specific features.

## Transport Comparison

| | HTTP | MCP | ContextVM | Socket |
|---|---|---|---|---|
| **Primary use** | Remote generic | Local agent compat | Nostr-native remote | Local shared access |
| **Framing** | HTTP POST | JSON-RPC stdio | Nostr events (NIP-44/59) | Length-prefixed JSON |
| **Auth** | None (local/trusted) | N/A (local) | Nostr keypairs + ACL | Unix permissions |
| **Transport-specific** | Health, stats endpoints | Tool listing | Encryption, allowlist | Subscribe, push events |

## HTTP

First-class remote transport.

**Canonical dispatch:** `POST /memory/api/dispatch`

```json
{ "action": "memory.search", "params": { "query": "relay config", "limit": 10 } }
```

**Utility endpoints:**

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check |
| `/stats` | GET | Memory statistics |
| `/config` | GET | Current config |
| `/config/reload` | POST | Reload config |

## MCP (stdio)

Wrapper for agent frameworks that speak MCP (JSON-RPC over stdio).

- Tool names use underscore: `memory_search`, `memory_put`, etc.
- Same argument shapes as canonical API
- Calls `api::dispatch()` internally

## ContextVM (Nostr-Native)

Nostr-native transport carrying canonical operations over encrypted events.

- Encrypted transport (NIP-44 / NIP-59)
- Identity via Nostr keypairs
- Server announcements and discovery
- Supports both `tools/call` dispatch and direct action dispatch
- ACL (allowed npubs) and rate limiting

**Event kinds:**
- Kind 25910 (ephemeral) for unencrypted
- Kind 1059 (NIP-59 gift wrap) for encrypted

**Encryption modes:**
- `disabled` — plaintext kind 25910
- `optional` (default) — defaults to gift-wrap
- `required` — always gift-wrap encrypted

**Config:**

```toml
[contextvm]
relay = "wss://relay.example.com"
encryption = "optional"
allowed_npubs = ["npub1..."]
rate_limit_per_minute = 30
```

## Socket

Local-only transport for efficient shared access by local AI agents.

- Canonical operations via `action + params → ApiResponse` flow
- **Transport-specific:** `subscribe` / `unsubscribe` for push event management
- Push events: `memory.updated`, `agent.connected`, etc.
- Wire protocol: length-prefixed JSON frames

## Serve Mode Combinations

```bash
nomen serve                           # stdio MCP (default)
nomen serve --http :3000              # HTTP only
nomen serve --context-vm              # ContextVM only
nomen serve --socket /tmp/nomen.sock  # Socket only
nomen serve --http :3000 --context-vm # HTTP + ContextVM
nomen serve --context-vm --stdio      # ContextVM + MCP
```

ContextVM requires nsec keys (via config or `--nsec`).
