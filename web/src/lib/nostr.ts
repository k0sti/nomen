// Nostr authentication: NIP-07 extension + NIP-46 remote signing (Nostr Connect / Amber)

import { nip19 } from 'nostr-tools';
import { generateSecretKey, getPublicKey } from 'nostr-tools/pure';
import { BunkerSigner, createNostrConnectURI } from 'nostr-tools/nip46';
import type { EventTemplate, VerifiedEvent } from 'nostr-tools/core';

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

// Create a NIP-07 signer wrapper
export function createNip07Signer(): NostrSigner {
  const ext = (window as any).nostr;
  if (!ext) throw new Error('No NIP-07 extension found');
  return {
    getPublicKey: () => ext.getPublicKey(),
    signEvent: (event: EventTemplate) => ext.signEvent(event),
  };
}

// Login with NIP-07 web extension
export async function loginWithNip07(): Promise<{ profile: NostrProfile; signer: NostrSigner }> {
  const signer = createNip07Signer();
  const pubkey = await signer.getPublicKey();
  const npub = nip19.npubEncode(pubkey);

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
  secret: string;
  clientSecretKey: Uint8Array;
  relay: string;
}

// Generate a Nostr Connect session (URI + ephemeral keypair)
export function createNostrConnectSession(relay: string): NostrConnectSession {
  const clientSecretKey = generateSecretKey();
  const clientPubkey = getPublicKey(clientSecretKey);
  const secret = Array.from(crypto.getRandomValues(new Uint8Array(16)))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');

  const uri = createNostrConnectURI({
    clientPubkey,
    relays: [relay],
    secret,
    name: 'Nomen',
  });

  return { uri, secret, clientSecretKey, relay };
}

// Wait for remote signer to connect via NIP-46
export async function waitForNostrConnect(
  session: NostrConnectSession,
  abortSignal?: AbortSignal,
): Promise<{ profile: NostrProfile; signer: NostrSigner }> {
  const bunkerSigner = await BunkerSigner.fromURI(
    session.clientSecretKey,
    session.uri,
    undefined,
    abortSignal ?? 120_000, // 2 minute timeout
  );

  const pubkey = await bunkerSigner.getPublicKey();
  const npub = nip19.npubEncode(pubkey);

  const profile: NostrProfile = {
    pubkey,
    npub,
    npubShort: compressNpub(npub),
  };

  const signer: NostrSigner = {
    getPublicKey: () => bunkerSigner.getPublicKey(),
    signEvent: (event: EventTemplate) => bunkerSigner.signEvent(event),
    close: () => bunkerSigner.close(),
  };

  // Store session info for reconnection
  sessionStorage.setItem(
    'nomen:nip46',
    JSON.stringify({
      clientSecretKey: Array.from(session.clientSecretKey),
      relay: session.relay,
      remotePubkey: pubkey,
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
    const { clientSecretKey, relay, remotePubkey } = JSON.parse(stored);
    const sk = new Uint8Array(clientSecretKey);

    const bunkerSigner = BunkerSigner.fromBunker(sk, {
      pubkey: remotePubkey,
      relays: [relay],
      secret: null,
    });

    // Verify connection works
    await bunkerSigner.ping();

    const pubkey = await bunkerSigner.getPublicKey();
    const npub = nip19.npubEncode(pubkey);

    const profile: NostrProfile = {
      pubkey,
      npub,
      npubShort: compressNpub(npub),
    };

    const signer: NostrSigner = {
      getPublicKey: () => bunkerSigner.getPublicKey(),
      signEvent: (event: EventTemplate) => bunkerSigner.signEvent(event),
      close: () => bunkerSigner.close(),
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

  // Check cache first
  for (const pk of pubkeys) {
    const cached = getCachedProfile(pk);
    if (cached) {
      results.set(pk, cached);
    } else {
      uncached.push(pk);
    }
  }

  if (uncached.length === 0) return results;

  // Try each public relay until we've resolved all pubkeys
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
    const timeout = setTimeout(() => {
      ws.close();
      resolve(results); // return whatever we got
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
          // Keep latest per pubkey
          if (!results.has(event.pubkey)) {
            try {
              results.set(event.pubkey, JSON.parse(event.content));
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
