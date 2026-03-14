// Nostr relay client — applesauce-relay + applesauce-core integration
//
// applesauce-relay handles: WebSocket, reconnect, NIP-42 AUTH flow, retry on auth-required
// applesauce-core handles: EventStore (local cache), QueryStore (reactive queries)
//
// Key insight: relay.request()/subscription() automatically WAIT for authentication
// when auth-required is detected. We just need to call authenticate() when the
// challenge arrives, and everything else flows automatically.

import { Relay as ApplesauceRelay } from 'applesauce-relay';
import { EventStore, QueryStore } from 'applesauce-core';
import { firstValueFrom, filter as rxFilter, toArray, map, Observable, Subscription as RxSubscription } from 'rxjs';
import { type EventTemplate, type NostrEvent } from 'nostr-tools';
import type { Memory, Message, Group } from './api';

// ── Types ────────────────────────────────────────────────────────

export type Signer = {
  getPublicKey(): Promise<string>;
  signEvent(event: EventTemplate): NostrEvent | Promise<NostrEvent>;
};

export interface Subscription {
  close(): void;
}

// ── Singleton instances ──────────────────────────────────────────

let relayInstance: ApplesauceRelay | null = null;
let relayUrl = 'wss://zooid.atlantislabs.space';
let currentSigner: Signer | null = null;
let authSubscription: RxSubscription | null = null;

export const eventStore = new EventStore();
export const queryStore = new QueryStore(eventStore);

function getRelay(): ApplesauceRelay {
  if (!relayInstance || relayInstance.url !== relayUrl) {
    // Clean up old auth subscription
    authSubscription?.unsubscribe();
    relayInstance = new ApplesauceRelay(relayUrl);
    setupAutoAuth(relayInstance);
  }
  return relayInstance;
}

// ── Auto-authentication ──────────────────────────────────────────
// Subscribe to challenge$ and automatically authenticate when a challenge arrives.
// This is the correct pattern for applesauce-relay: requests/subscriptions
// automatically WAIT for authenticated$ to become true when auth is required.

function setupAutoAuth(relay: ApplesauceRelay) {
  authSubscription?.unsubscribe();
  authSubscription = relay.challenge$.pipe(
    rxFilter(challenge => challenge !== null),
  ).subscribe((_challenge) => {
    if (currentSigner && !relay.authenticated) {
      try {
        relay.authenticate(currentSigner).subscribe({
          next: (result) => {
            if (result.ok) {
              console.log('[relay] Authenticated successfully');
            } else {
              console.warn('[relay] Auth rejected:', result.message);
            }
          },
          error: (err) => console.warn('[relay] Auth error:', err.message),
        });
      } catch (err: any) {
        console.warn('[relay] authenticate() threw:', err.message);
      }
    }
  });
}

// ── Public API ───────────────────────────────────────────────────

export function setRelayUrl(url: string) {
  if (url !== relayUrl) {
    relayUrl = url;
    relayInstance = null; // force re-create on next getRelay()
  }
}

export function setSigner(signer: Signer | null) {
  currentSigner = signer;
  // If relay is connected and has a challenge but isn't authenticated, authenticate now
  const relay = relayInstance;
  if (signer && relay && relay.challenge && !relay.authenticated) {
    try {
      relay.authenticate(signer).subscribe({
        error: (err) => console.warn('[relay] Auth error on setSigner:', err.message),
      });
    } catch { /* ignore */ }
  }
}

export function getConnectionStatus(): Observable<boolean> {
  return getRelay().connected$;
}

export function getAuthStatus(): Observable<boolean> {
  return getRelay().authenticated$;
}

// ── Request helpers ──────────────────────────────────────────────

async function requestEvents(filters: any[], timeoutMs = 15000): Promise<NostrEvent[]> {
  const r = getRelay();
  const events: NostrEvent[] = [];

  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      sub.unsubscribe();
      resolve(events);
    }, timeoutMs);

    const sub = r.request(filters).subscribe({
      next: (event) => {
        if (typeof event === 'object' && event && 'id' in event) {
          events.push(event as NostrEvent);
          eventStore.add(event as NostrEvent);
        }
      },
      complete: () => {
        clearTimeout(timer);
        resolve(events);
      },
      error: (err) => {
        clearTimeout(timer);
        reject(err);
      },
    });
  });
}

async function publishEvent(event: NostrEvent, timeoutMs = 10000): Promise<string> {
  const relay = getRelay();

  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      sub.unsubscribe();
      reject(new Error('Publish timeout'));
    }, timeoutMs);

    const sub = relay.publish(event).subscribe({
      next: (result) => {
        clearTimeout(timer);
        if (result.ok) {
          resolve(event.id);
        } else {
          reject(new Error(result.message || 'Event rejected'));
        }
      },
      error: (err) => {
        clearTimeout(timer);
        reject(err);
      },
    });
  });
}

