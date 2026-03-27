# Circle Encryption — Design

Circles are ad-hoc participant sets for encrypted shared memories.

## Architecture

```
Nomen Agent (always participant)
  ├── 2-person: ECDH-derived key (no distribution)
  ├── N-person: agent-generated key, NIP-44 distributed
  └── All encrypted with ChaCha20-Poly1305
```

## Key Derivation

### 2-Participant (agent + 1 user)

```
circle_key = HKDF-SHA256(
  ikm  = ECDH(my_nsec, their_pubkey),
  salt = "nomen-circle",
  info = circle_id
)
```

Both sides compute independently — no key distribution needed.

### Multi-Participant (agent + N users)

Agent generates random 256-bit key, distributes to each participant via NIP-44 DM:

```json
{
  "type": "circle_key",
  "circle_id": "a3f8b2c1e9d04712",
  "key": "<hex>",
  "participants": ["<pubkey1>", "<pubkey2>"]
}
```

## Circle ID

1. Collect all participant hex pubkeys (including agent)
2. Sort alphabetically
3. SHA-256 hash of concatenated sorted pubkeys
4. First 16 hex characters

## Encryption Flow

1. Detect `circle` visibility on publish
2. Look up or derive circle key from circle_id
3. Encrypt `content` with ChaCha20-Poly1305 (random nonce)
4. Output: `base64(nonce || ciphertext || tag)`
5. Tags remain plaintext for relay-side filtering

## Status

Designed but not yet implemented. See `obsidian/03-11 circle-encryption-impl.md` for implementation tasks.
