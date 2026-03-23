# Multi-User Identity & Signing

**Version:** v0.1
**Date:** 2026-03-23
**Status:** Draft

---

## Motivation

Nomen currently reads a single `nsec` from its config file and uses it for all signing and encryption. This limits it to one identity per instance. For multi-user deployments — where multiple agents or users connect to the same Nomen server — each client needs its own identity.

The `NomenSigner` trait already abstracts signing and encryption. This spec defines how clients provide their identity at connection time, enabling per-session signers without changing Nomen's core.

---

## Goals

1. Each connected client operates under its own Nostr identity (keypair)
2. Signing and encryption use the client's key, not a server-wide key
3. Support both direct key handoff (agents) and remote signing (humans)
4. No breaking changes to the `NomenSigner` trait
5. Transport-agnostic — works over MCP stdio, HTTP, and ContextVM

---

## Identity Modes

### 1. Direct nsec

Client provides its secret key at connection time. Nomen instantiates a `KeysSigner` and holds the key in memory for the session lifetime.

**When to use:** Agents and self-hosted single-user setups. The client already holds the nsec in its process memory — passing it to Nomen adds no new trust boundary.

**Handshake:**
```json
{
  "method": "identity.auth",
  "params": {
    "mode": "nsec",
    "nsec": "nsec1..."
  }
}
```

**Server response:**
```json
{
  "pubkey": "<hex-pubkey>",
  "mode": "nsec"
}
```

**Security considerations:**
- Key is transmitted once and held in server memory only for the session
- Should only be used over local transports (stdio, localhost) or encrypted channels
- Server MUST NOT log or persist the nsec

### 2. NIP-46 (Nostr Connect)

Client provides a NIP-46 connection URI. Nomen connects to the remote signer and delegates all signing/encryption operations. The secret key never leaves the signer.

**When to use:** Human users, multi-user deployments, any scenario where key isolation matters.

**Handshake:**
```json
{
  "method": "identity.auth",
  "params": {
    "mode": "nip46",
    "uri": "bunker://<signer-pubkey>?relay=wss://relay.example.com&secret=<token>"
  }
}
```

**Server response:**
```json
{
  "pubkey": "<hex-pubkey>",
  "mode": "nip46"
}
```

**Implementation:**
- Nomen creates a `Nip46Signer` implementing `NomenSigner`
- `sign_event()` → sends `sign_event` request to remote signer
- `encrypt()` / `decrypt()` → sends `nip44_encrypt` / `nip44_decrypt` to remote signer
- `secret_key()` → returns `None` (key is remote)

**Trade-offs:**
- Latency: each sign/encrypt operation is a network round-trip
- Availability: remote signer must be online for the session
- Caching: Nomen MAY cache the pubkey but MUST NOT cache any secret material

---

## Transport Integration

### MCP (stdio)

Identity is established as the first message after initialization:

```
→ {"jsonrpc":"2.0","id":1,"method":"identity.auth","params":{"mode":"nsec","nsec":"nsec1..."}}
← {"jsonrpc":"2.0","id":1,"result":{"pubkey":"abc...","mode":"nsec"}}
→ {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_store",...}}
```

All subsequent operations use the authenticated identity. If `identity.auth` is not called, Nomen falls back to the config-level nsec (backwards compatible).

### HTTP API

Identity can be provided per-request or per-session:

**Per-request (header):**
```
Authorization: Nostr nsec1...
Authorization: Nostr bunker://<signer-pubkey>?relay=...&secret=...
```

**Per-session (cookie/token):**
Client calls `POST /identity/auth` once, receives a session token, and includes it in subsequent requests.

```
POST /identity/auth
Content-Type: application/json

{"mode": "nsec", "nsec": "nsec1..."}

→ 200 OK
{"pubkey": "abc...", "session": "<session-token>"}
```

Subsequent requests:
```
Authorization: Bearer <session-token>
```

### ContextVM (NIP-44 encrypted)

The ContextVM transport already authenticates via Nostr event signatures. The connecting client's pubkey is known from the NIP-44 envelope. Nomen can:

1. **Use the client's pubkey for read-only operations** — filtering memories by visibility/scope
2. **Require explicit identity.auth for write operations** — since Nomen needs signing capability, not just identity

For ContextVM clients that are agents with their own nsec, they send `identity.auth` with mode `nsec` over the encrypted channel. For human clients, they send a NIP-46 URI.

---

## NomenSigner Implementations

### Existing: `KeysSigner`

Wraps `nostr_sdk::Keys`. Used for direct nsec and managed modes. No changes needed.

### New: `Nip46Signer`

