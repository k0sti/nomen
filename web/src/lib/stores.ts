// Svelte stores for state management

import { writable, derived } from 'svelte/store';
import { NomenClient } from './api';
import type { NostrProfile } from './nostr';
import type { Memory, Message, Group, Entity, SearchResult } from './api';

// ── Settings ──────────────────────────────────────────────────────
export const apiBaseUrl = writable(localStorage.getItem('nomen:apiBaseUrl') || 'http://localhost:3000');
export const relayUrl = writable(localStorage.getItem('nomen:relayUrl') || 'wss://zooid.atlantislabs.space');
export const embeddingProvider = writable(localStorage.getItem('nomen:embeddingProvider') || 'openai');
export const defaultChannel = writable(localStorage.getItem('nomen:defaultChannel') || 'nostr');

// Persist settings
apiBaseUrl.subscribe((v) => localStorage.setItem('nomen:apiBaseUrl', v));
relayUrl.subscribe((v) => localStorage.setItem('nomen:relayUrl', v));
embeddingProvider.subscribe((v) => localStorage.setItem('nomen:embeddingProvider', v));
defaultChannel.subscribe((v) => localStorage.setItem('nomen:defaultChannel', v));

// ── API Client ────────────────────────────────────────────────────
export const client = derived(apiBaseUrl, ($url) => {
  const c = new NomenClient($url);
  // Start in mock mode — real mode activates when backend responds
  c.enableMock();
  return c;
});

// ── Auth ──────────────────────────────────────────────────────────
export const profile = writable<NostrProfile | null>(null);
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
export const mockMode = writable(true);
export const expandedMemory = writable<string | null>(null);
