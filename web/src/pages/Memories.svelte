<script lang="ts">
  import { onMount } from 'svelte';
  import MemoryCard from '../components/MemoryCard.svelte';
  import TierBadge from '../components/TierBadge.svelte';
  import { client, memories, tierFilter, loading } from '../lib/stores';
  import type { Memory } from '../lib/api';

  let filterText = $state('');

  const filtered = $derived(
    $memories.filter((m) => {
      const matchesTier = !$tierFilter || m.tier === $tierFilter;
      const matchesText =
        !filterText ||
        m.topic.toLowerCase().includes(filterText.toLowerCase()) ||
        m.summary.toLowerCase().includes(filterText.toLowerCase());
      return matchesTier && matchesText;
    })
  );

  const stats = $derived({
    total: $memories.length,
    public: $memories.filter((m) => m.tier === 'public').length,
    group: $memories.filter((m) => m.tier === 'group').length,
    private: $memories.filter((m) => m.tier === 'private').length,
  });

  onMount(async () => {
    loading.set(true);
    try {
      const result = await $client.listMemories($tierFilter || undefined);
      memories.set(result);
    } finally {
      loading.set(false);
    }
  });

  async function handleDelete(topic: string) {
    await $client.deleteMemory(topic);
    memories.update((ms) => ms.filter((m) => m.topic !== topic));
  }

  function setTierFilter(tier: string) {
    tierFilter.set($tierFilter === tier ? '' : tier);
  }
</script>

<div class="max-w-4xl mx-auto space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-2xl font-bold text-gray-100">Memories</h2>
      <p class="text-sm text-gray-500 mt-1">
        {stats.total} memories &mdash; {stats.public} public, {stats.group} group, {stats.private} private
      </p>
    </div>
  </div>

  <div class="flex items-center gap-3">
    <input
      type="text"
      placeholder="Filter by topic or summary..."
      bind:value={filterText}
      class="flex-1 px-4 py-2.5 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 transition-colors duration-150 focus:border-accent-500"
    />
    <div class="flex gap-1.5">
      {#each ['public', 'group', 'private'] as tier}
        <button
          onclick={() => setTierFilter(tier)}
          class="px-3 py-2 min-h-11 rounded-md text-xs font-medium border transition-colors duration-150
            {$tierFilter === tier
              ? 'border-accent-500 bg-accent-500/20 text-accent-400'
              : 'border-gray-700 bg-gray-800/50 text-gray-400 hover:text-gray-200 active:bg-gray-700'}"
        >
          {tier}
        </button>
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
      {$memories.length === 0 ? 'No memories yet' : 'No memories match your filters'}
    </div>
  {:else}
    <div class="space-y-2">
      {#each filtered as memory (memory.d_tag)}
        <MemoryCard {memory} ondelete={handleDelete} />
      {/each}
    </div>
  {/if}
</div>
