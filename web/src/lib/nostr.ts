// Nostr authentication using applesauce-signers + profile fetching

import { nip19 } from 'nostr-tools';
import type { EventTemplate, VerifiedEvent, NostrEvent, Filter } from 'nostr-tools';
import { ExtensionSigner } from 'applesauce-signers';
import { NostrConnectSigner, type NostrConnectAppMetadata } from 'applesauce-signers/signers/nostr-connect-signer';
import { SimpleSigner } from 'applesauce-signers/signers/simple-signer';

export interface NostrProfile {
  pubkey: string;
  npub: string;
  npubShort: string;
  name?: string;
  displayName?: string;
  picture?: string;
  about?: string;
}

export interface NostrSigner {
  getPublicKey(): Promise<string>;
  signEvent(event: EventTemplate): Promise<VerifiedEvent>;
  close?(): Promise<void>;
}

// Check if NIP-07 extension is available
export function hasNip07(): boolean {
  return typeof window !== 'undefined' && !!(window as any).nostr;
}

// Compress npub: first 14 + last 4 chars
export function compressNpub(npub: string): string {
  if (npub.length <= 18) return npub;
  return `${npub.slice(0, 14)}...${npub.slice(-4)}`;
}

// Login with NIP-07 web extension (using applesauce ExtensionSigner)
export async function loginWithNip07(): Promise<{ profile: NostrProfile; signer: NostrSigner }> {
  const extSigner = new ExtensionSigner();
  const pubkey = await extSigner.getPublicKey();
  const npub = nip19.npubEncode(pubkey);

  const signer: NostrSigner = {
    getPublicKey: () => extSigner.getPublicKey(),
    signEvent: (event: EventTemplate) => extSigner.signEvent(event),
  };

  const profile: NostrProfile = {
    pubkey,
    npub,
    npubShort: compressNpub(npub),
  };

  try {
    const meta = await fetchProfileMetadata(pubkey);
    if (meta) {
      profile.name = meta.name;
      profile.displayName = meta.display_name || meta.displayName;
      profile.picture = meta.picture;
      profile.about = meta.about;
    }
  } catch {
    // Profile fetch is optional
  }

  return { profile, signer };
}

// NIP-46 Nostr Connect session
export interface NostrConnectSession {
  uri: string;
  connectSigner: NostrConnectSigner;
  relay: string;
}

// Simple WebSocket-based subscription method for NostrConnectSigner
function createSubscriptionMethod(relays: string[]) {
  return (subRelays: string[], filters: Filter[]) => {
    const targetRelays = subRelays.length > 0 ? subRelays : relays;
    return {
      subscribe: (observer: { next?: (e: NostrEvent) => void; error?: (err: any) => void; complete?: () => void }) => {
        const connections: WebSocket[] = [];
        for (const url of targetRelays) {
          try {
            const ws = new WebSocket(url);
            connections.push(ws);
            ws.onopen = () => {
              for (const filter of filters) {
                const subId = crypto.randomUUID().slice(0, 8);
                ws.send(JSON.stringify(['REQ', subId, filter]));
              }
            };
            ws.onmessage = (evt) => {
              try {
                const data = JSON.parse(evt.data);
                if (data[0] === 'EVENT' && data[2]) {
                  observer.next?.(data[2] as NostrEvent);
                }
              } catch { /* ignore */ }
            };
            ws.onerror = () => observer.error?.(new Error(`WebSocket error: ${url}`));
          } catch (err) {
            observer.error?.(err);
          }
        }
        return {
          unsubscribe: () => {
            for (const ws of connections) {
              try { ws.close(); } catch { /* ignore */ }
            }
          },
        };
      },
    };
  };
}

