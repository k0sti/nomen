// Nostr relay client — applesauce-relay wrapper with NIP-42 AUTH

import { Relay } from 'applesauce-relay';
import { EventStore, QueryStore } from 'applesauce-core';
import { firstValueFrom, toArray, filter, map } from 'rxjs';
import { type EventTemplate, type NostrEvent } from 'nostr-tools';
import type { Memory, Message, Group } from './api';

export type AuthSigner = {
  getPublicKey(): Promise<string>;
  signEvent(event: EventTemplate): NostrEvent | Promise<NostrEvent>;
};

export interface Subscription {
  close(): void;
}

// ── Singleton relay, event store, and query store ───────────────

export const relay = new Relay('wss://zooid.atlantislabs.space');
export const eventStore = new EventStore();
export const queryStore = new QueryStore(eventStore);

// ── Connection and auth state ────────────────────────────────────

let currentSigner: AuthSigner | null = null;

// Subscribe relay events to event store
relay.request([{}]).subscribe((event) => {
  if (typeof event === 'object' && event && 'id' in event) {
    eventStore.add(event as NostrEvent);
  }
});

// ── Auth helpers ─────────────────────────────────────────────────

export async function ensureAuthenticated(signer: AuthSigner): Promise<void> {
  currentSigner = signer;

  // Wait for relay to be connected first
  await firstValueFrom(relay.connected$.pipe(filter(connected => connected)));

  // Authenticate if not already authenticated
  if (!await isAuthenticated()) {
    await firstValueFrom(relay.authenticate(signer));
  }
}

export async function isAuthenticated(): Promise<boolean> {
  return firstValueFrom(relay.authenticated$);
}

export function getConnectionStatus() {
  return relay.connected$;
}

export function getAuthStatus() {
  return relay.authenticated$;
}

// ── Helper for converting RxJS to promises ──────────────────────

async function requestToPromise(filters: any[], timeoutMs = 15000): Promise<NostrEvent[]> {
  const timeout = new Promise<never>((_, reject) =>
    setTimeout(() => reject(new Error('Request timeout')), timeoutMs)
  );

  const request = relay.request(filters).pipe(
    filter(event => typeof event === 'object' && event && 'id' in event),
    toArray()
  );

  return Promise.race([firstValueFrom(request), timeout]) as Promise<NostrEvent[]>;
}

async function publishToPromise(event: NostrEvent, timeoutMs = 10000): Promise<string> {
  const timeout = new Promise<never>((_, reject) =>
    setTimeout(() => reject(new Error('Publish timeout')), timeoutMs)
  );

  const publish = relay.publish(event).pipe(
    map(response => {
      if (response.ok) {
        return event.id;
      } else {
        throw new Error(response.message || 'Event rejected');
      }
    })
  );

  return Promise.race([firstValueFrom(publish), timeout]) as Promise<string>;
}

// ── Memory operations ────────────────────────────────────────────

export async function listMemories(_pubkey?: string): Promise<Memory[]> {
  // Fetch all memory events on this relay (not filtered by author)
  const events = await requestToPromise([{
    kinds: [30078],
    limit: 500,
  }]);

  return events
    .filter((e) => isMemoryEvent(e))
    .map((e) => parseMemory(e));
}

export function subscribeMemories(_pubkey: string | undefined, callback: (m: Memory) => void): Subscription {
  const subscription = relay.subscription([{
    kinds: [30078],
    since: Math.floor(Date.now() / 1000),
  }]).pipe(
    filter(event => typeof event === 'object' && event && 'id' in event && event !== 'EOSE'),
    filter(event => isMemoryEvent(event as NostrEvent)),
    map(event => parseMemory(event as NostrEvent))
  ).subscribe(callback);

  return {
    close: () => subscription.unsubscribe(),
  };
}

export async function storeMemory(
  topic: string,
  summary: string,
  detail: string,
  tier: string,
  signer: AuthSigner
): Promise<string> {
  await ensureAuthenticated(signer);

  const dTag = `snow:memory:${tier}:${topic}`;
  const content = JSON.stringify({
    summary,
    detail,
    topic,
    confidence: 0.8,
    created_at: Math.floor(Date.now() / 1000),
  });

  const event: EventTemplate = {
    kind: 30078,
    created_at: Math.floor(Date.now() / 1000),
    tags: [
      ['d', dTag],
      ['snow:tier', tier],
      ['snow:topic', topic],
      ['snow:confidence', '0.8'],
      ['snow:version', '1'],
    ],
    content,
  };

  const signed = await signer.signEvent(event);
  const signedEvent = typeof signed === 'object' && 'sig' in signed ? signed : await signed;
  return publishToPromise(signedEvent);
}

