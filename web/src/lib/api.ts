// Nomen API Client — wraps calls to Nomen MCP/REST endpoint

export interface SearchResult {
  topic: string;
  summary: string;
  detail: string;
  tier: string;
  scope: string;
  confidence: number;
  score: number;
  match_type: string;
  created_at: string;
}

export interface Memory {
  topic: string;
  summary: string;
  detail: string;
  tier: string;
  scope: string;
  confidence: number;
  model: string;
  version: string;
  source: string;
  created_at: string;
  d_tag: string;
}

export interface Message {
  id: string;
  source: string;
  sender: string;
  channel: string;
  content: string;
  metadata: string;
  consolidated: boolean;
  created_at: string;
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
  tier?: string;
  scope?: string;
  limit?: number;
}

export interface StoreOpts {
  detail?: string;
  tier?: string;
  scope?: string;
  confidence?: number;
}

export interface IngestOpts {
  sender?: string;
  channel?: string;
  metadata?: Record<string, unknown>;
}

export interface MessageOpts {
  source?: string;
  channel?: string;
  sender?: string;
  since?: string;
  limit?: number;
}

export interface ConsolidateOpts {
  channel?: string;
  since?: string;
}

export interface EntityOpts {
  kind?: string;
  query?: string;
}

export interface SendOpts {
  channel?: string;
  metadata?: Record<string, unknown>;
}

export interface SendResult {
  ok: boolean;
  message: string;
}

export class NomenClient {
  private baseUrl: string;
  private useMock: boolean = false;

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl.replace(/\/$/, '');
  }

  private async request<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
    try {
      const resp = await fetch(`${this.baseUrl}/rpc`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          jsonrpc: '2.0',
          id: crypto.randomUUID(),
          method: 'tools/call',
          params: { name: method, arguments: params },
        }),
      });

      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);

      const data = await resp.json();
      if (data.error) throw new Error(data.error.message);

      const content = data.result?.content?.[0]?.text;
      if (data.result?.isError) throw new Error(content || 'Unknown error');

      return JSON.parse(content) as T;
    } catch {
      this.useMock = true;
      throw new Error('Backend unavailable — using mock data');
    }
  }

  get isMockMode(): boolean {
    return this.useMock;
  }

  enableMock(): void {
    this.useMock = true;
  }

  async search(query: string, opts?: SearchOpts): Promise<SearchResult[]> {
    if (this.useMock) return (await import('./mock')).mockSearch(query, opts);
    return this.request('nomen_search', { query, ...opts });
  }

  async store(topic: string, summary: string, opts?: StoreOpts): Promise<void> {
    if (this.useMock) return;
    await this.request('nomen_store', { topic, summary, ...opts });
  }

  async ingest(source: string, content: string, opts?: IngestOpts): Promise<void> {
    if (this.useMock) return;
    await this.request('nomen_ingest', { source, content, ...opts });
  }

  async getMessages(opts?: MessageOpts): Promise<Message[]> {
    if (this.useMock) return (await import('./mock')).mockMessages(opts);
    return this.request('nomen_messages', { ...opts });
  }

  async consolidate(opts?: ConsolidateOpts): Promise<ConsolidateReport> {
    if (this.useMock) return { messages_processed: 0, memories_created: 0, channels: [] };
    return this.request('nomen_consolidate', { ...opts });
  }

  async listGroups(): Promise<Group[]> {
    if (this.useMock) return (await import('./mock')).mockGroups();
    return this.request('nomen_groups', { action: 'list' });
  }

  async listEntities(opts?: EntityOpts): Promise<Entity[]> {
    if (this.useMock) return (await import('./mock')).mockEntities(opts);
    return this.request('nomen_entities', { ...opts });
  }

  async send(recipient: string, content: string, opts?: SendOpts): Promise<SendResult> {
    if (this.useMock) return { ok: true, message: 'Mock send' };
    return this.request('nomen_send', { recipient, content, ...opts });
  }

  async deleteMemory(topic: string): Promise<void> {
    if (this.useMock) return;
    await this.request('nomen_delete', { topic });
  }

  async listMemories(tier?: string): Promise<Memory[]> {
    if (this.useMock) return (await import('./mock')).mockMemories(tier);
    return this.request('nomen_search', { query: '*', tier, limit: 100 });
  }

  async createGroup(id: string, name: string, members: string[]): Promise<void> {
    if (this.useMock) return;
    await this.request('nomen_groups', { action: 'create', id, name, members });
  }

  async addMember(groupId: string, npub: string): Promise<void> {
    if (this.useMock) return;
    await this.request('nomen_groups', { action: 'add_member', id: groupId, npub });
  }

  async removeMember(groupId: string, npub: string): Promise<void> {
    if (this.useMock) return;
    await this.request('nomen_groups', { action: 'remove_member', id: groupId, npub });
  }
}