// Simple WebSocket-based publish method for NostrConnectSigner
function createPublishMethod(relays: string[]) {
  return async (pubRelays: string[], event: NostrEvent) => {
    const targetRelays = pubRelays.length > 0 ? pubRelays : relays;
    for (const url of targetRelays) {
      try {
        const ws = new WebSocket(url);
        await new Promise<void>((resolve, reject) => {
          const timeout = setTimeout(() => { ws.close(); reject(new Error('timeout')); }, 5000);
          ws.onopen = () => {
            ws.send(JSON.stringify(['EVENT', event]));
            clearTimeout(timeout);
            setTimeout(() => { ws.close(); resolve(); }, 500);
          };
          ws.onerror = () => { clearTimeout(timeout); reject(new Error('error')); };
        });
      } catch { /* try next relay */ }
    }
  };
}

// Generate a Nostr Connect session (using applesauce NostrConnectSigner)
export function createNostrConnectSession(relay: string): NostrConnectSession {
  const clientSigner = new SimpleSigner();
  const relays = [relay];

  const connectSigner = new NostrConnectSigner({
    relays,
    signer: clientSigner,
    subscriptionMethod: createSubscriptionMethod(relays),
    publishMethod: createPublishMethod(relays),
  });

  const metadata: NostrConnectAppMetadata = {
    name: 'Nomen',
    url: window.location.origin,
  };

  const uri = connectSigner.getNostrConnectURI(metadata);

  return { uri, connectSigner, relay };
}

// Wait for remote signer to connect via NIP-46
export async function waitForNostrConnect(
  session: NostrConnectSession,
): Promise<{ profile: NostrProfile; signer: NostrSigner }> {
  await session.connectSigner.open();
  await session.connectSigner.waitForSigner();

  const pubkey = await session.connectSigner.getPublicKey();
  const npub = nip19.npubEncode(pubkey);

  const profile: NostrProfile = {
    pubkey,
    npub,
    npubShort: compressNpub(npub),
  };

  const signer: NostrSigner = {
    getPublicKey: () => session.connectSigner.getPublicKey(),
    signEvent: (event: EventTemplate) => session.connectSigner.signEvent(event),
    close: () => session.connectSigner.close(),
  };

  // Store session info for reconnection
  sessionStorage.setItem(
    'nomen:nip46',
    JSON.stringify({
      clientKey: Array.from(session.connectSigner.signer.key),
      relay: session.relay,
      remotePubkey: session.connectSigner.remote,
    }),
  );

  try {
    const meta = await fetchProfileMetadata(pubkey);
    if (meta) {
      profile.name = meta.name;
      profile.displayName = meta.display_name || meta.displayName;
      profile.picture = meta.picture;
      profile.about = meta.about;
    }
  } catch {
    // Profile fetch is optional
  }

  return { profile, signer };
}

// Restore a NIP-46 session from sessionStorage
export async function restoreNip46Session(): Promise<{ profile: NostrProfile; signer: NostrSigner } | null> {
  const stored = sessionStorage.getItem('nomen:nip46');
  if (!stored) return null;

  try {
    const { clientKey, relay, remotePubkey } = JSON.parse(stored);
    const clientSigner = new SimpleSigner(new Uint8Array(clientKey));
    const relays = [relay];

    const connectSigner = new NostrConnectSigner({
      relays,
      signer: clientSigner,
      remote: remotePubkey,
      subscriptionMethod: createSubscriptionMethod(relays),
      publishMethod: createPublishMethod(relays),
    });

    await connectSigner.open();
    await connectSigner.requireConnection();

    const pubkey = await connectSigner.getPublicKey();
    const npub = nip19.npubEncode(pubkey);

    const profile: NostrProfile = {
      pubkey,
      npub,
      npubShort: compressNpub(npub),
    };

    const signer: NostrSigner = {
      getPublicKey: () => connectSigner.getPublicKey(),
      signEvent: (event: EventTemplate) => connectSigner.signEvent(event),
      close: () => connectSigner.close(),
    };

    try {
      const meta = await fetchProfileMetadata(pubkey);
      if (meta) {
        profile.name = meta.name;
        profile.displayName = meta.display_name || meta.displayName;
        profile.picture = meta.picture;
        profile.about = meta.about;
      }
    } catch {
      // optional
    }

    return { profile, signer };
  } catch {
    sessionStorage.removeItem('nomen:nip46');
    return null;
  }
}