// ── Memory helpers ───────────────────────────────────────────────

function isMemoryEvent(e: NostrEvent): boolean {
  const dTag = e.tags.find(t => t[0] === 'd')?.[1];
  if (!dTag) return false;
  // Exclude config events
  if (dTag.startsWith('nomen:config:') || dTag.startsWith('snowclaw:config:')) return false;
  return true;
}

function parseMemory(e: NostrEvent): Memory {
  let parsed: any = {};
  try { parsed = JSON.parse(e.content); } catch { /* skip */ }

  const getTag = (name: string) => e.tags.find(t => t[0] === name)?.[1];

  return {
    id: e.id,
    d_tag: getTag('d') || '',
    topic: getTag('snow:topic') || parsed.topic || '',
    summary: parsed.summary || '',
    detail: parsed.detail || '',
    visibility: getTag('snow:tier') || parsed.tier || 'public',
    scope: getTag('snow:scope') || getTag('h') || parsed.scope || '',
    confidence: parseFloat(getTag('snow:confidence') || String(parsed.confidence || '0.8')),
    source: e.pubkey,
    model: parsed.model || '',
    created_at: new Date(e.created_at * 1000).toISOString(),
    version: parseInt(getTag('snow:version') || String(parsed.version || '1'), 10),
  };
}

// ── Exported operations ──────────────────────────────────────────

export async function listMemories(_pubkey?: string): Promise<Memory[]> {
  const events = await requestEvents([{ kinds: [30078], limit: 500 }]);
  return events.filter(isMemoryEvent).map(parseMemory);
}

export function subscribeMemories(_pubkey: string | undefined, callback: (m: Memory) => void): Subscription {
  const relay = getRelay();
  const sub = relay.subscription(
    [{ kinds: [30078], since: Math.floor(Date.now() / 1000) }],
  ).subscribe({
    next: (resp) => {
      if (resp === 'EOSE') return;
      const event = resp as NostrEvent;
      if (isMemoryEvent(event)) {
        eventStore.add(event);
        callback(parseMemory(event));
      }
    },
    error: (err) => console.warn('[relay] Memory subscription error:', err.message),
  });

  return { close: () => sub.unsubscribe() };
}

