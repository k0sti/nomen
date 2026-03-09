# Nomen ↔ Zeroclaw Memory Integration Specification

**Date:** 2026-03-09  
**Status:** Draft  
**Audience:** Nomen implementers, Snowclaw/Zeroclaw/OpenClaw integrators

---

## 1. Purpose

This document defines how Nomen integrates with the existing Zeroclaw/OpenClaw memory interface.

Zeroclaw's memory system is already in production use and existing agents depend on it. Nomen is a newer, Nostr-native memory architecture with different primitives. The goal of this spec is:

- preserve compatibility with existing agents
- avoid forcing Nomen to inherit legacy abstractions as core concepts
- define a clean adapter contract so integrations do not invent incompatible mappings

This is a **compatibility specification**, not a redefinition of Nomen's core memory model.

---

## 2. Core position

### Nomen is canonical

Nomen's canonical memory model is:

- raw messages are Nostr events (source of truth)
- durable named memories are kind 31234 replaceable events
- memory identity is based on `visibility + scope + topic`
- provider/container details belong in `channel`, not in memory scope

### Zeroclaw is a compatibility interface

Zeroclaw's memory interface is treated as a host-facing compatibility layer:

- `key`
- `category`
- `session_id`
- `store / recall / get / list / forget`

These fields and calls must be mapped into Nomen explicitly. They are **not** equivalent to Nomen's native ontology.

---

## 3. Zeroclaw memory model summary

A Zeroclaw memory entry contains:

- `key`
- `content`
- `category`
- `session_id` (optional)
- `timestamp`
- `score` (optional)

### Category enum

Zeroclaw categories are:

- `core`
- `daily`
- `conversation`
- `custom(<string>)`

These are **organizational buckets**, not access-control or visibility primitives.

### Session ID

`session_id` is an optional opaque string used by Zeroclaw backends as a partition/filter label.
It is not a canonical scope model and should not be assumed to equal Nomen scope.

### Key

`key` is a host/application identifier. It may be semantic, generated, or legacy. It is not automatically a valid durable Nomen topic.

---

## 4. Mapping rules

## 4.1 Category mapping

### Rule

Zeroclaw `category` MUST NOT be mapped directly to Nomen visibility, scope, or tier.

### Rationale

Zeroclaw categories answer:
> what kind of memory bucket is this in the host application?

Nomen visibility/scope answer:
> who can access this memory, and in what durable boundary does it live?

These are different dimensions.

### Required behavior

A compatibility adapter MUST treat category as one or more of:

- metadata
- topic-generation hint
- storage-policy hint
- retrieval/ranking hint

A compatibility adapter MUST NOT treat arbitrary category strings as canonical Nomen visibility values.

### Canonical mappings

Recommended interpretation:

| Zeroclaw category | Default Nomen meaning |
|---|---|
| `core` | durable named memory; scope determined from actual context |
| `daily` | dated/log-style named memory or raw-message-derived note; scope determined from actual context |
| `conversation` | conversation-derived material; usually better represented as raw messages + consolidation |
| `custom(x)` | metadata/tag/topic hint only unless explicitly mapped by host |

### Unknown/custom categories

Unknown/custom categories MUST NOT be used directly as Nomen visibility or scope values.
They MAY be preserved as metadata, for example:

- `host_system = zeroclaw`
- `host_category = project_notes`

---

## 4.2 Session ID mapping

### Rule

Zeroclaw `session_id` is a compatibility hint, not a canonical Nomen primitive.

### Allowed uses

An adapter MAY use `session_id` for:

- retrieval narrowing
- provenance metadata
- raw-message grouping hint
- fallback topic generation for compatibility records

### Forbidden use

An adapter MUST NOT assume:

- `session_id == scope`
- `session_id == channel`
- `session_id` belongs in durable memory identity by default

### Recommended preservation

Store legacy session identifiers as metadata when helpful:

- `host_session_id = <value>`

### Compatibility retrieval

If a host requests recall/list with `session_id`, the adapter MAY narrow results by:

1. exact metadata match on `host_session_id`
2. current channel context
3. scope-local memories first, then broader scope-visible memories

This is intentionally a compatibility behavior, not a core Nomen requirement.

---

## 4.3 Key mapping

### Rule

Zeroclaw `key` is a host identifier and MUST be treated as one of:

- semantic topic candidate
- compatibility reference
- provenance metadata

It MUST NOT automatically be assumed to be the final durable Nomen topic.

### Recommended behavior

#### If key is semantically usable
Use it as a topic or topic seed.

Examples:
- `favorite_language` → topic candidate
- `user-profile` → topic candidate
- `deploy-decision` → topic candidate

#### If key is opaque or legacy
Preserve it as metadata and generate a better topic.

Examples:
- `assistant_resp_173`
- `openclaw_core_001`
- `legacy_key`

Recommended metadata:
- `host_key = <original key>`

### Fallback rule

If no semantic topic is available, the adapter MAY derive a compatibility topic from category/session/key, but this should be treated as provisional.

Examples:
- `compat/core/favorite_language`
- `compat/conversation/sess-42`
- `compat/daily/2026-03-09`

These are acceptable compatibility topics, but not ideal long-term memory identities.

---

## 5. Raw messages vs named memory

This is the most important structural difference.

### Zeroclaw view

Zeroclaw's legacy interface tends to treat memory as a generic entry store.

