// Nomen API Client — REST calls for server-side operations

import { normalizeVisibility } from './dtag';

export interface SearchResult {
  topic: string;
  summary: string;
  detail: string;
  visibility: string;
  scope: string;
  confidence: number;
  score: number;
  match_type: string;
  created_at: string;
}

export interface Memory {
  id: string;
  topic: string;
  summary: string;
  detail: string;
  visibility: string;
  scope: string;
  confidence: number;
  model: string;
  version: number;
  source: string;
  created_at: string;
  updated_at: string;
  d_tag: string;
  nostr_id: string;
  ephemeral: boolean;
  importance: number | null;
  access_count: number;
  source_time_start: string | null;
  source_time_end: string | null;
  consolidated_from: string | null;
  consolidated_at: string | null;
  pinned: boolean;
  embedded: boolean;
}

export interface MemoryListStats {
  total: number;
  named: number;
  pending: number;
  by_visibility?: Record<string, number>;
}

export interface MemoryListResult {
  count: number;
  memories: Memory[];
  stats?: MemoryListStats;
}

export interface Message {
  id: string;
  source: string;
  source_id: string;
  sender: string;
  channel: string;
  content: string;
  metadata: string;
  consolidated: boolean;
  created_at: string;
  nostr_event_id: string;
  provider_id: string;
  sender_id: string;
  room: string;
  topic: string;
  thread: string;
  scope: string;
  source_created_at: string;
  publish_status: string;
}

export interface MessageListOpts {
  source?: string;
  channel?: string;
  sender?: string;
  room?: string;
  topic?: string;
  thread?: string;
  since?: string;
  until?: string;
  order?: 'asc' | 'desc';
  include_consolidated?: boolean;
  limit?: number;
}

export interface MessageListResult {
  count: number;
  messages: Message[];
}

export interface MessageContextOpts {
  id?: string;
  nostr_event_id?: string;
  provider_id?: string;
  channel?: string;
  source_id?: string;
  before?: number;
  after?: number;
  same_container_only?: boolean;
}

export interface MessageContextResult {
  count: number;
  messages: Message[];
  target_index: number;
}

export interface Group {
  id: string;
  name: string;
  members: string[];
  nostr_group?: string;
  relay?: string;
  created_at: string;
}

export interface Entity {
  name: string;
  kind: string;
  created_at: string;
}

export interface ConsolidateReport {
  messages_processed: number;
  memories_created: number;
  channels: string[];
}

export interface SearchOpts {
  visibility?: string;
  scope?: string;
  limit?: number;
  mode?: 'hybrid' | 'text';
}

export interface ListOpts {
  visibility?: string;
  limit?: number;
  stats?: boolean;
}

export interface EntityOpts {
  kind?: string;
  query?: string;
}

export interface ConsolidateOpts {
  channel?: string;
  since?: string;
}

export interface CreateGroupOpts {
  id: string;
  name: string;
  members?: string[];
  nostr_group?: string;
}

export interface SystemStats {
  total_memories: number;
  named_memories: number;
  ephemeral_messages: number;
  entities: number;
  groups: number;
  last_consolidation: string | null;
  last_prune: string | null;
  db_size_bytes: number;
}

export interface PruneResult {
  memories_pruned: number;
  raw_messages_pruned: number;
  dry_run: boolean;
  pruned: { topic: string; confidence: number | null; age_days: number }[];
}

export interface NomenConfig {
  relay: string | null;
  embedding: { provider: string; model: string; dimensions: number } | null;
  consolidation: Record<string, unknown> | null;
  groups: { id: string; name: string; member_count: number }[];
  config_path: string;
}

export class NomenApi {
  private baseUrl: string;

  constructor(baseUrl: string = '/memory/api') {
    this.baseUrl = baseUrl.replace(/\/$/, '');
  }

