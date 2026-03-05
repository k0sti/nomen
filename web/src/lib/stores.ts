// Svelte stores for state management — relay-first architecture

import { writable, derived } from 'svelte/store';
import { NomenRelay } from './relay';
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
export const relay = writable<NomenRelay>(new NomenRelay());
export const api = derived(apiBaseUrl, ($url) => new NomenApi($url));

// ── Auth ──────────────────────────────────────────────────────────
export const profile = writable<NostrProfile | null>(null);
export const signer = writable<NostrSigner | null>(null);
export const isLoggedIn = derived(profile, ($p) => $p !== null);
export const showLoginModal = writable(false);
export const showProfileModal = writable(false);

// ── Navigation ───────────────────────────────────────────────────
export type Page = 'memories' | 'search' | 'messages' | 'groups' | 'settings';

const validPages: Page[] = ['memories', 'search', 'messages', 'groups', 'settings'];

function getPageFromHash(): Page {
  const hash = window.location.hash.replace('#/', '').replace('#', '');
  return validPages.includes(hash as Page) ? (hash as Page) : 'memories';
}

export const currentPage = writable<Page>(getPageFromHash());

// Sync hash <-> store
if (typeof window !== 'undefined') {
  window.addEventListener('hashchange', () => {
    currentPage.set(getPageFromHash());
  });

  currentPage.subscribe((page) => {
    const target = `#/${page}`;
    if (window.location.hash !== target) {
      window.location.hash = target;
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

// ── Relay connection state ───────────────────────────────────────
export const relayConnected = writable(false);

// ── Signer helper ────────────────────────────────────────────────
// Returns the current signer (NIP-07 or NIP-46)
export function getSigner(): NostrSigner {
  let current: NostrSigner | null = null;
  signer.subscribe((s) => (current = s))();
  if (!current) throw new Error('Not logged in');
  return current;
}