### Nomen view

Nomen separates:

- raw messages/events
- durable named memories

### Compatibility rule

Adapters should choose between two write paths:

#### Path A — direct named memory
Use when the incoming write is already a durable fact, preference, lesson, or structured note.

Good examples:
- user preference
- stable project fact
- explicit lesson
- curated long-term memory item

#### Path B — raw message / conversation ingestion
Use when the incoming write is conversation-derived, transient, or session-local.

Good examples:
- dialogue context
- thread discussion
- temporary conversation state
- transcript fragments

In Path B, the adapter should prefer raw-message ingestion plus later consolidation over directly creating durable named memories.

### Recommendation by category

| Category | Preferred path |
|---|---|
| `core` | direct named memory |
| `daily` | depends: named log note or raw-message-derived summary |
| `conversation` | raw message / provenance first |
| `custom(x)` | host-defined; choose explicitly |

---

## 6. Scope and channel mapping

Per the core Nomen spec:

- **scope** is Nostr-native and memory-aligned
- **channel** contains concrete provider/container details

### Compatibility rule

When adapting Zeroclaw/OpenClaw agents:

- scope MUST be derived from actual context (group, personal, internal, public, circle)
- provider-specific transport/container ids MUST go into channel metadata, not scope
- `session_id` MUST NOT be used as a substitute for scope

### Examples

#### Telegram forum topic
- scope: `group:techteam`
- channel: `telegram:-1003821690204:694`

#### Nostr DM
- scope: `personal:<pubkey>`
- channel: `nostr-dm:<peer-pubkey-hex>`

#### Internal agent reflection
- scope: `internal:<agent-pubkey>`
- channel: `local:internal`

---

## 7. CRUD compatibility semantics

## 7.1 store(key, content, category, session_id)

The adapter should:

1. determine whether this is direct named memory or raw-message-style material
2. derive Nomen scope from actual context
3. preserve host metadata (`key`, `category`, `session_id`) when useful
4. choose/generate a semantic topic when possible
5. avoid embedding category/session/provider details into durable memory d-tags unless explicitly part of topic semantics

## 7.2 recall(query, limit, session_id)

The adapter should:

1. search relevant named memories in the current scope
2. optionally incorporate raw-message context from the active channel
3. if `session_id` is provided, apply it only as a compatibility narrowing hint
4. not reinterpret `session_id` as scope unless the host explicitly defines that behavior

## 7.3 get(key)

`get(key)` is a compatibility convenience. Implementations may satisfy it by:

- direct host-key lookup metadata index
- raw topic lookup if key was promoted to topic
- compatibility alias table if needed

The spec does not require all Nomen topics to be directly retrievable by original host key.

## 7.4 list(category, session_id)

This is compatibility-oriented listing. Implementations may filter by:

- preserved host category metadata
- preserved host session metadata
- scope-local defaults first

Category and session filters should be understood as host compatibility filters, not core Nomen semantics.

## 7.5 forget(key)

`forget(key)` in compatibility mode should remove or tombstone the corresponding durable memory identified by host-key mapping or topic alias. Implementations SHOULD preserve auditability where possible.

---

## 8. Metadata convention

To make compatibility auditable and portable, adapters SHOULD preserve these fields when relevant:

- `host_system` — e.g. `zeroclaw`, `openclaw`, `snowclaw`
- `host_key` — original key
- `host_category` — original category string
- `host_session_id` — original session id

These may live in tags, structured metadata, or DB-side compatibility indexes depending on implementation.

---

## 9. Compliance profiles

## Profile Z1 — Basic Zeroclaw compatibility

Supports:
- store/get/list/recall/forget
- category preserved as metadata or simple filter
- session_id preserved or filtered as opaque string
- direct named-memory storage

Does not require:
- raw-message ingestion
- consolidation
- full provenance fidelity

## Profile Z2 — Conversation-aware compatibility

Supports:
- all Z1 capabilities
- conversation/category writes can route to raw-message ingestion
- scope/channel separation honored
- session_id treated as compatibility hint only
- provenance preserved back to channel/events where possible

## Profile Z3 — Nomen-native host adaptation

Supports:
- all Z2 capabilities
- host progressively moves off category/session-based design
- semantic topics preferred over host keys
- raw messages and named memories used according to Nomen's native model

This is the recommended long-term target for Snowclaw.

---

## 10. Design guidance for Snowclaw

Snowclaw should not treat the legacy Zeroclaw memory abstraction as the canonical memory model.

Recommended direction:

1. keep Zeroclaw compatibility via adapter layer
2. make Nomen the canonical storage/retrieval/consolidation model
3. treat `category`, `key`, and `session_id` as host compatibility inputs
4. route new Snowclaw memory features through Nomen-native concepts (`scope`, `channel`, raw messages, named memories)

This reduces semantic mismatch and long-term adapter complexity.

---

## 11. Summary

### Zeroclaw primitives mean:
- `category` = organizational bucket
- `key` = host identifier/reference
- `session_id` = optional opaque partition/filter hint

### Nomen primitives mean:
- `visibility` = access class
- `scope` = durable Nostr-native boundary
- `channel` = concrete message container
- `topic` = durable semantic subject

### Therefore:
- category is **not** visibility
- session_id is **not** scope
- key is **not always** topic
- compatibility must be explicit, not assumed

That is the whole point of this spec.