export async function deleteMemory(eventId: string, signer: AuthSigner, dTag?: string): Promise<void> {
  await ensureAuthenticated(signer);

  const pubkey = await signer.getPublicKey();
  const tags: string[][] = [['e', eventId]];
  // Kind 30078 are addressable events — use a-tag for proper NIP-09 deletion
  if (dTag) {
    tags.push(['a', `30078:${pubkey}:${dTag}`]);
  }

  const event: EventTemplate = {
    kind: 5,
    created_at: Math.floor(Date.now() / 1000),
    tags,
    content: 'Memory deleted via Nomen UI',
  };

  const signed = await signer.signEvent(event);
  const signedEvent = typeof signed === 'object' && 'sig' in signed ? signed : await signed;
  await publishToPromise(signedEvent);
}

// ── Group messages (kind 9) ──────────────────────────────────────

export async function getGroupMessages(groupId: string, limit: number = 50): Promise<Message[]> {
  const events = await requestToPromise([{
    kinds: [9],
    '#h': [groupId],
    limit,
  }]);

  return events.map((e) => parseMessage(e, groupId));
}

// ── NIP-29 groups (kind 39000) ───────────────────────────────────

export async function listGroups(): Promise<Group[]> {
  const events = await requestToPromise([{
    kinds: [39000],
    limit: 100,
  }]);

  return events.map((e) => parseGroup(e));
}

// ── Generic NIP-78 app data ──────────────────────────────────────

export async function fetchAppData(pubkey: string, dTag: string): Promise<NostrEvent | null> {
  const events = await requestToPromise([{
    kinds: [30078],
    authors: [pubkey],
    '#d': [dTag],
    limit: 1,
  }]);
  return events[0] || null;
}

export async function publishAppData(dTag: string, content: string, signer: AuthSigner): Promise<string> {
  await ensureAuthenticated(signer);

  const event: EventTemplate = {
    kind: 30078,
    created_at: Math.floor(Date.now() / 1000),
    tags: [['d', dTag]],
    content,
  };

  const signed = await signer.signEvent(event);
  const signedEvent = typeof signed === 'object' && 'sig' in signed ? signed : await signed;
  return publishToPromise(signedEvent);
}

// ── Profiles (kind 0) ────────────────────────────────────────────

export async function listProfiles(): Promise<{ pubkey: string; meta: Record<string, any>; created_at: number }[]> {
  // Step 1: Query kind 0 profiles on this relay
  const kind0Events = await requestToPromise([{ kinds: [0], limit: 500 }]);
  const profileMap = new Map<string, { meta: Record<string, any>; created_at: number }>();

  for (const e of kind0Events) {
    try {
      profileMap.set(e.pubkey, {
        meta: JSON.parse(e.content),
        created_at: e.created_at,
      });
    } catch { /* skip malformed */ }
  }

  // Return only pubkeys that have a kind 0 profile on this relay
  const profiles: { pubkey: string; meta: Record<string, any>; created_at: number }[] = [];
  for (const [pubkey, p] of profileMap) {
    profiles.push({ pubkey, meta: p.meta, created_at: p.created_at });
  }

  return profiles;
}

// ── Forward a pre-signed event to this relay ────────────────────

export async function publishEvent(event: NostrEvent): Promise<string> {
  return publishToPromise(event);
}

// ── Legacy compatibility layer ───────────────────────────────────

export class NomenRelay {
  private _url: string;
  onConnectionChange?: (connected: boolean) => void;

  constructor(url: string = 'wss://zooid.atlantislabs.space') {
    this._url = url;

    // Subscribe to connection changes
    getConnectionStatus().subscribe(connected => {
      this.onConnectionChange?.(connected);
    });
  }

  get url(): string {
    return this._url;
  }

  async connect(): Promise<void> {
    // applesauce-relay connects automatically, just wait for connection
    await firstValueFrom(relay.connected$.pipe(filter(connected => connected)));
  }

  async authenticate(signer: AuthSigner): Promise<void> {
    await ensureAuthenticated(signer);
  }

  async listMemories(pubkey?: string): Promise<Memory[]> {
    return listMemories(pubkey);
  }

  subscribeMemories(pubkey: string | undefined, callback: (m: Memory) => void): Subscription {
    return subscribeMemories(pubkey, callback);
  }

  async storeMemory(
    topic: string,
    summary: string,
    detail: string,
    tier: string,
    signer: AuthSigner
  ): Promise<string> {
    return storeMemory(topic, summary, detail, tier, signer);
  }

  async deleteMemory(eventId: string, signer: AuthSigner, dTag?: string): Promise<void> {
    return deleteMemory(eventId, signer, dTag);
  }

