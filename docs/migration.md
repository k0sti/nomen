# Migration Guide: D-Tag Format v0.1 → v0.2

**Date:** 2026-03-07

## Overview

The nostr-memory-spec v0.2 introduces a new d-tag format that encodes visibility, scope, and topic directly in the d-tag. This replaces the v0.1 format that used `snow:memory:` and `snowclaw:memory:` prefixes with separate `tier` and `source` tags.

## Format Changes

### D-Tag Format

| v0.1 | v0.2 |
|------|------|
| `snow:memory:{topic}` | `public::{topic}` |
| `snowclaw:memory:npub:{npub}` | `personal:{hex-pubkey}:{topic}` |
| `snowclaw:memory:group:{id}` | `group:{id}:{topic}` |
| (no equivalent) | `circle:{hash}:{topic}` |
| (no equivalent) | `internal:{hex-pubkey}:{topic}` |

### Tag Changes

| v0.1 | v0.2 |
|------|------|
| `tier` tag | Replaced by indexed `visibility` + `scope` tags |
| `snow:tier` tag | Removed — legacy compat |
| `source` tag | Removed — use `event.pubkey` |
| `snow:confidence` | `confidence` (no prefix) |
| `snow:version` | `version` (no prefix) |
| `snow:model` | `model` (no prefix) |

### Tier Rename

| v0.1 | v0.2 |
|------|------|
| `private` | `personal` (user-auditable) or `internal` (agent-only) |

## Dual-Format Read Support

The codebase now supports **both** formats on read:

- `memory::parse_d_tag()` recognizes v0.1 prefixes (`snow:memory:`, `snowclaw:memory:`) and v0.2 prefixes (`public:`, `group:`, `personal:`, `internal:`, `circle:`)
- `memory::parse_tier()` checks for `tier`/`snow:tier` tags (v0.1) and falls back to extracting visibility from the d-tag prefix (v0.2)
- `memory::is_v2_dtag()` returns true if a d-tag uses the v0.2 format
- Legacy `private` tier is normalized to `personal` on read

## New-Format Write

All new memory events are published with the v0.2 d-tag format:

- CLI `nomen store` builds d-tags as `{visibility}:{scope}:{topic}`
- No `tier` or `source` tags are emitted
- Tag names use clean names (no `snow:` prefix)
- Provider-specific channel/container IDs are not part of the d-tag; they belong in raw-message provenance/metadata

## Migration Steps

For existing deployments with v0.1 events on relay:

1. **No immediate action required** — dual-format read support means old events are still readable
2. **New writes** use v0.2 format automatically
3. **Optional batch migration**: fetch all old events, re-publish with new d-tag format, delete old events via NIP-09
4. Since kind 31234 is replaceable by `(author, kind, d-tag)`, old and new events coexist until old ones are explicitly deleted

## Helper Functions

```rust
// Check if a d-tag is v0.2 format
memory::is_v2_dtag("public::rust-errors") // true
memory::is_v2_dtag("snow:memory:rust-errors") // false

// Extract topic from v0.2 d-tag
memory::v2_dtag_topic("group:techteam:deploy") // Some("deploy")

// Build a v0.2 d-tag
memory::build_v2_dtag("personal", "d29fe7c1...", "ssh-config")
// → "personal:d29fe7c1...:ssh-config"
```