// ── Profile cache ──────────────────────────────────────────────
const PROFILE_CACHE_KEY = 'nomen:profileCache';
const PROFILE_CACHE_TTL = 60 * 60 * 1000; // 1 hour

interface CachedProfile {
  meta: Record<string, any>;
  ts: number;
}

const profileMemCache = new Map<string, CachedProfile>();

function loadProfileCache(): Map<string, CachedProfile> {
  if (profileMemCache.size > 0) return profileMemCache;
  try {
    const raw = localStorage.getItem(PROFILE_CACHE_KEY);
    if (raw) {
      const entries = JSON.parse(raw) as Record<string, CachedProfile>;
      const now = Date.now();
      for (const [k, v] of Object.entries(entries)) {
        if (now - v.ts < PROFILE_CACHE_TTL) {
          profileMemCache.set(k, v);
        }
      }
    }
  } catch { /* ignore corrupt cache */ }
  return profileMemCache;
}

function saveProfileCache() {
  const obj: Record<string, CachedProfile> = {};
  for (const [k, v] of profileMemCache) {
    obj[k] = v;
  }
  try {
    localStorage.setItem(PROFILE_CACHE_KEY, JSON.stringify(obj));
  } catch { /* storage full, ignore */ }
}

function getCachedProfile(pubkey: string): Record<string, any> | undefined {
  const cache = loadProfileCache();
  const entry = cache.get(pubkey);
  if (entry && Date.now() - entry.ts < PROFILE_CACHE_TTL) {
    return entry.meta;
  }
  return undefined;
}

function setCachedProfile(pubkey: string, meta: Record<string, any>) {
  profileMemCache.set(pubkey, { meta, ts: Date.now() });
  saveProfileCache();
}

// Fetch the raw kind 0 event for a single pubkey from public relays
export async function fetchProfileEvent(pubkey: string): Promise<any | null> {
  for (const url of PUBLIC_PROFILE_RELAYS) {
    try {
      const fetched = await fetchKind0EventsFromRelay(url, [pubkey]);
      const event = fetched.get(pubkey);
      if (event) return event;
    } catch {
      continue;
    }
  }
  return null;
}

export const PUBLIC_PROFILE_RELAYS = [
  'wss://relay.damus.io',
  'wss://relay.nostr.band',
  'wss://nos.lol',
  'wss://purplepag.es',
];

// Fetch kind 0 metadata from well-known relays (single pubkey, cached)
export async function fetchProfileMetadata(pubkey: string): Promise<any> {
  const cached = getCachedProfile(pubkey);
  if (cached) return cached;

  for (const url of PUBLIC_PROFILE_RELAYS) {
    try {
      const ws = new WebSocket(url);
      const result = await new Promise<any>((resolve, reject) => {
        const timeout = setTimeout(() => {
          ws.close();
          reject(new Error('timeout'));
        }, 5000);

        ws.onopen = () => {
          const subId = crypto.randomUUID().slice(0, 8);
          ws.send(JSON.stringify(['REQ', subId, { kinds: [0], authors: [pubkey], limit: 1 }]));
        };

        ws.onmessage = (evt) => {
          const data = JSON.parse(evt.data);
          if (data[0] === 'EVENT') {
            clearTimeout(timeout);
            ws.close();
            try {
              resolve(JSON.parse(data[2].content));
            } catch {
              resolve(null);
            }
          } else if (data[0] === 'EOSE') {
            clearTimeout(timeout);
            ws.close();
            resolve(null);
          }
        };

        ws.onerror = () => {
          clearTimeout(timeout);
          reject(new Error('WebSocket error'));
        };
      });

      if (result) {
        setCachedProfile(pubkey, result);
        return result;
      }
    } catch {
      continue;
    }
  }

  return null;
}

