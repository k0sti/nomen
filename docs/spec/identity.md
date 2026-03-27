# Identity & Access Control

## Multi-User Identity

Nomen supports per-session identity. Each connected client can operate under its own Nostr keypair.

### Identity Modes

**Direct nsec** — Client provides secret key at connection time. Nomen instantiates a `KeysSigner`. Use over local/encrypted transports only.

**NIP-46 (Nostr Connect)** — Client provides a NIP-46 connection URI. Nomen delegates signing/encryption to the remote signer. Secret key never leaves the signer.

### Handshake

```json
{
  "method": "identity.auth",
  "params": { "mode": "nsec", "nsec": "nsec1..." }
}
```

or:

```json
{
  "method": "identity.auth",
  "params": { "mode": "nip46", "uri": "bunker://<signer-pubkey>?relay=...&secret=..." }
}
```

If no `identity.auth` is sent, Nomen falls back to the config-level nsec (backwards compatible).

### Transport Integration

| Transport | Identity mechanism |
|---|---|
| MCP (stdio) | First message after init |
| HTTP | `Authorization: Nostr <nsec>` header or `POST /identity/auth` session |
| ContextVM | Client pubkey from NIP-44 envelope; explicit auth for writes |
| Socket | First message on connection |

### Session Model

Each session holds its own `Arc<dyn NomenSigner>`. Multiple sessions may share the same pubkey. The `Nomen` struct's config-level signer is the fallback for unauthenticated sessions.

- **Memories** are tied to `event.pubkey` — shared across sessions for the same identity
- **Collected messages** are tied to their conversation container — multiple sessions with the same pubkey can produce messages in different chats/threads

## Access Control

With per-session identity:

| Visibility | Read access |
|---|---|
| `public` | Always visible |
| `private` | Only if `event.pubkey == session.pubkey` |
| `personal` | Only if `event.pubkey == session.pubkey` or scope matches session pubkey |
| `group` | Only if session pubkey is a group member |
| `circle` | Only if session pubkey is in the participant set |

Write access: events are signed with the session's signer — pubkey is implicit.

## Groups

### Named Groups

Pre-defined groups with an ID, name, and explicit member list. Configured in `config.toml` or created via API.

```toml
[[groups]]
id = "atlantislabs.engineering"
name = "Engineering"
members = ["npub1abc...", "npub1def..."]
nostr_group = "techteam"
relay = "wss://zooid.atlantislabs.space"
```

Features:
- Hierarchical IDs with dot separator
- NIP-29 group mapping via `nostr_group`
- Stored in `nomen_group` table

### Ad-hoc npub Sets

Implicit groups formed by a set of participants. Identity is the sorted set of npubs, hashed deterministically.

## Encryption

| Tier | Encryption | Method |
|---|---|---|
| `public` | None | — |
| `group` | None | Relay-enforced access (NIP-29) |
| `circle` | Required | Shared symmetric key (ChaCha20-Poly1305) |
| `personal` | Required | NIP-44 self-encrypt |
| `private` | Required | NIP-44 self-encrypt |

Only `content` is encrypted. Tags remain plaintext for relay-side filtering.

### Circle Key Derivation

**2-participant (agent + 1 user):**

```
circle_key = HKDF-SHA256(
  ikm  = ECDH(my_nsec, their_pubkey),
  salt = "nomen-circle",
  info = circle_id
)
```

**Multi-participant (agent + N users, N ≥ 2):**

Agent generates random 256-bit key, distributes via NIP-44 DMs to each participant.

### Relay Auth

NIP-42 AUTH is required by some relays (e.g. zooid). Client responds to AUTH challenge with signed kind 22242 event.
