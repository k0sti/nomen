<script lang="ts">
  import MemoryCard from '../components/MemoryCard.svelte';
  import { api, memories, visibilityFilter, loading, showError, showInfo, profile } from '../lib/stores';
  import type { Memory, MemoryListStats } from '../lib/api';
  import { ALL_VISIBILITIES } from '../lib/dtag';

  let filterText = $state('');
  let pinnedOnly = $state(false);
  let stats = $state<MemoryListStats | null>(null);

  const filtered = $derived(
    $memories.filter((m) => {
      const matchesPinned = !pinnedOnly || m.pinned;
      const matchesVisibility = !$visibilityFilter || m.visibility === $visibilityFilter;
      const matchesText =
        !filterText ||
        m.topic.toLowerCase().includes(filterText.toLowerCase()) ||
        (m.summary || '').toLowerCase().includes(filterText.toLowerCase());
      return matchesPinned && matchesVisibility && matchesText;
    })
  );

  const visCounts = $derived.by(() => {
    const counts: Record<string, number> = {};
    for (const vis of ALL_VISIBILITIES) counts[vis] = 0;
    if (stats?.by_visibility) {
      for (const [k, v] of Object.entries(stats.by_visibility)) {
        counts[k] = (counts[k] || 0) + v;
      }
    } else {
      for (const m of $memories) {
        counts[m.visibility] = (counts[m.visibility] || 0) + 1;
      }
    }
    return counts;
  });

  const statsLine = $derived.by(() => {
    const parts: string[] = [];
    const total = stats?.total ?? $memories.length;
    parts.push(`${total} memories`);
    for (const vis of ALL_VISIBILITIES) {
      const c = visCounts[vis];
      if (c > 0) parts.push(`${c} ${vis}`);
    }
    if (stats?.pending) parts.push(`${stats.pending} pending`);
    return parts.join(' \u2014 ');
  });

  async function loadMemories() {
    loading.set(true);
    stats = null;
    try {
      const result = await $api.listMemories({ limit: 500, stats: true });
      memories.set(result.memories);
      if (result.stats) stats = result.stats;
    } catch (err: any) {
      showError('Failed to load memories: ' + (err.message || err));
    } finally {
      loading.set(false);
    }
  }

  // Track the logged-in pubkey so we reload when auth state changes
  let lastPubkey = $state<string | null>(null);

  $effect(() => {
    void $api;
    const currentPubkey = $profile?.pubkey ?? null;
    if ($memories.length === 0 || currentPubkey !== lastPubkey) {
      lastPubkey = currentPubkey;
      loadMemories();
    }
  });

  async function handleDelete(memory: Memory) {
    try {
      await $api.deleteMemory({ d_tag: memory.d_tag || undefined, id: memory.nostr_id || undefined });
      memories.update((ms) => ms.filter((m) => m.d_tag !== memory.d_tag));
      showInfo('Memory deleted');
    } catch (err: any) {
      showError('Failed to delete memory: ' + (err.message || err));
    }
  }

  async function handleTogglePin(memory: Memory) {
    try {
      if (memory.pinned) {
        await $api.unpinMemory(memory.d_tag);
      } else {
        await $api.pinMemory(memory.d_tag);
      }
      memories.update((ms) =>
        ms.map((m) => m.d_tag === memory.d_tag ? { ...m, pinned: !m.pinned } : m)
      );
      showInfo(memory.pinned ? 'Memory unpinned' : 'Memory pinned');
    } catch (err: any) {
      showError('Failed to toggle pin: ' + (err.message || err));
    }
  }

  function setVisibilityFilter(vis: string) {
    visibilityFilter.set($visibilityFilter === vis ? '' : vis);
  }

  function refresh() {
    memories.set([]);
    stats = null;
    loadMemories();
  }
</script>

<div class="max-w-4xl mx-auto space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-2xl font-bold text-gray-100">Memories</h2>
      <p class="text-sm text-gray-500 mt-1">{statsLine}</p>
    </div>
    <button
      onclick={refresh}
      class="px-4 py-2 min-h-11 rounded-lg border border-gray-700 bg-gray-800/50 hover:bg-gray-700 text-gray-300 text-sm font-medium transition-colors duration-150"
    >
      Refresh
    </button>
  </div>

  <div class="flex items-center gap-3">
    <input
      type="text"
      placeholder="Filter by topic or summary..."
      bind:value={filterText}
      class="flex-1 px-4 py-2.5 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 transition-colors duration-150 focus:border-accent-500"
    />
    <button
      onclick={() => pinnedOnly = !pinnedOnly}
      class="px-3 py-2 min-h-11 rounded-md text-xs font-medium border transition-colors duration-150
        {pinnedOnly
          ? 'border-accent-500 bg-accent-500/20 text-accent-400'
          : 'border-gray-700 bg-gray-800/50 text-gray-400 hover:text-gray-200 active:bg-gray-700'}"
    >
      📌 Pinned
    </button>
    <div class="flex gap-1.5 flex-wrap">
      {#each ALL_VISIBILITIES as vis}
        {@const count = visCounts[vis]}
        {#if count > 0}
          <button
            onclick={() => setVisibilityFilter(vis)}
            class="px-3 py-2 min-h-11 rounded-md text-xs font-medium border transition-colors duration-150
              {$visibilityFilter === vis
                ? 'border-accent-500 bg-accent-500/20 text-accent-400'
                : 'border-gray-700 bg-gray-800/50 text-gray-400 hover:text-gray-200 active:bg-gray-700'}"
          >
            {vis} ({count})
          </button>
        {/if}
      {/each}
    </div>
  </div>

  {#if $loading}
    <div class="space-y-2">
      {#each { length: 4 } as _}
        <div class="border border-gray-800 rounded-lg p-4 bg-gray-900/50">
          <div class="flex items-start justify-between gap-3">
            <div class="flex-1 space-y-2">
              <div class="flex items-center gap-2">
                <div class="skeleton h-4 w-40"></div>
                <div class="skeleton h-5 w-16"></div>
              </div>
              <div class="skeleton h-3.5 w-full max-w-md"></div>
            </div>
            <div class="space-y-1.5 text-right">
              <div class="skeleton h-3 w-24 ml-auto"></div>
              <div class="skeleton h-3 w-16 ml-auto"></div>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {:else if filtered.length === 0}
    <div class="text-center py-12 text-gray-500">
      {$memories.length === 0 ? 'No memories found in the database' : 'No memories match your filters'}
    </div>
  {:else}
    <div class="space-y-2">
      {#each filtered as memory (memory.d_tag || memory.id)}
        <MemoryCard {memory} ondelete={handleDelete} ontogglepin={handleTogglePin} />
      {/each}
    </div>
  {/if}
</div>
