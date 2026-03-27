# Security

## Authentication

### NIP-42 Relay Auth

Required by auth-enforcing relays. Client responds to relay AUTH challenge with signed kind 22242 event containing relay URL and challenge string.

### NIP-98 HTTP Auth

The HTTP transport supports NIP-98 authentication. Clients sign a kind 27235 ephemeral event containing the request URL and method, base64-encode it, and send it as `Authorization: Nostr <base64>`. The server verifies the signature, kind, and timestamp freshness (≤60s).

The server never sees the client's secret key — only the signed proof of identity.

Legacy `Authorization: Nostr nsec1...` is supported for backward compatibility but not recommended.

### Per-Session Identity

See [identity.md](identity.md). Clients authenticate via `identity.auth` with either direct nsec or NIP-46 remote signer. Unauthenticated sessions fall back to config-level identity.

## Encryption

### Memory Encryption

| Tier | Encryption | Method |
|---|---|---|
| `public` | None | — |
| `group` | None | Relay-enforced (NIP-29) |
| `circle` | ChaCha20-Poly1305 | Shared symmetric key |
| `personal` | NIP-44 | Self-encrypt (author → own pubkey) |
| `private` | NIP-44 | Self-encrypt (author → own pubkey) |

Only `content` is encrypted. Tags remain plaintext for relay-side filtering.

### Circle Encryption

Uses shared symmetric key model. Agent is always a participant.

**2-participant:** ECDH-derived (no distribution needed).

**Multi-participant:** Agent generates random key, distributes via NIP-44 DMs.

**Algorithm:** ChaCha20-Poly1305. No forward secrecy by design — persistent memories readable forever by all participants.

**Key storage:** Encrypted at rest in SurrealDB (`circle_key` table), self-encrypted with agent's NIP-44 key.

### ContextVM Encryption

NIP-44 / NIP-59 gift-wrap encryption between client and server. Configurable: `disabled`, `optional`, `required`.

## Access Control

Memory visibility enforced at query time using session pubkey:

- `public` → always visible
- `private` → only event author
- `personal` → event author or scope-matching pubkey
- `group` → group members only
- `circle` → participant set only

Write operations sign with session signer — pubkey is implicit.

## Key Management

- Config-level nsec: single identity per instance (backwards-compatible default)
- Per-session signers: multiple identities via `identity.auth`
- NIP-46 remote signing: secret key never leaves the signer
- Circle keys: stored encrypted at rest, distributed via NIP-44 DMs
- Server MUST NOT log or persist client nsec values
