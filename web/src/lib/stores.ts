// Svelte stores for state management — relay-first architecture

import { writable, derived, get } from 'svelte/store';
import { NomenRelay, relay as applesauceRelay, getConnectionStatus, ensureAuthenticated } from './relay';
import { NomenApi } from './api';
import type { NostrProfile, NostrSigner } from './nostr';
import type { Memory, Message, Group, Entity, SearchResult } from './api';

// ── Settings ──────────────────────────────────────────────────────
export const relayUrl = writable(localStorage.getItem('nomen:relayUrl') || 'wss://zooid.atlantislabs.space');
export const apiBaseUrl = writable(localStorage.getItem('nomen:apiBaseUrl') || '/memory/api');

// Persist settings
relayUrl.subscribe((v) => localStorage.setItem('nomen:relayUrl', v));
apiBaseUrl.subscribe((v) => localStorage.setItem('nomen:apiBaseUrl', v));

// ── Relay & API instances ────────────────────────────────────────
const nomenRelayWrapper = new NomenRelay();
nomenRelayWrapper.onConnectionChange = (connected) => relayConnected.set(connected);
export const relay = writable<NomenRelay>(nomenRelayWrapper);
export const api = derived(apiBaseUrl, ($url) => new NomenApi($url));

// ── Auth ──────────────────────────────────────────────────────────
export const profile = writable<NostrProfile | null>(null);
export const signer = writable<NostrSigner | null>(null);
export const isLoggedIn = derived(profile, ($p) => $p !== null);
export const showLoginModal = writable(false);
export const showProfileModal = writable(false);

// ── Auto-restore login session ──────────────────────────────────
import { loginWithNip07, restoreNip46Session, fetchProfileEvent } from './nostr';

if (typeof window !== 'undefined') {
  const savedMethod = localStorage.getItem('nomen_login_method');
  if (savedMethod === 'nip07') {
    loginWithNip07()
      .then((result) => {
        profile.set(result.profile);
        signer.set(result.signer);
        mirrorProfileToZooid(result.profile.pubkey, result.signer);
      })
      .catch(() => {
        localStorage.removeItem('nomen_login_method');
      });
  } else if (savedMethod === 'nip46') {
    restoreNip46Session()
      .then((result) => {
        if (result) {
          profile.set(result.profile);
          signer.set(result.signer);
          mirrorProfileToZooid(result.profile.pubkey, result.signer);
        } else {
          localStorage.removeItem('nomen_login_method');
          localStorage.removeItem('nomen_nip46_relay');
        }
      })
      .catch(() => {
        localStorage.removeItem('nomen_login_method');
        localStorage.removeItem('nomen_nip46_relay');
      });
  }
}

// ── Centralized relay connection helper ──────────────────────────
// Ensures relay is connected and authenticated. Idempotent — safe to call from every page.
export async function ensureConnected(): Promise<NomenRelay> {
  const r = get(relay);
  const s = get(signer);

  // Wait for connection
  await r.connect();

  // Authenticate if signer is available
  if (s) {
    // Convert NostrSigner to AuthSigner format
    const authSigner = {
      getPublicKey: () => s.getPublicKey(),
      signEvent: (event: any) => s.signEvent(event),
    };
    await ensureAuthenticated(authSigner);
  }

  relayConnected.set(true);
  return r;
}

// ── Mirror profile to zooid relay ────────────────────────────────
async function mirrorProfileToZooid(pubkey: string, signerInstance: import('./nostr').NostrSigner) {
  try {
    const event = await fetchProfileEvent(pubkey);
    if (!event) return;
    const relayInstance = get(relay);
    await relayInstance.connect();

    // Convert NostrSigner to AuthSigner format
    const authSigner = {
      getPublicKey: () => signerInstance.getPublicKey(),
      signEvent: (event: any) => signerInstance.signEvent(event),
    };
    await ensureAuthenticated(authSigner);
    await relayInstance.publishEvent(event);
    relayConnected.set(true);
  } catch {
    // Best-effort — don't block login
  }
}

// ── Navigation ───────────────────────────────────────────────────
export type Page = 'landing' | 'memories' | 'search' | 'messages' | 'members' | 'groups' | 'agents' | 'settings';

const validPages: Page[] = ['memories', 'search', 'messages', 'members', 'groups', 'agents', 'settings'];

function getPageFromHash(): Page {
  const hash = window.location.hash.replace('#/', '').replace('#', '');
  if (!hash) {
    // No hash — default to memories page
    return 'memories';
  }
  return validPages.includes(hash as Page) ? (hash as Page) : 'memories';
}

export const currentPage = writable<Page>(getPageFromHash());

// Sync hash <-> store
if (typeof window !== 'undefined') {
  window.addEventListener('hashchange', () => {
    currentPage.set(getPageFromHash());
  });

  currentPage.subscribe((page) => {
    if (page === 'landing') {
      if (window.location.hash && window.location.hash !== '#' && window.location.hash !== '#/') {
        history.pushState(null, '', window.location.pathname);
      }
    } else {
      const target = `#/${page}`;
      if (window.location.hash !== target) {
        window.location.hash = target;
      }
    }
  });
}

// ── Data ─────────────────────────────────────────────────────────
export const memories = writable<Memory[]>([]);
export const searchResults = writable<SearchResult[]>([]);
export const searchQuery = writable('');
export const messages = writable<Message[]>([]);
export const groups = writable<Group[]>([]);
export const entities = writable<Entity[]>([]);

// ── Filters ──────────────────────────────────────────────────────
export const tierFilter = writable<string>('');
export const sourceFilter = writable<string>('');
export const channelFilter = writable<string>('');

// ── UI State ─────────────────────────────────────────────────────
export const loading = writable(false);
export const expandedMemory = writable<string | null>(null);

// ── Error toasts ─────────────────────────────────────────────────
export interface Toast {
  id: number;
  message: string;
  type: 'error' | 'info';
}
let toastId = 0;
export const toasts = writable<Toast[]>([]);

export function showError(message: string) {
  const id = ++toastId;
  toasts.update((t) => [...t, { id, message, type: 'error' }]);
  setTimeout(() => toasts.update((t) => t.filter((x) => x.id !== id)), 5000);
}

export function showInfo(message: string) {
  const id = ++toastId;
  toasts.update((t) => [...t, { id, message, type: 'info' }]);
  setTimeout(() => toasts.update((t) => t.filter((x) => x.id !== id)), 3000);
}

// ── Relay connection state ───────────────────────────────────────
export const relayConnected = writable(false);

// ── Signer helper ────────────────────────────────────────────────
// Returns the current signer (NIP-07 or NIP-46)
export function getSigner(): NostrSigner {
  const current = get(signer);
  if (!current) throw new Error('Not logged in');
  return current;
}
