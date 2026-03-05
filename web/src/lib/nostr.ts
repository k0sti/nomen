// Nostr authentication using applesauce-signers + nostr-tools

import { nip19 } from 'nostr-tools';

export interface NostrProfile {
  pubkey: string;
  npub: string;
  npubShort: string;
  name?: string;
  displayName?: string;
  picture?: string;
  about?: string;
}

export type SignerType = 'nip07' | 'nostr-connect' | 'amber';

// Check if NIP-07 extension is available
export function hasNip07(): boolean {
  return typeof window !== 'undefined' && !!(window as any).nostr;
}

// Check if Amber is available (clipboard-based signer on Android)
export function hasAmber(): boolean {
  return typeof window !== 'undefined' && !!(window as any).amber;
}

// Compress npub: first 10 + last 4 chars
export function compressNpub(npub: string): string {
  if (npub.length <= 18) return npub;
  return `${npub.slice(0, 14)}...${npub.slice(-4)}`;
}

// Login with NIP-07 web extension
export async function loginWithNip07(): Promise<NostrProfile> {
  const ext = (window as any).nostr;
  if (!ext) throw new Error('No NIP-07 extension found');

  const pubkey = await ext.getPublicKey();
  const npub = nip19.npubEncode(pubkey);

  const profile: NostrProfile = {
    pubkey,
    npub,
    npubShort: compressNpub(npub),
  };

  // Try to fetch profile metadata (kind 0)
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

  return profile;
}

// Fetch kind 0 metadata from well-known relays
async function fetchProfileMetadata(pubkey: string): Promise<any> {
  const relays = [
    'wss://relay.damus.io',
    'wss://relay.nostr.band',
    'wss://nos.lol',
  ];

  for (const url of relays) {
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

      if (result) return result;
    } catch {
      continue;
    }
  }

  return null;
}

// Generate Nostr Connect URI for QR code
export function generateNostrConnectUri(
  relay: string = 'wss://relay.nsec.app'
): { uri: string; secret: string } {
  const secret = Array.from(crypto.getRandomValues(new Uint8Array(32)))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');

  const uri = `nostrconnect://${secret}?relay=${encodeURIComponent(relay)}`;
  return { uri, secret };
}