```rust
pub struct Nip46Signer {
    client: Nip46Client,
    pubkey: PublicKey,
}

#[async_trait]
impl NomenSigner for Nip46Signer {
    async fn sign_event(&self, unsigned: UnsignedEvent) -> Result<Event> {
        self.client.sign_event(unsigned).await
    }

    fn public_key(&self) -> PublicKey {
        self.pubkey
    }

    fn encrypt(&self, content: &str) -> Result<String> {
        // NIP-46: request nip44_encrypt(own_pubkey, content)
        self.client.nip44_encrypt(&self.pubkey, content)
    }

    fn decrypt(&self, encrypted: &str) -> Result<String> {
        self.client.nip44_decrypt(&self.pubkey, encrypted)
    }

    fn encrypt_to(&self, content: &str, recipient: &PublicKey) -> Result<String> {
        self.client.nip44_encrypt(recipient, content)
    }

    fn decrypt_from(&self, encrypted: &str, sender: &PublicKey) -> Result<String> {
        self.client.nip44_decrypt(sender, encrypted)
    }

    fn secret_key(&self) -> Option<&SecretKey> {
        None
    }
}
```

Note: `encrypt`/`decrypt` methods on the trait are currently synchronous. NIP-46 operations are async (network round-trip). The trait may need to be updated to make encrypt/decrypt async, or the Nip46Signer uses a blocking bridge internally.

---

## Session Model

```
┌─────────┐     identity.auth      ┌─────────────┐
│ Agent A  │ ───────────────────→  │    Nomen     │
│  (nsec)  │  KeysSigner created   │              │
│          │  ←──────────────────  │  Session A   │──→ pubkey_a
└─────────┘                        │  (signer_a)  │
                                    │              │
┌─────────┐     identity.auth      │  Session B   │──→ pubkey_b
│ Agent B  │ ───────────────────→  │  (signer_b)  │
│  (nsec)  │  KeysSigner created   │              │
│          │  ←──────────────────  │  Session C   │──→ pubkey_a (same identity)
└─────────┘                        │  (signer_c)  │
                                    └─────────────┘
┌─────────┐     identity.auth
│  Human   │ ───────────────────→   Session C uses Nip46Signer
│  (nip46) │  remote signer
└─────────┘
```

Each session holds its own `Arc<dyn NomenSigner>`. Multiple sessions may share the same pubkey (same identity, different channels). The `Nomen` struct's current single signer becomes the fallback/default for unauthenticated sessions.

### Memory vs Raw Message Isolation

**Memories** are tied to `event.pubkey` — shared across all sessions for the same identity. This is by design: an agent connecting from two channels should see the same knowledge.

**Raw messages** are tied to their originating channel via the `channel` metadata field (e.g. `telegram:-1003821690204:694`). Multiple sessions with the same pubkey produce raw messages tagged to different channels, which consolidate into shared memories.

---

## Access Control

With per-session identity, access control becomes straightforward:

- **Memory reads:** filter by visibility rules using the session's pubkey
  - `public` → always visible
  - `private` → only if `event.pubkey == session.pubkey`
  - `personal` → only if `event.pubkey == session.pubkey` OR scope matches session pubkey
  - `group` → only if session pubkey is a group member
  - `circle` → only if session pubkey is in the participant set
- **Memory writes:** events are signed with the session's signer — pubkey is implicit
- **Encryption:** handled by the session's signer — correct keys used automatically

---

## Backwards Compatibility

- If no `identity.auth` is sent, Nomen uses the config-level nsec (current behavior)
- Single-user deployments continue to work unchanged
- The `identity.auth` method is optional — not a breaking protocol change

---

## Migration Path

1. Make `encrypt`/`decrypt` async on `NomenSigner` trait
2. Add `Nip46Signer` implementing `NomenSigner`
3. Add `identity.auth` handler to MCP and HTTP transports
4. Refactor `Nomen` to hold per-session signers instead of a single global signer
5. Session lifecycle: create on `identity.auth`, cleanup on disconnect
6. Update access control to use session pubkey for filtering

---

## Design Decisions

1. **Session lifetime** — sessions are tied to the connection. Disconnect = session ends. No persistent server-side session state beyond the connection.
2. **Trait sync→async** — `encrypt`/`decrypt` on `NomenSigner` will be made `async` for consistency with `sign_event` and to support NIP-46 cleanly. All callers already operate in async contexts.
3. **Key rotation** — future feature, out of scope for this spec.
4. **Multi-device** — multiple sessions with the same pubkey is supported and expected. Memories are shared (same `event.pubkey`), raw messages are separated by channel metadata.
