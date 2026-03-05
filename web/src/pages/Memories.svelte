<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import MemoryCard from '../components/MemoryCard.svelte';
  import { relay, memories, tierFilter, loading, profile, isLoggedIn, getNip07Signer } from '../lib/stores';
  import type { Memory } from '../lib/api';
  import type { Subscription } from '../lib/relay';

  let filterText = $state('');
  let sub: Subscription | null = null;

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
    if (!$profile) return;
    loading.set(true);
    try {
      const r = $relay;
      await r.connect();
      const signer = getNip07Signer();
      await r.authenticate(signer);

      const result = await r.listMemories($profile.pubkey);
      memories.set(result);

      // Live subscription for new memories
      sub = r.subscribeMemories($profile.pubkey, (m: Memory) => {
        memories.update((ms) => {
          const idx = ms.findIndex((x) => x.d_tag === m.d_tag);
          if (idx >= 0) {
            const updated = [...ms];
            updated[idx] = m;
            return updated;
          }
          return [m, ...ms];
        });
      });
    } catch (err: any) {
      console.error('Failed to load memories:', err);
    } finally {
      loading.set(false);
    }
  });

  onDestroy(() => {
    sub?.close();
  });

  async function handleDelete(memory: Memory) {
    if (!memory.id) return;
    try {
      const signer = getNip07Signer();
      await $relay.deleteMemory(memory.id, signer);
      memories.update((ms) => ms.filter((m) => m.d_tag !== memory.d_tag));
    } catch (err: any) {
      console.error('Failed to delete memory:', err);
    }
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

  {#if !$isLoggedIn}
    <div class="text-center py-12 text-gray-500">Login to view memories from the relay</div>
  {:else if $loading}
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
