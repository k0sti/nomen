// Mock data for development when backend is unavailable

import type { SearchResult, Memory, Message, Group, Entity, SearchOpts, MessageOpts, EntityOpts } from './api';

const MOCK_MEMORIES: Memory[] = [
  {
    topic: 'rust/error-handling',
    summary: 'Use anyhow for application errors, thiserror for library errors',
    detail: 'In application code, prefer anyhow::Result for ergonomic error propagation. For library crates, use thiserror to define structured error types that downstream consumers can match on.',
    tier: 'public',
    scope: '',
    confidence: 0.92,
    model: 'anthropic/claude-opus-4-6',
    version: '1',
    source: 'mcp',
    created_at: '2026-02-18T14:30:00Z',
    d_tag: 'snow:memory:rust/error-handling',
  },
  {
    topic: 'project-decisions/storage',
    summary: 'Use NIP-78 for all persistent memory storage',
    detail: 'NIP-78 addressable events provide the right semantics for agent memory: replaceable, tagged, and relay-native. SurrealDB is the local index only; relay is source of truth.',
    tier: 'group',
    scope: 'techteam',
    confidence: 0.88,
    model: 'anthropic/claude-sonnet-4-6',
    version: '3',
    source: 'consolidation',
    created_at: '2026-02-20T09:15:00Z',
    d_tag: 'snow:memory:project-decisions/storage',
  },
  {
    topic: 'nostr/nip-44-encryption',
    summary: 'NIP-44 is the standard for Nostr encrypted payloads',
    detail: 'NIP-44 replaces NIP-04 for encryption. Uses XChaCha20-Poly1305 with conversation keys derived via ECDH. Always use NIP-44 for new encrypted content.',
    tier: 'public',
    scope: '',
    confidence: 0.95,
    model: 'anthropic/claude-opus-4-6',
    version: '1',
    source: 'api',
    created_at: '2026-02-22T11:00:00Z',
    d_tag: 'snow:memory:nostr/nip-44-encryption',
  },
  {
    topic: 'surrealdb/vector-search',
    summary: 'Use HNSW index with cosine similarity for semantic search',
    detail: 'SurrealDB supports HNSW indexes on array<float> fields. Configure with dimension=1536, dist=COSINE, type=HNSW for OpenAI embeddings. Combine with BM25 full-text for hybrid search.',
    tier: 'public',
    scope: '',
    confidence: 0.90,
    model: 'anthropic/claude-opus-4-6',
    version: '2',
    source: 'mcp',
    created_at: '2026-02-25T16:45:00Z',
    d_tag: 'snow:memory:surrealdb/vector-search',
  },
  {
    topic: 'agent/session-management',
    summary: 'Session IDs encode tier, scope, and channel in a single identifier',
    detail: 'Format: npub1... (private DM), group:name (group), "public" (broadcast). When channel not specified, resolve from entity preferences, group config, messaging config, then default to nostr.',
    tier: 'private',
    scope: 'abc123hex',
    confidence: 0.85,
    model: 'anthropic/claude-sonnet-4-6',
    version: '1',
    source: 'consolidation',
    created_at: '2026-03-01T08:20:00Z',
    d_tag: 'snow:memory:agent/session-management',
  },
  {
    topic: 'rust/async-patterns',
    summary: 'Prefer tokio::spawn for background tasks, select! for concurrent futures',
    detail: 'Use tokio::spawn for fire-and-forget background work. Use tokio::select! when you need to race multiple futures. Avoid blocking the runtime with CPU-heavy work — use spawn_blocking instead.',
    tier: 'public',
    scope: '',
    confidence: 0.91,
    model: 'anthropic/claude-opus-4-6',
    version: '1',
    source: 'api',
    created_at: '2026-03-02T13:10:00Z',
    d_tag: 'snow:memory:rust/async-patterns',
  },
];

