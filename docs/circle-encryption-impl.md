# Circle Encryption — Implementation Plan

**Date:** 2026-03-11
**Status:** Ready for implementation
**Spec:** nostr-memory-spec.md §10 Encryption

## Overview

Implement encryption for `circle` and `group` visibility memory events. All events on the shared relay are encrypted. The agent (Nomen) is always a participant in every circle.

## Architecture

```
Nomen Agent (always participant)
  ├── 2-person circle: ECDH-derived key (no distribution)
  ├── N-person circle: agent-generated key, NIP-44 distributed
  ├── group: agent-generated key, NIP-44 distributed to members
  └── All stored on shared auth relay, encrypted with ChaCha20-Poly1305
```

## Tasks

### Phase 1: Circle Key Management

#### T1.1 — Circle key derivation for 2-participant circles
**File:** `src/circle.rs` (new)

- [ ] Create `circle.rs` module
- [ ] Implement `compute_circle_id(pubkeys: &[PublicKey]) -> String`
  - Sort pubkeys lexicographically (hex representation)
  - SHA-256 hash of concatenated sorted hex pubkeys
  - Return first 16 hex characters
- [ ] Implement `derive_circle_key_ecdh(signer: &dyn NomenSigner, peer: &PublicKey, circle_id: &str) -> [u8; 32]`
  - Compute NIP-44 conversation key via ECDH(my_nsec, peer_pubkey)
  - HKDF-SHA256 with salt=`"nomen-circle"`, info=`circle_id`
  - Return 32-byte symmetric key
- [ ] Unit tests: deterministic circle_id from known pubkeys, key derivation symmetry

**Verification:** `cargo test circle` — tests pass, circle_id is deterministic, ECDH keys match from both sides.

#### T1.2 — Circle key generation for multi-participant circles
**File:** `src/circle.rs`

- [ ] Implement `generate_circle_key() -> [u8; 32]`
  - Generate 32 random bytes via `rand::rngs::OsRng`
- [ ] Implement `CircleKeyStore` trait
  - `store_key(circle_id: &str, key: &[u8; 32], participants: &[PublicKey]) -> Result<()>`
  - `get_key(circle_id: &str) -> Result<Option<[u8; 32]>>`
  - `list_circles() -> Result<Vec<CircleInfo>>`
- [ ] SurrealDB implementation of `CircleKeyStore`
  - Table: `circle_key` with fields: `id` (circle_id), `key` (encrypted), `participants`, `created_at`
  - Keys stored encrypted at rest (self-encrypt with agent NIP-44)

**Verification:** `cargo test circle_key_store` — store/retrieve/list work, keys are encrypted in DB.

#### T1.3 — Circle key distribution via NIP-44 DM
**File:** `src/circle.rs`

- [ ] Implement `distribute_circle_key(relay: &NomenRelay, signer: &dyn NomenSigner, circle_id: &str, key: &[u8; 32], participants: &[PublicKey]) -> Result<()>`
  - For each participant: NIP-44 encrypt key + circle metadata
  - Publish as NIP-17 gift-wrapped DM to each participant
  - Message payload: `{"type":"circle_key","circle_id":"...","key":"...hex...","participants":["..."]}`
- [ ] Implement `request_circle_key(relay: &NomenRelay, signer: &dyn NomenSigner, circle_id: &str) -> Result<[u8; 32]>`
  - Check local store first
  - If not found, query relay for incoming circle_key DMs

**Verification:** Integration test — agent distributes key, simulated participant receives and decrypts.

### Phase 2: Encrypted Event Publishing/Reading

#### T2.1 — Symmetric content encryption
**File:** `src/circle.rs`

- [ ] Implement `encrypt_content(key: &[u8; 32], plaintext: &str) -> Result<String>`
  - ChaCha20-Poly1305 encrypt with random nonce
  - Output: base64(nonce || ciphertext || tag)
- [ ] Implement `decrypt_content(key: &[u8; 32], encrypted: &str) -> Result<String>`
  - Decode base64, extract nonce, decrypt
- [ ] Unit tests: round-trip encrypt/decrypt

**Verification:** `cargo test symmetric_encrypt` — round-trip works, wrong key fails.

#### T2.2 — Integrate circle encryption into memory publish
**File:** `src/memory.rs`, `src/relay.rs`

- [ ] In memory event publishing: detect `circle` visibility
- [ ] Look up or derive circle key based on circle_id from d-tag scope
- [ ] Encrypt `content` field with circle key before publishing
- [ ] Keep tags unencrypted (visibility, scope, h, t tags stay plaintext)

**Verification:** Publish a circle memory event, verify content is encrypted on relay, verify tags are plaintext.

#### T2.3 — Integrate circle decryption into memory reading
**File:** `src/memory.rs`

- [ ] Update `try_decrypt_content` to handle circle-encrypted events
  - Detect circle visibility from d-tag or visibility tag
  - Look up circle key from store
  - Attempt symmetric decryption before NIP-44 pairwise decryption
- [ ] Update `parse_event` to use circle decryption path

**Verification:** Fetch circle events from relay, verify decryption works, verify non-members can't decrypt.

### Phase 3: Group Encryption

#### T3.1 — Group key management
**File:** `src/circle.rs` or `src/groups.rs`

- [ ] Reuse `CircleKeyStore` for group keys (same storage, different scope)
- [ ] On group creation/config load: generate group key if not exists
- [ ] Distribute group key to all members via NIP-44 DMs
- [ ] Store group key in SurrealDB (encrypted at rest)

**Verification:** Group key generated, distributed to members, stored encrypted.

#### T3.2 — Group event encryption
**File:** `src/memory.rs`

- [ ] Detect `group` visibility in publish path
- [ ] Look up group key, encrypt content
- [ ] Detect `group` visibility in read path, decrypt with group key

**Verification:** Group memory events encrypted/decrypted correctly.

### Phase 4: CLI & Config

#### T4.1 — Circle CLI commands
**File:** `src/main.rs`

- [ ] `nomen circle create <pubkey1> [pubkey2...]` — create circle, derive/generate key, distribute
- [ ] `nomen circle list` — list circles with participants
- [ ] `nomen circle key <circle_id>` — show circle key (for debugging)

**Verification:** CLI commands work end-to-end.

#### T4.2 — Config for circle/group encryption
**File:** `src/config.rs`

- [ ] Add `[encryption]` config section (optional)
  - `enabled: bool` (default: true)
  - `key_distribution: "nip17"` (default, only option for now)

**Verification:** Config loads, encryption can be disabled for testing.

## Dependencies

- `chacha20poly1305` crate for symmetric encryption
- `hkdf` + `sha2` crates for key derivation
- `nostr-sdk` already has NIP-44 support (already in Cargo.toml)
- `rand` for key generation (likely already a dependency)

## File Summary

| File | Action | Description |
|------|--------|-------------|
| `src/circle.rs` | **New** | Circle ID, key derivation, key store, symmetric crypto, key distribution |
| `src/memory.rs` | Modify | Add circle/group decryption path in `try_decrypt_content` and `parse_event` |
| `src/relay.rs` | Modify | Encrypt content before publish for circle/group visibility |
| `src/groups.rs` | Modify | Add group key management |
| `src/config.rs` | Modify | Add encryption config section |
| `src/main.rs` | Modify | Add circle CLI commands |
| `src/kinds.rs` | No change | Existing kinds sufficient |
| `src/lib.rs` | Modify | Add `pub mod circle;` |
| `Cargo.toml` | Modify | Add chacha20poly1305, hkdf, sha2 dependencies |
