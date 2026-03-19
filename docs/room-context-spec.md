# Room Context Injection Specification

**Version:** v0.3
**Date:** 2026-03-19
**Status:** Draft

Defines how integrations (for example OpenClaw) derive, fetch, and inject room context from Nomen for groups, forum topics, threads, circles, and DMs.

Room context uses standard memory events. No new event kind is required.

---

## 1. Concept

A **room** is a durable conversation container: a group chat, a DM, a forum topic, a thread, or a circle.

The goal is simple: when an agent starts a session, it should immediately know:
- where it is
- what the room is for
- what the current topic/thread is about
- optionally, who the other participant is

For forum-style chats, room context is **two-layered**:
1. **group layer** — shared across all topics in the group
2. **topic layer** — specific to the current topic/thread

Both layers should be injected when available.

---

## 2. D-tag Convention

Follows the standard Nomen d-tag format:

`{visibility}:{scope}:{topic}`

### Room events

| Chat type | d-tag | Example |
|-----------|-------|---------|
| Group | `group:<provider_group_id>:room` | `group:telegram:-1003821690204:room` |
| Topic/thread | `group:<provider_group_id>:room/<topic_id>` | `group:telegram:-1003821690204:room/8485` |
| Circle | `circle:<circle_hash>:room` | `circle:a3f8b2c1e9d04712:room` |
| DM (npub known) | `personal:<user_hex_pubkey>:room` | `personal:1634b87b...:room` |
| DM (no npub) | `personal:<provider_user_id>:room` | `personal:telegram:60996061:room` |

### Notes

- **Group rooms use `group` visibility**
- **Circle rooms use `circle` visibility**
- **DM rooms use `personal` visibility**
- **Topic/thread rooms are children of the group room**
- **Scope is the provider group/chat identifier** (e.g. `telegram:-1003821690204`)
- Provider-specific identifiers are used as scope — this makes d-tags deterministically derivable from inbound metadata

### Examples for this Telegram forum chat

```text
# Group layer
group:telegram:-1003821690204:room

# Topic layer (topic 8485 / "Nomen")
group:telegram:-1003821690204:room/8485
```

---

## 3. Two-layer injection model

For forum-style conversations, the integration should inject:

1. **group info** — room context shared across all topics in the group
2. **topic info** — room context specific to the current topic/thread

For this chat, that means:
- TechTeam group info
- Nomen topic info

This is the expected behavior for Telegram forum topics.

If only one layer exists, inject the one that exists.

---

## 4. Scope-based behavior

| Scope | Determined by | Room context | Participant context |
|-------|--------------|-------------|-------------------|
| Group | Group chat / forum topic | Group + topic layers | No |
| Circle | Nostr circle / ad-hoc room | Room layer | Yes |
| Personal | DM | Room layer | Yes |

Scope is determined directly from inbound channel metadata.

---

## 5. Fetch flow

### Session start (read path)

When a new session starts, the integration derives room d-tags directly from inbound metadata.

#### Telegram forum topics

Given:
- `chatId = telegram:-1003821690204`
- `threadId = 8485`

derive:
- group room d-tag: `group:telegram:-1003821690204:room`
- topic room d-tag: `group:telegram:-1003821690204:room/8485`

The integration fetches both using direct d-tag lookup.

### Lookup method

**Direct d-tag fetch** — the only supported lookup method.

Given inbound metadata, the integration deterministically constructs d-tags and fetches them:
- `memory.get(d_tag: "group:{chatId}:room")` for group layer
- `memory.get(d_tag: "group:{chatId}:room/{threadId}")` for topic layer

This approach is simple, predictable, and requires no additional state (no binding tables, no indirection).

### Lookup order

For topic/thread conversations:
1. fetch **group room** by d-tag
2. fetch **topic room** by d-tag
3. inject both when present

For DMs:
1. fetch **room** by d-tag
2. optionally fetch **participant profile**

---

## 6. Injection format

Injected into session context as structured text.

Example:

```text
# Room Context (TechTeam)

TechTeam — engineering coordination across Telegram forum and Nostr group

...group-level notes...

## Topic Context (Nomen)

Nomen memory system development

...topic-specific notes...
```

The exact rendering can vary, but the semantic layers should remain distinct.

---

## 7. Write flow

### Auto-generation on miss

If a room/topic context is missing, the integration may auto-generate it from:
- group subject/name
- topic/thread name
- channel type
- first user message(s)

This is especially useful for topic rooms, since many forum topics start with only a title.

### Explicit update

Users can update room or topic context explicitly. This should write a normal memory event with the same d-tag, allowing standard supersedes/versioning behavior.

---

## 8. API operations used

| Operation | Purpose |
|-----------|---------|
| `memory.get` | Fetch room context by exact d-tag |
| `memory.get_batch` | Fetch group + topic + participant context in one call |
| `memory.put` | Create/update room or topic context |
| `memory.search` | Fallback lookup (not currently used for room injection) |

No new Nomen API operations are required for room context injection.

---

## 9. OpenClaw integration notes

For OpenClaw's `before_prompt_build` hook:
- use `ctx.inboundContext.chatId` for the chat-level provider id
- use `ctx.inboundContext.threadId` for the topic/thread id when present
- derive d-tags deterministically: `group:{chatId}:room` and `group:{chatId}:room/{threadId}`
- for Telegram forum topics, inject **both group and topic layers**
- inject room context in system-context space (`appendSystemContext`)

This behavior is compatible with the OpenClaw plugin hook spec.

---

## Appendix A: Provider Bindings (not currently implemented)

> **Status:** Designed but disabled. The direct d-tag lookup approach (Section 5) is simpler and covers all current use cases. Provider bindings may be revisited if cross-channel room identity mapping becomes necessary.

Provider bindings are an indirection layer that maps provider-specific chat identifiers to memory d-tags via a `provider_binding` table.

### Concept

A binding maps a provider ID to a memory d-tag:

```
provider_binding {
  provider_id: "telegram:-1003821690204"
  d_tag: "group:techteam:room"
}
```

This would allow the same room memory to be resolved from multiple channels (e.g. both Telegram and Nostr group pointing to the same room context).

### API operations (disabled)

| Operation | Purpose |
|-----------|---------|
| `room.resolve` | Resolve room memories by bound provider id |
| `room.bind` | Bind provider id to room memory |
| `room.unbind` | Remove a binding |

### When this might be needed

- Cross-channel room identity: same group accessible from Telegram, Nostr, Discord
- Migration scenarios: changing provider IDs while preserving room context
- Named aliases: human-friendly room names mapped to provider IDs

### Why it's disabled

- Adds indirection that makes debugging harder
- Unclear when/how bindings should be created (manual only? auto-discovered?)
- Direct d-tag derivation covers all current integration patterns
- The binding table requires its own lifecycle management (creation, cleanup, conflict resolution)