const MOCK_MESSAGES: Message[] = [
  {
    id: 'msg_001',
    source: 'telegram',
    sender: 'alice',
    channel: 'general',
    content: 'Has anyone tried using SurrealDB with embedded mode? Wondering about performance characteristics.',
    metadata: '',
    consolidated: false,
    created_at: '2026-03-04T10:30:00Z',
  },
  {
    id: 'msg_002',
    source: 'telegram',
    sender: 'bob',
    channel: 'general',
    content: 'Yes, embedded mode works great. kv-surrealkv backend is pure Rust, no external deps needed.',
    metadata: '',
    consolidated: false,
    created_at: '2026-03-04T10:32:00Z',
  },
  {
    id: 'msg_003',
    source: 'nostr',
    sender: 'npub1abc...def',
    channel: '',
    content: 'Published new memory about NIP-78 event format. Updated confidence to 0.95 based on spec review.',
    metadata: '',
    consolidated: true,
    created_at: '2026-03-04T11:00:00Z',
  },
  {
    id: 'msg_004',
    source: 'telegram',
    sender: 'alice',
    channel: 'engineering',
    content: 'We should consolidate the discussion about error handling into a memory entry.',
    metadata: '',
    consolidated: false,
    created_at: '2026-03-04T14:20:00Z',
  },
  {
    id: 'msg_005',
    source: 'webhook',
    sender: 'ci-bot',
    channel: 'builds',
    content: 'Build #142 passed. All 38 tests green. Coverage: 82%.',
    metadata: '{"build_id": 142, "status": "pass"}',
    consolidated: false,
    created_at: '2026-03-04T15:45:00Z',
  },
];

const MOCK_GROUPS: Group[] = [
  {
    id: 'techteam',
    name: 'Tech Team',
    members: [
      'npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqz4s3f5',
      'npub1yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy5kxe2k',
      'npub1zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz6d8ked',
    ],
    nostr_group: 'techteam',
    relay: 'wss://zooid.atlantislabs.space',
    created_at: '2026-01-15T00:00:00Z',
  },
  {
    id: 'techteam.infra',
    name: 'Infrastructure',
    members: [
      'npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqz4s3f5',
      'npub1zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz6d8ked',
    ],
    created_at: '2026-01-20T00:00:00Z',
  },
  {
    id: 'inner-circle',
    name: 'Inner Circle',
    members: [
      'npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqz4s3f5',
    ],
    nostr_group: 'inner-circle',
    created_at: '2026-02-01T00:00:00Z',
  },
];

const MOCK_ENTITIES: Entity[] = [
  { name: 'SurrealDB', kind: 'project', created_at: '2026-02-18T00:00:00Z' },
  { name: 'Nostr', kind: 'concept', created_at: '2026-02-18T00:00:00Z' },
  { name: 'NIP-78', kind: 'concept', created_at: '2026-02-20T00:00:00Z' },
  { name: 'Atlantis Labs', kind: 'organization', created_at: '2026-02-15T00:00:00Z' },
  { name: 'Alice', kind: 'person', created_at: '2026-03-01T00:00:00Z' },
  { name: 'Bob', kind: 'person', created_at: '2026-03-01T00:00:00Z' },
];

export function mockMemories(tier?: string): Memory[] {
  if (tier) return MOCK_MEMORIES.filter((m) => m.tier === tier);
  return MOCK_MEMORIES;
}

export function mockSearch(query: string, _opts?: SearchOpts): SearchResult[] {
  const q = query.toLowerCase();
  return MOCK_MEMORIES.filter(
    (m) => m.topic.toLowerCase().includes(q) || m.summary.toLowerCase().includes(q) || m.detail.toLowerCase().includes(q)
  ).map((m, i) => ({
    topic: m.topic,
    summary: m.summary,
    detail: m.detail,
    tier: m.tier,
    scope: m.scope,
    confidence: m.confidence,
    score: 1.0 - i * 0.1,
    match_type: 'hybrid',
    created_at: m.created_at,
  }));
}

export function mockMessages(opts?: MessageOpts): Message[] {
  let msgs = [...MOCK_MESSAGES];
  if (opts?.source) msgs = msgs.filter((m) => m.source === opts.source);
  if (opts?.channel) msgs = msgs.filter((m) => m.channel === opts.channel);
  if (opts?.sender) msgs = msgs.filter((m) => m.sender === opts.sender);
  return msgs.slice(0, opts?.limit ?? 50);
}

export function mockGroups(): Group[] {
  return MOCK_GROUPS;
}

export function mockEntities(opts?: EntityOpts): Entity[] {
  let ents = [...MOCK_ENTITIES];
  if (opts?.kind) ents = ents.filter((e) => e.kind === opts.kind);
  if (opts?.query) {
    const q = opts.query.toLowerCase();
    ents = ents.filter((e) => e.name.toLowerCase().includes(q));
  }
  return ents;
}