export async function storeMemory(
  topic: string,
  summary: string,
  detail: string,
  tier: string,
  signer: Signer,
): Promise<string> {
  const dTag = `snow:memory:${tier}:${topic}`;
  const content = JSON.stringify({
    summary, detail, topic,
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
  return publishEvent(signed as NostrEvent);
}

export async function deleteMemory(eventId: string, signer: Signer, dTag?: string): Promise<void> {
  const tags: string[][] = [['e', eventId]];
  if (dTag) tags.push(['a', `30078:${await signer.getPublicKey()}:${dTag}`]);

  const event: EventTemplate = {
    kind: 5,
    created_at: Math.floor(Date.now() / 1000),
    tags,
    content: '',
  };

  const signed = await signer.signEvent(event);
  await publishEvent(signed as NostrEvent);
}

// ── Profile operations ───────────────────────────────────────────

export async function listMembers(): Promise<{ pubkey: string; meta: Record<string, any>; created_at: number; hasProfile: boolean }[]> {
  // Step 1: Get all kind 0 profiles on this relay
  const profileEvents = await requestEvents([{ kinds: [0], limit: 500 }]);
  const profileMap = new Map<string, { meta: Record<string, any>; created_at: number }>();

  for (const e of profileEvents) {
    try {
      const existing = profileMap.get(e.pubkey);
      if (!existing || e.created_at > existing.created_at) {
        profileMap.set(e.pubkey, { meta: JSON.parse(e.content), created_at: e.created_at });
      }
    } catch { /* skip malformed */ }
  }

  // Step 2: Discover all pubkeys that have sent events (messages, memories, etc.)
  const activityEvents = await requestEvents([{ limit: 500 }]);
  const activityMap = new Map<string, number>(); // pubkey -> latest timestamp
  for (const e of activityEvents) {
    const existing = activityMap.get(e.pubkey) || 0;
    if (e.created_at > existing) {
      activityMap.set(e.pubkey, e.created_at);
    }
  }

  // Step 3: Merge — all unique pubkeys, with profile if available
  const allPubkeys = new Set([...profileMap.keys(), ...activityMap.keys()]);
  const results: { pubkey: string; meta: Record<string, any>; created_at: number; hasProfile: boolean }[] = [];

  for (const pubkey of allPubkeys) {
    const profile = profileMap.get(pubkey);
    results.push({
      pubkey,
      meta: profile?.meta || {},
      created_at: profile?.created_at || activityMap.get(pubkey) || 0,
      hasProfile: !!profile,
    });
  }

  return results;
}

// Keep old name as alias for backward compatibility
export const listProfiles = listMembers;

// ── Single profile fetch from this relay ─────────────────────────

export async function fetchProfileFromRelay(pubkey: string): Promise<Record<string, any> | null> {
  const events = await requestEvents([{ kinds: [0], authors: [pubkey], limit: 1 }]);
  if (events.length === 0) return null;
  try {
    return JSON.parse(events[0].content);
  } catch {
    return null;
  }
}

// ── Group operations ─────────────────────────────────────────────

export async function listGroups(): Promise<Group[]> {
  const events = await requestEvents([{ kinds: [39000], limit: 100 }]);
  return events.map(e => {
    const getTag = (name: string) => e.tags.find(t => t[0] === name)?.[1];
    return {
      id: getTag('d') || e.id,
      name: getTag('name') || getTag('d') || 'unnamed',
      about: getTag('about') || '',
      picture: getTag('picture') || '',
      members: e.tags.filter(t => t[0] === 'p').map(t => t[1]),
      memberCount: e.tags.filter(t => t[0] === 'p').length,
    };
  });
}

export async function getGroupMessages(groupId: string, limit = 100): Promise<Message[]> {
  const events = await requestEvents([{ kinds: [9], '#h': [groupId], limit }]);
  return events
    .map(e => ({
      id: e.id,
      content: e.content,
      sender: e.pubkey,
      channel: groupId,
      created_at: new Date(e.created_at * 1000).toISOString(),
      consolidated: false,
    }))
    .sort((a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime());
}

// ── App data (NIP-78) ────────────────────────────────────────────

export async function fetchAppData(pubkey: string, dTag: string): Promise<NostrEvent | null> {
  const events = await requestEvents([{ kinds: [30078], authors: [pubkey], '#d': [dTag], limit: 1 }]);
  return events[0] || null;
}

export async function publishAppData(dTag: string, content: string, signer: Signer): Promise<string> {
  const event: EventTemplate = {
    kind: 30078,
    created_at: Math.floor(Date.now() / 1000),
    tags: [['d', dTag]],
    content,
  };

  const signed = await signer.signEvent(event);
  return publishEvent(signed as NostrEvent);
}

// ── Forward pre-signed event ─────────────────────────────────────

export async function forwardEvent(event: NostrEvent): Promise<string> {
  return publishEvent(event);
}

// ── Legacy NomenRelay wrapper ────────────────────────────────────
// Maintains the same interface so page components don't need changes.

export class NomenRelay {
  private _url: string;
  onConnectionChange?: (connected: boolean) => void;
  private connSub: RxSubscription | null = null;

  constructor(url: string = 'wss://zooid.atlantislabs.space') {
    this._url = url;
    setRelayUrl(url);

    // Bridge connection status to callback
    this.connSub = getConnectionStatus().subscribe(connected => {
      this.onConnectionChange?.(connected);
    });
  }

  get url(): string { return this._url; }

  async connect(): Promise<void> {
    // applesauce-relay connects lazily on first request/subscription.
    // Nothing to do here — the first request() call will trigger connection.
  }

  async authenticate(signer: Signer): Promise<void> {
    setSigner(signer);
    // Don't wait here — applesauce-relay's request()/subscription() have
    // built-in waitForAuth that pauses until authenticated$ is true.
    // The auto-auth via challenge$ subscription handles the actual AUTH flow.
  }

  async listMemories(pubkey?: string): Promise<Memory[]> { return listMemories(pubkey); }
  subscribeMemories(pubkey: string | undefined, callback: (m: Memory) => void): Subscription { return subscribeMemories(pubkey, callback); }
  async storeMemory(topic: string, summary: string, detail: string, tier: string, signer: Signer): Promise<string> { return storeMemory(topic, summary, detail, tier, signer); }
  async deleteMemory(eventId: string, signer: Signer, dTag?: string): Promise<void> { return deleteMemory(eventId, signer, dTag); }
  async listProfiles(): Promise<{ pubkey: string; meta: Record<string, any>; created_at: number; hasProfile: boolean }[]> { return listMembers(); }
  async listGroups(): Promise<Group[]> { return listGroups(); }
  async getGroupMessages(groupId: string, limit?: number): Promise<Message[]> { return getGroupMessages(groupId, limit); }
  async fetchAppData(pubkey: string, dTag: string): Promise<NostrEvent | null> { return fetchAppData(pubkey, dTag); }
  async publishAppData(dTag: string, content: string, signer: Signer): Promise<string> { return publishAppData(dTag, content, signer); }
  async publishEvent(event: NostrEvent): Promise<string> { return forwardEvent(event); }

  disconnect() {
    this.connSub?.unsubscribe();
  }
}
