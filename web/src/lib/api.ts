// Nomen API Client — REST calls for server-side operations only (search, entities, consolidation)

export interface SearchResult {
  topic: string;
  content: string;
  visibility: string;
  scope: string;
  score: number;
  match_type: string;
  created_at: string;
}

export interface Memory {
  id: string;
  topic: string;
  content: string;
  visibility: string;
  scope: string;
  model: string;
  version: number;
  source: string;
  created_at: string;
  d_tag: string;
  importance?: number;
}

export interface Message {
  id: string;
  source: string;
  sender: string;
  // Legacy compatibility field; canonical UI should prefer platform/community/chat/thread.
  channel: string;
  platform?: string;
  community?: string;
  chat?: string;
  thread?: string;
  message_id?: string;
  source_id?: string;
  content: string;
  metadata: string;
  consolidated: boolean;
  created_at: string;
}

export interface MessageListOpts {
  platform?: string;
  community?: string;
  chat?: string;
  thread?: string;
  sender?: string;
  since?: string;
  limit?: number;
  include_consolidated?: boolean;
}

export interface MessageListResult {
  count: number;
  messages: Message[];
}

export interface MessageContextOpts {
  platform?: string;
  community?: string;
  chat: string;
  thread?: string;
  sender?: string;
  before?: number;
  since?: number;
  limit?: number;
}

export interface MessageContextResult {
  count: number;
  messages: Message[];
  target_index?: number;
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
  containers?: string[];
}

export interface SearchOpts {
  visibility?: string;
  scope?: string;
  limit?: number;
  mode?: 'hybrid' | 'text';
}

export interface EntityOpts {
  kind?: string;
  query?: string;
}

export interface ConsolidateOpts {
  platform?: string;
  community?: string;
  chat?: string;
  thread?: string;
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
  dry_run: boolean;
  pruned: { topic: string; age_days: number }[];
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


  async listMessages(opts?: MessageListOpts): Promise<MessageListResult> {
    const params: Record<string, unknown> = { limit: opts?.limit ?? 100 };
    if (opts?.platform) params["#proxy"] = [opts.platform];
    if (opts?.community) params["#community"] = [opts.community];
    if (opts?.chat) params["#chat"] = [opts.chat];
    if (opts?.thread) params["#thread"] = [opts.thread];
    if (opts?.sender) params["#sender"] = [opts.sender];
    if (opts?.since) params.since = opts.since;

    const data = await this.dispatch<{ count: number; events: RawCollectedEvent[] }>('message.query', params);
    return {
      count: data.count,
      messages: (data.events || []).map(mapCollectedEvent),
    };
  }

  async getMessageContext(opts: MessageContextOpts): Promise<MessageContextResult> {
    const params: Record<string, unknown> = { '#chat': [opts.chat], limit: opts.limit ?? 50 };
    if (opts.platform) params['#proxy'] = [opts.platform];
    if (opts.community) params['#community'] = [opts.community];
    if (opts.thread) params['#thread'] = [opts.thread];
    if (opts.sender) params['#sender'] = [opts.sender];
    if (opts.before !== undefined) params.before = opts.before;
    if (opts.since !== undefined) params.since = opts.since;

    const data = await this.dispatch<{ count: number; messages: RawContextMessage[] }>('message.context', params);
    return {
      count: data.count,
      messages: (data.messages || []).map(mapContextMessage),
    };
  }

  async getEntities(opts?: EntityOpts): Promise<Entity[]> {
    const data = await this.dispatch<{ entities: Entity[] }>('entity.list', {
      kind: opts?.kind,
      query: opts?.query,
    });
    return data.entities;
  }

  async consolidate(opts?: ConsolidateOpts): Promise<ConsolidateReport> {
    const params: Record<string, unknown> = {};
    if (opts?.platform) params['#proxy'] = [opts.platform];
    if (opts?.community) params['#community'] = [opts.community];
    if (opts?.chat) params['#chat'] = [opts.chat];
    if (opts?.thread) params['#thread'] = [opts.thread];
    if (opts?.since) params.since = opts.since;
    return this.dispatch('memory.consolidate', params);
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
}

// ── Raw backend types & mappers ──────────────────────────────────

interface RawSearchResult {
  visibility: string;
  topic: string;
  content: string;
  scope: string;
  created_at: string;
  score: number;
  match_type: string;
}

function mapSearchResult(r: RawSearchResult): SearchResult {
  return {
    topic: r.topic,
    content: r.content || '',
    visibility: r.visibility,
    scope: r.scope || '',
    score: r.score,
    match_type: r.match_type,
    created_at: r.created_at || '',
  };
}

interface RawCollectedEvent {
  id?: string;
  kind?: number;
  content?: string;
  created_at?: number;
  tags?: Array<Array<string>>;
}

interface RawContextMessage {
  sender?: string;
  platform?: string;
  community?: string;
  chat?: string;
  thread?: string;
  message_id?: string;
  content?: string;
  created_at?: number;
}

function tagValue(tags: Array<Array<string>> | undefined, name: string, index = 1): string {
  const tag = tags?.find((t) => t[0] === name);
  return tag?.[index] || '';
}

function mapCollectedEvent(event: RawCollectedEvent): Message {
  const tags = event.tags || [];
  const platform = tagValue(tags, 'proxy', 2);
  const chat = tagValue(tags, 'chat', 1);
  const thread = tagValue(tags, 'thread', 1);
  const sender = tagValue(tags, 'sender', 1);
  const sourceId = tagValue(tags, 'd', 1);
  return {
    id: sourceId || event.id || '',
    source: platform || '',
    sender,
    channel: thread ? `${chat || ''}/${thread}`.replace(/^\//, '') : chat,
    platform: platform || undefined,
    community: tagValue(tags, 'community', 1) || undefined,
    chat: chat || undefined,
    thread: thread || undefined,
    message_id: tagValue(tags, 'message', 1) || undefined,
    source_id: sourceId || undefined,
    content: event.content || '',
    metadata: '',
    consolidated: false,
    created_at: event.created_at ? new Date(event.created_at * 1000).toISOString() : '',
  };
}

function mapContextMessage(msg: RawContextMessage): Message {
  const chat = msg.chat || '';
  const thread = msg.thread || '';
  return {
    id: msg.message_id || '',
    source: msg.platform || '',
    sender: msg.sender || '',
    channel: thread ? `${chat}/${thread}` : chat,
    platform: msg.platform || undefined,
    community: msg.community || undefined,
    chat: chat || undefined,
    thread: thread || undefined,
    message_id: msg.message_id || undefined,
    source_id: msg.message_id || undefined,
    content: msg.content || '',
    metadata: '',
    consolidated: false,
    created_at: msg.created_at ? new Date(msg.created_at * 1000).toISOString() : '',
  };
}