  private async postJson<T>(path: string, body: Record<string, unknown>): Promise<T> {
    const resp = await fetch(`${this.baseUrl}${path}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!resp.ok) {
      const text = await resp.text().catch(() => '');
      throw new Error(`${resp.status}: ${text}`);
    }
    return resp.json();
  }

  private async getJson<T>(path: string, params?: Record<string, string | undefined>): Promise<T> {
    const url = new URL(`${this.baseUrl}${path}`, window.location.origin);
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        if (v !== undefined && v !== '') url.searchParams.set(k, v);
      }
    }
    const resp = await fetch(url.toString());
    if (!resp.ok) {
      const text = await resp.text().catch(() => '');
      throw new Error(`${resp.status}: ${text}`);
    }
    return resp.json();
  }

  private async dispatch<T = unknown>(action: string, params: Record<string, unknown> = {}): Promise<T> {
    const envelope = await this.postJson<{ ok: boolean; result: T; error?: { code: string; message: string } }>('/dispatch', { action, params });
    if (!envelope.ok) {
      throw new Error(envelope.error?.message ?? 'Unknown error');
    }
    return envelope.result;
  }

  async search(query: string, opts?: SearchOpts): Promise<SearchResult[]> {
    const data = await this.dispatch<{ results: RawSearchResult[] }>('memory.search', {
      query,
      visibility: opts?.visibility,
      scope: opts?.scope,
      limit: opts?.limit,
      mode: opts?.mode,
    });
    return data.results.map(mapSearchResult);
  }

  async listMemories(opts?: ListOpts): Promise<MemoryListResult> {
    const params: Record<string, unknown> = {
      limit: opts?.limit ?? 500,
      stats: opts?.stats ?? true,
    };
    if (opts?.visibility) params.visibility = opts.visibility;
    const data = await this.dispatch<MemoryListResult>('memory.list', params);
    // Normalize visibilities
    for (const m of data.memories) {
      m.visibility = normalizeVisibility(m.visibility);
    }
    return data;
  }

  async deleteMemory(opts: { topic?: string; d_tag?: string; id?: string }): Promise<{ deleted: boolean; d_tag?: string }> {
    return this.dispatch('memory.delete', opts as Record<string, unknown>);
  }

  async pinMemory(d_tag: string): Promise<{ pinned: boolean; d_tag: string }> {
    return this.dispatch('memory.pin', { d_tag });
  }

  async unpinMemory(d_tag: string): Promise<{ pinned: boolean; d_tag: string }> {
    return this.dispatch('memory.unpin', { d_tag });
  }

  async getEntities(opts?: EntityOpts): Promise<Entity[]> {
    const data = await this.dispatch<{ entities: Entity[] }>('entity.list', {
      kind: opts?.kind,
      query: opts?.query,
    });
    return data.entities;
  }

  async consolidate(opts?: ConsolidateOpts): Promise<ConsolidateReport> {
    return this.dispatch('memory.consolidate', { ...opts });
  }

  async getGroups(): Promise<Group[]> {
    const data = await this.dispatch<{ groups: Group[] }>('group.list');
    return data.groups;
  }

  async createGroup(opts: CreateGroupOpts): Promise<{ created: string }> {
    return this.dispatch('group.create', opts as unknown as Record<string, unknown>);
  }

  async getConfig(): Promise<NomenConfig> {
    return this.getJson('/config');
  }

  async reloadConfig(): Promise<NomenConfig> {
    return this.postJson('/config/reload', {});
  }

  async getStats(): Promise<SystemStats> {
    return this.getJson('/stats');
  }

  async prune(days: number, dryRun: boolean): Promise<PruneResult> {
    return this.dispatch('memory.prune', { days, dry_run: dryRun });
  }

  async listMessages(opts?: MessageListOpts): Promise<MessageListResult> {
    return this.dispatch('message.list', { ...opts, limit: opts?.limit ?? 100 });
  }

  async getMessageContext(opts: MessageContextOpts): Promise<MessageContextResult> {
    return this.dispatch('message.context', { ...opts });
  }
}

// ── Raw backend types & mappers ──────────────────────────────────

interface RawSearchResult {
  visibility: string;
  topic: string;
  confidence: string;
  summary: string;
  detail: string;
  scope: string;
  created_at: string;
  score: number;
  match_type: string;
}

function mapSearchResult(r: RawSearchResult): SearchResult {
  return {
    topic: r.topic,
    summary: r.summary,
    detail: r.detail || '',
    visibility: normalizeVisibility(r.visibility),
    scope: r.scope || '',
    confidence: parseFloat(r.confidence) || 0,
    score: r.score,
    match_type: r.match_type,
    created_at: r.created_at || '',
  };
}
