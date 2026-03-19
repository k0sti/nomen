<script lang="ts">
  import TierBadge from '../components/TierBadge.svelte';
  import { loading, profile, ensureConnected, showError } from '../lib/stores';
  import { requestRawEvents } from '../lib/relay';
  import { parseDTag, normalizeVisibility, isV2DTag } from '../lib/dtag';
  import { nip19 } from 'nostr-tools';
  import type { NostrEvent } from 'nostr-tools';

  // Event kinds we care about
  const KIND_MEMORY = 31234;
  const KIND_RAW_SOURCE = 1235;
  const KIND_LABELS: Record<number, string> = {
    31234: 'Memory',
    1235: 'Raw Source',
    31235: 'Lesson',
    1234: 'Ephemeral',
    30078: 'App Data',
  };

  let events = $state<NostrEvent[]>([]);
  let kindFilter = $state<number | null>(null);
  let filterText = $state('');
  let expandedEvent = $state<string | null>(null);
  let loaded = $state(false);

  const filteredEvents = $derived(
    events.filter((e) => {
      if (kindFilter !== null && e.kind !== kindFilter) return false;
      if (filterText) {
        const q = filterText.toLowerCase();
        const dTag = getTag(e, 'd') || '';
        const content = e.content.toLowerCase();
        if (!dTag.toLowerCase().includes(q) && !content.includes(q) && !e.id.includes(q)) return false;
      }
      return true;
    })
  );

  const kindCounts = $derived.by(() => {
    const counts: Record<number, number> = {};
    for (const e of events) {
      counts[e.kind] = (counts[e.kind] || 0) + 1;
    }
    return counts;
  });

  function getTag(e: NostrEvent, name: string): string | undefined {
    return e.tags.find(t => t[0] === name)?.[1];
  }

  function formatPubkey(hex: string): string {
    try {
      return nip19.npubEncode(hex).slice(0, 16) + '...';
    } catch {
      return hex.slice(0, 16) + '...';
    }
  }

  function formatTimestamp(ts: number): string {
    return new Date(ts * 1000).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  function tryParseContent(content: string): { parsed: boolean; display: string } {
    if (!content) return { parsed: false, display: '' };
    try {
      const obj = JSON.parse(content);
      return { parsed: true, display: JSON.stringify(obj, null, 2) };
    } catch {
      // Could be encrypted or plain text
      return { parsed: false, display: content };
    }
  }

  async function loadEvents() {
    loading.set(true);
    loaded = false;
    try {
      await ensureConnected();

      const [memoryEvents, rawSourceEvents, otherEvents] = await Promise.all([
        requestRawEvents([{ kinds: [KIND_MEMORY], limit: 200 }]),
        requestRawEvents([{ kinds: [KIND_RAW_SOURCE], limit: 200 }]),
        requestRawEvents([{ kinds: [31235, 30078], limit: 100 }]),
      ]);

      events = [...memoryEvents, ...rawSourceEvents, ...otherEvents]
        .sort((a, b) => b.created_at - a.created_at);
      loaded = true;
    } catch (err: any) {
      showError('Failed to load events: ' + (err.message || err));
    } finally {
      loading.set(false);
    }
  }

  function toggleExpand(id: string) {
    expandedEvent = expandedEvent === id ? null : id;
  }

  $effect(() => {
    if ($profile && !loaded) loadEvents();
  });
</script>

<div class="max-w-5xl mx-auto space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-2xl font-bold text-gray-100">Event Explorer</h2>
      <p class="text-sm text-gray-500 mt-1">Raw Nostr events from the relay — {events.length} events loaded</p>
    </div>
    <button
      onclick={loadEvents}
      class="px-4 py-2 min-h-11 rounded-lg border border-gray-700 bg-gray-800/50 hover:bg-gray-700 text-gray-300 text-sm font-medium transition-colors duration-150"
    >
      Refresh
    </button>
  </div>

  <!-- Filters -->
  <div class="flex items-center gap-3">
    <input
      type="text"
      placeholder="Filter by d-tag, content, or event ID..."
      bind:value={filterText}
      class="flex-1 px-4 py-2.5 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 focus:border-accent-500"
    />
    <div class="flex gap-1.5">
      {#each Object.entries(kindCounts).sort((a, b) => Number(a[0]) - Number(b[0])) as [kind, count]}
        {@const k = Number(kind)}
        <button
          onclick={() => kindFilter = kindFilter === k ? null : k}
          class="px-3 py-2 min-h-11 rounded-md text-xs font-medium border transition-colors duration-150
            {kindFilter === k
              ? 'border-accent-500 bg-accent-500/20 text-accent-400'
              : 'border-gray-700 bg-gray-800/50 text-gray-400 hover:text-gray-200'}"
        >
          {KIND_LABELS[k] || `Kind ${k}`} ({count})
        </button>
      {/each}
    </div>
  </div>

  {#if !$profile}
    <div class="text-center py-12 text-gray-500">Login to browse relay events</div>
  {:else if $loading}
    <div class="space-y-2">
      {#each { length: 5 } as _}
        <div class="border border-gray-800 rounded-lg p-4 bg-gray-900/50">
          <div class="skeleton h-4 w-full max-w-lg"></div>
          <div class="skeleton h-3 w-48 mt-2"></div>
        </div>
      {/each}
    </div>
  {:else if filteredEvents.length === 0}
    <div class="text-center py-12 text-gray-500">
      {events.length === 0 ? 'No events found' : 'No events match your filters'}
    </div>
  {:else}
    <div class="space-y-2">
      {#each filteredEvents as event (event.id)}
        {@const dTag = getTag(event, 'd') || ''}
        {@const parsed = parseDTag(dTag)}
        {@const isExpanded = expandedEvent === event.id}
        {@const content = tryParseContent(event.content)}
        {@const kindLabel = KIND_LABELS[event.kind] || `Kind ${event.kind}`}

        <div class="border border-gray-800 rounded-lg bg-gray-900/50 hover:border-gray-700 transition-colors duration-150">
          <button
            class="w-full p-4 min-h-14 text-left hover:bg-gray-800/30"
            onclick={() => toggleExpand(event.id)}
            aria-expanded={isExpanded}
          >
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="inline-flex items-center px-2 py-0.5 rounded text-[10px] font-bold border
                    {event.kind === KIND_MEMORY ? 'bg-accent-900/30 text-accent-400 border-accent-800/40' :
                     event.kind === KIND_RAW_SOURCE ? 'bg-orange-900/30 text-orange-400 border-orange-800/40' :
                     'bg-gray-800 text-gray-400 border-gray-700'}">
                    {kindLabel}
                  </span>
                  {#if dTag}
                    <span class="font-mono text-sm text-gray-300 truncate max-w-md">{dTag}</span>
                  {/if}
                  {#if isV2DTag(dTag)}
                    <TierBadge visibility={normalizeVisibility(parsed.visibility)} scope={parsed.scope} />
                  {/if}
                </div>
                {#if content.display}
                  <p class="text-xs text-gray-500 mt-1 line-clamp-1 font-mono">{content.display.slice(0, 120)}</p>
                {/if}
              </div>
              <div class="text-right shrink-0">
                <div class="text-xs text-gray-500">{formatTimestamp(event.created_at)}</div>
                <div class="text-[11px] text-gray-600 font-mono mt-1">{formatPubkey(event.pubkey)}</div>
              </div>
            </div>
          </button>

          {#if isExpanded}
            <div class="px-4 pb-4 border-t border-gray-800/50 pt-3 space-y-3">
              <!-- Metadata -->
              <div class="grid grid-cols-2 gap-x-4 gap-y-1.5 text-xs">
                <div class="text-gray-500">Event ID: <span class="text-gray-400 font-mono">{event.id}</span></div>
                <div class="text-gray-500">Kind: <span class="text-gray-400">{event.kind} ({kindLabel})</span></div>
                <div class="text-gray-500">Author: <span class="text-gray-400 font-mono">{event.pubkey}</span></div>
                <div class="text-gray-500">Created: <span class="text-gray-400">{formatTimestamp(event.created_at)}</span></div>
                {#if isV2DTag(dTag)}
                  <div class="text-gray-500">Visibility: <span class="text-gray-400">{normalizeVisibility(parsed.visibility)}</span></div>
                  <div class="text-gray-500">Scope: <span class="text-gray-400">{parsed.scope || '(none)'}</span></div>
                  <div class="text-gray-500">Topic: <span class="text-gray-400">{parsed.topic}</span></div>
                {/if}
              </div>

              <!-- Tags -->
              <div>
                <span class="text-xs font-medium text-gray-500 uppercase tracking-wide">Tags ({event.tags.length})</span>
                <div class="mt-1 bg-gray-950 rounded-lg p-3 overflow-x-auto">
                  <table class="text-xs font-mono w-full">
                    <tbody>
                      {#each event.tags as tag, i}
                        <tr class="border-b border-gray-800/50 last:border-0">
                          <td class="py-1 pr-3 text-gray-600 align-top">{i}</td>
                          <td class="py-1 pr-3 text-accent-400 align-top font-bold">{tag[0]}</td>
                          <td class="py-1 text-gray-300 break-all">{tag.slice(1).join(' | ')}</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
              </div>

              <!-- Content -->
              <div>
                <span class="text-xs font-medium text-gray-500 uppercase tracking-wide">Content</span>
                <pre class="mt-1 bg-gray-950 rounded-lg p-3 text-xs text-gray-300 overflow-x-auto max-h-64 overflow-y-auto whitespace-pre-wrap break-all">{content.display || '(empty)'}</pre>
              </div>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
