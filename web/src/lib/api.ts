// Nomen API Client — REST calls for server-side operations only (search, entities, consolidation)

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
  id: string;
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
  mode?: 'hybrid' | 'text';
}

export interface EntityOpts {
  kind?: string;
  query?: string;
}

export interface ConsolidateOpts {
  channel?: string;
  since?: string;
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

export interface ConsolidationStatus {
  due: boolean;
  reason: string;
  last_run: string | null;
  hours_since_last_run: number | null;
  pending_messages: number;
  interval_hours: number;
  max_ephemeral_count: number;
  enabled: boolean;
  ephemeral_ttl_minutes: number;
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

  async search(query: string, opts?: SearchOpts): Promise<SearchResult[]> {
    const data = await this.postJson<{ results: RawSearchResult[] }>('/search', {
      query,
      tier: opts?.tier,
      scope: opts?.scope,
      limit: opts?.limit,
      mode: opts?.mode,
    });
    return data.results.map(mapSearchResult);
  }

  async getEntities(opts?: EntityOpts): Promise<Entity[]> {
    const data = await this.getJson<{ entities: Entity[] }>('/entities', {
      kind: opts?.kind,
      query: opts?.query,
    });
    return data.entities;
  }

  async consolidate(opts?: ConsolidateOpts): Promise<ConsolidateReport> {
    return this.postJson('/consolidate', { ...opts });
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

  async getConsolidationStatus(): Promise<ConsolidationStatus> {
    return this.getJson('/consolidate/status');
  }

  async prune(days: number, dryRun: boolean): Promise<PruneResult> {
    return this.postJson('/prune', { days, dry_run: dryRun });
  }
}

// ── Raw backend types & mappers ──────────────────────────────────

interface RawSearchResult {
  tier: string;
  topic: string;
  confidence: string;
  summary: string;
  created_at: number;
  score: number;
  match_type: string;
}

function mapSearchResult(r: RawSearchResult): SearchResult {
  return {
    topic: r.topic,
    summary: r.summary,
    detail: '',
    tier: r.tier,
    scope: '',
    confidence: parseFloat(r.confidence) || 0,
    score: r.score,
    match_type: r.match_type,
    created_at: r.created_at ? new Date(r.created_at * 1000).toISOString() : '',
  };
}