  async getGroupMessages(groupId: string, limit: number = 50): Promise<Message[]> {
    return getGroupMessages(groupId, limit);
  }

  async listGroups(): Promise<Group[]> {
    return listGroups();
  }

  async fetchAppData(pubkey: string, dTag: string): Promise<NostrEvent | null> {
    return fetchAppData(pubkey, dTag);
  }

  async publishAppData(dTag: string, content: string, signer: AuthSigner): Promise<string> {
    return publishAppData(dTag, content, signer);
  }

  async listProfiles(): Promise<{ pubkey: string; meta: Record<string, any>; created_at: number }[]> {
    return listProfiles();
  }

  async publishEvent(event: NostrEvent): Promise<string> {
    return publishEvent(event);
  }

  disconnect() {
    // applesauce-relay manages its own connection lifecycle
    currentSigner = null;
  }
}

// ── Parsers ───────────────────────────────────────────────────────

function isMemoryEvent(event: NostrEvent): boolean {
  const dTag = event.tags.find((t) => t[0] === 'd')?.[1] || '';
  return dTag.startsWith('snow:memory:') || dTag.startsWith('snowclaw:memory:');
}

function parseMemory(event: NostrEvent): Memory {
  const dTag = event.tags.find((t) => t[0] === 'd')?.[1] || '';
  const tierTag = event.tags.find((t) => t[0] === 'snow:tier')?.[1] || '';
  const topicTag = event.tags.find((t) => t[0] === 'snow:topic')?.[1] || '';
  const confTag = event.tags.find((t) => t[0] === 'snow:confidence')?.[1] || '';
  const modelTag = event.tags.find((t) => t[0] === 'snow:model')?.[1] || '';
  const versionTag = event.tags.find((t) => t[0] === 'snow:version')?.[1] || '1';
  const sourceTag = event.tags.find((t) => t[0] === 'snow:source')?.[1] || '';

  // Parse content JSON
  let summary = '';
  let detail = '';
  let topic = topicTag;
  let confidence = parseFloat(confTag) || 0.8;

  try {
    const parsed = JSON.parse(event.content);
    summary = parsed.summary || '';
    detail = parsed.detail || '';
    if (parsed.topic && !topic) topic = parsed.topic;
    if (parsed.confidence) confidence = parsed.confidence;
  } catch {
    // If content isn't JSON, use raw content as detail
    detail = event.content;
  }

  // Extract tier and scope from d-tag if not in tags
  let tier = tierTag;
  let scope = '';
  if (!tier) {
    const parts = dTag.replace('snow:memory:', '').replace('snowclaw:memory:', '').split(':');
    if (parts[0] === 'core' || parts[0] === 'public') {
      tier = 'public';
    } else if (parts[0] === 'group') {
      tier = 'group';
      scope = parts[1] || '';
    } else if (parts[0] === 'npub') {
      tier = 'private';
      scope = parts[1] || '';
    } else if (parts[0] === 'lesson') {
      tier = 'public';
    } else {
      tier = 'public';
    }
  }

  // Extract topic from d-tag if not in tags
  if (!topic) {
    // d-tag format: snow:memory:<tier>:<topic> or snow:memory:<topic>
    const prefix = dTag.startsWith('snowclaw:memory:') ? 'snowclaw:memory:' : 'snow:memory:';
    const rest = dTag.slice(prefix.length);
    const colonIdx = rest.indexOf(':');
    topic = colonIdx >= 0 ? rest.slice(colonIdx + 1) : rest;
  }

  return {
    id: event.id,
    topic,
    summary,
    detail,
    tier,
    scope,
    confidence,
    model: modelTag,
    version: versionTag,
    source: sourceTag,
    created_at: new Date(event.created_at * 1000).toISOString(),
    d_tag: dTag,
  };
}

function parseMessage(event: NostrEvent, groupId: string): Message {
  const channel = event.tags.find((t) => t[0] === 'h')?.[1] || groupId;

  return {
    id: event.id,
    source: 'nostr',
    sender: event.pubkey.slice(0, 12) + '...',
    channel,
    content: event.content,
    metadata: '',
    consolidated: false,
    created_at: new Date(event.created_at * 1000).toISOString(),
  };
}

function parseGroup(event: NostrEvent): Group {
  const dTag = event.tags.find((t) => t[0] === 'd')?.[1] || '';
  const name = event.tags.find((t) => t[0] === 'name')?.[1] || dTag;
  const members = event.tags.filter((t) => t[0] === 'p').map((t) => t[1]);

  return {
    id: dTag,
    name,
    members,
    nostr_group: dTag,
    relay: relay.url,
    created_at: new Date(event.created_at * 1000).toISOString(),
  };
}