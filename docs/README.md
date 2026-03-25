# Nomen Docs

Canonical repo documentation index.

Repo docs should describe the **current implementation and canonical interfaces**. Planning notes, drafts, and task trackers belong in Obsidian/project notes, not here.

## Core docs

- `architecture.md` — system architecture and boundaries
- `api-reference.md` — user-facing API reference
- `api-v2-spec.md` — lower-level API/domain specification (partly legacy; being converged into canonical reference docs)
- `collected-messages.md` — canonical normalized messaging hierarchy and kind `30100` collected-message model
- `consolidation-spec.md` — current consolidation pipeline behavior and constraints
- `nostr-memory-spec.md` — durable memory event model
- `multi-user-identity-spec.md` — identity/session behavior across clients and channels

## Specialized docs

- `dtag-v3-spec.md` — d-tag format details for memory identifiers
- `circle-encryption-impl.md` — circle encryption implementation notes

## Docs status notes

Current canonical message hierarchy:

**platform → community → chat → thread → message**

Current canonical collected-message identity rule:

```text
<platform>:<chat_id>:<message_id>
```

unless a platform truly requires more coordinates for uniqueness.

## Docs hygiene rules

- Keep repo docs aligned with shipped behavior.
- Move draft/planning/task content to Obsidian instead of leaving it in `docs/`.
- If an older spec is still useful historically but not canonical, mark it clearly as legacy/superseded.
- Avoid parallel conflicting specs.