// Batch fetch kind 0 profiles for multiple pubkeys from public relays
export async function fetchProfilesBatch(
  pubkeys: string[],
): Promise<Map<string, Record<string, any>>> {
  const results = new Map<string, Record<string, any>>();
  const uncached: string[] = [];

  for (const pk of pubkeys) {
    const cached = getCachedProfile(pk);
    if (cached) {
      results.set(pk, cached);
    } else {
      uncached.push(pk);
    }
  }

  if (uncached.length === 0) return results;

  for (const url of PUBLIC_PROFILE_RELAYS) {
    if (uncached.length === 0) break;
    const remaining = uncached.filter((pk) => !results.has(pk));
    if (remaining.length === 0) break;

    try {
      const fetched = await fetchKind0FromRelay(url, remaining);
      for (const [pk, meta] of fetched) {
        results.set(pk, meta);
        setCachedProfile(pk, meta);
      }
    } catch {
      continue;
    }
  }

  return results;
}

function fetchKind0FromRelay(
  url: string,
  pubkeys: string[],
): Promise<Map<string, Record<string, any>>> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    const results = new Map<string, Record<string, any>>();
    const timestamps = new Map<string, number>();
    const timeout = setTimeout(() => {
      ws.close();
      resolve(results);
    }, 8000);

    ws.onopen = () => {
      const subId = crypto.randomUUID().slice(0, 8);
      ws.send(JSON.stringify(['REQ', subId, { kinds: [0], authors: pubkeys, limit: pubkeys.length }]));
    };

    ws.onmessage = (evt) => {
      try {
        const data = JSON.parse(evt.data);
        if (data[0] === 'EVENT') {
          const event = data[2];
          const existing = timestamps.get(event.pubkey) || 0;
          if (!results.has(event.pubkey) || event.created_at > existing) {
            try {
              results.set(event.pubkey, JSON.parse(event.content));
              timestamps.set(event.pubkey, event.created_at);
            } catch { /* skip malformed */ }
          }
        } else if (data[0] === 'EOSE') {
          clearTimeout(timeout);
          ws.close();
          resolve(results);
        }
      } catch { /* ignore parse errors */ }
    };

    ws.onerror = () => {
      clearTimeout(timeout);
      reject(new Error('WebSocket error'));
    };
  });
}

// Fetch raw kind 0 events (full NostrEvent objects) from a public relay
export function fetchKind0EventsFromRelay(
  url: string,
  pubkeys: string[],
): Promise<Map<string, any>> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    const results = new Map<string, any>();
    const timeout = setTimeout(() => {
      ws.close();
      resolve(results);
    }, 8000);

    ws.onopen = () => {
      const subId = crypto.randomUUID().slice(0, 8);
      ws.send(JSON.stringify(['REQ', subId, { kinds: [0], authors: pubkeys, limit: pubkeys.length }]));
    };

    ws.onmessage = (evt) => {
      try {
        const data = JSON.parse(evt.data);
        if (data[0] === 'EVENT') {
          const event = data[2];
          const existing = results.get(event.pubkey);
          if (!existing || event.created_at > existing.created_at) {
            results.set(event.pubkey, event);
          }
        } else if (data[0] === 'EOSE') {
          clearTimeout(timeout);
          ws.close();
          resolve(results);
        }
      } catch { /* ignore parse errors */ }
    };

    ws.onerror = () => {
      clearTimeout(timeout);
      reject(new Error('WebSocket error'));
    };
  });
}

// Fetch raw kind 0 events from public relays for given pubkeys
export async function fetchProfileEventsBatch(
  pubkeys: string[],
): Promise<Map<string, any>> {
  const results = new Map<string, any>();
  const remaining = new Set(pubkeys);

  for (const url of PUBLIC_PROFILE_RELAYS) {
    if (remaining.size === 0) break;
    try {
      const fetched = await fetchKind0EventsFromRelay(url, [...remaining]);
      for (const [pk, event] of fetched) {
        results.set(pk, event);
        remaining.delete(pk);
        try {
          setCachedProfile(pk, JSON.parse(event.content));
        } catch { /* skip malformed */ }
      }
    } catch {
      continue;
    }
  }

  return results;
}
