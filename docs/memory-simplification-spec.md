# Memory Simplification Spec

> Status: historical/superseded design note. Keep only as background reference until any still-relevant decisions are merged into canonical docs.

**Date:** 2026-03-22
**Status:** Implementing

## Goal

Simplify the memory model: remove `summary`, `confidence`, and `version` from the content/wire format. Memory content becomes plain text (markdown), not JSON.

## Changes

### Nostr Event Format (kind 31234)

**Before:**
```json
{
  "kind": 31234,
  "content": "{\"summary\":\"Short desc\",\"detail\":\"Full content...\",\"confidence\":0.85,\"version\":2}",
  "tags": [["d", "personal:<hex>:topic"], ...]
}
```

**After:**
```json
{
  "kind": 31234,
  "content": "Full content as plain text/markdown...",
  "tags": [
    ["d", "personal:<hex>:topic"],
    ["visibility", "personal"],
    ["scope", "<hex>"],
    ["pinned", "true"],
    ["importance", "8"],
    ["supersedes", "<old-event-id>"]
  ]
}
```

Content is **plain text or markdown**. Not JSON. First line can serve as display title when needed.

### Fields Removed

| Field | Where removed | Notes |
|---|---|---|
| `summary` | Content JSON, DB schema, API params, MCP tools | Redundant with content |
| `confidence` | Content JSON, DB schema | Unreliable LLM-assigned score |
| `version` | Content JSON | Nostr `created_at` provides ordering; DB can auto-increment internally |

### Fields Kept

| Field | Location | Notes |
|---|---|---|
| `content` | Nostr event content (plain text) | Was `detail`, now the whole content |
| `visibility` | Nostr tag | personal/group/public/internal |
| `scope` | Nostr tag | pubkey hex, group id, etc. |
| `pinned` | Nostr tag | optional |
| `importance` | Nostr tag | optional, 1-10 |
| `supersedes` | Nostr tag | optional, on update |
| `version` | DB-internal only | Auto-incremented, not in wire format |
| `access_count` | DB-internal only | Usage tracking |

### DB Schema Changes

In `MemoryRecord`:
- `summary` → removed (or `Option<String>` kept for migration reads, never written)
- `confidence` → removed
- `content` field replaces the old `detail` field conceptually
- `search_text` built from full content (was `summary + detail`)

Field name in DB stays `detail` to avoid a full table rewrite — it just stores the full content now.

### Migration / Backward Compatibility

**On read (from Nostr relay or DB):**
- If content parses as JSON with `summary` and/or `detail` fields → merge: `"{summary}\n\n{detail}"` (strip if summary is empty)
- If content is plain text → use as-is
- Ignore `confidence` and `version` from JSON content if present

**On write:**
- Always write plain text content
- Never write JSON content
- No bulk migration needed

### API Changes

**`memory.put`:**
- Remove `summary` from params (was required, now gone)
- Remove `confidence` from params
- `detail` param renamed to `content` (accept both for compat)
- `content` is the full memory text

**`memory.search` / `memory.list` / `memory.get` response:**
- Remove `summary` field from results
- Remove `confidence` field from results
- `detail` field contains the full content
- Add `content` as alias for `detail` in response

**MCP tools:**
- `memory_put`: remove `summary` from required, remove `confidence`, use `content` param
- `memory_search`: results have `content` instead of `summary`+`detail`
- Consolidation prompt: produce `{"topic", "content", "importance"}` instead of `{"topic", "summary", "importance"}`

### Web UI Changes

- `MemoryCard`: derive title from first line of content instead of summary
- Create form: single content textarea instead of summary + detail
- Search results: show truncated content
- `api.ts` types: remove summary from interfaces

### CLI Changes

- `nomen store` command: remove `--summary` flag, use `--content` or positional
- Display: show first line as title, rest as body
- Search output: truncated content instead of summary

### OpenClaw Nomen Plugin

- `memory_put` tool: `summary` param → prepend to `detail` for backward compat, or just use as content
- `memory_search` response: derive summary-like snippet from content (first line or truncation)

### Embedding

- Build from full content (was `"{summary} {detail}"`, now just content) — actually better

### Consolidation

- Update LLM prompt to produce `content` instead of `summary` + `detail`
- Remove `confidence` from extraction schema
