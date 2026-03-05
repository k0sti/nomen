<script lang="ts">
  import SearchResultCard from '../components/SearchResult.svelte';
  import { client, searchResults, searchQuery, loading } from '../lib/stores';

  let mode = $state<'hybrid' | 'text'>('hybrid');

  async function doSearch() {
    const q = $searchQuery.trim();
    if (!q) return;
    loading.set(true);
    try {
      const results = await $client.search(q, { limit: 20 });
      searchResults.set(results);
    } finally {
      loading.set(false);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') doSearch();
  }
</script>

<div class="max-w-4xl mx-auto space-y-6">
  <div>
    <h2 class="text-2xl font-bold text-gray-100">Search</h2>
    <p class="text-sm text-gray-500 mt-1">Search memories by topic, content, or semantic similarity</p>
  </div>

  <!-- Search bar -->
  <div class="flex gap-3">
    <input
      type="text"
      placeholder="Search memories..."
      bind:value={$searchQuery}
      onkeydown={handleKeydown}
      class="flex-1 px-4 py-2.5 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 focus:border-accent-500 focus:outline-none"
    />
    <div class="flex rounded-lg border border-gray-700 overflow-hidden">
      <button
        onclick={() => mode = 'hybrid'}
        class="px-3 py-2 text-xs font-medium transition-colors {mode === 'hybrid' ? 'bg-accent-600 text-white' : 'bg-gray-800 text-gray-400 hover:text-gray-200'}"
      >
        Hybrid
      </button>
      <button
        onclick={() => mode = 'text'}
        class="px-3 py-2 text-xs font-medium transition-colors {mode === 'text' ? 'bg-accent-600 text-white' : 'bg-gray-800 text-gray-400 hover:text-gray-200'}"
      >
        Text
      </button>
    </div>
    <button
      onclick={doSearch}
      disabled={!$searchQuery.trim()}
      class="px-5 py-2.5 rounded-lg text-sm font-medium bg-accent-600 text-white hover:bg-accent-500 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
    >
      Search
    </button>
  </div>

  <!-- Results -->
  {#if $loading}
    <div class="text-center py-12 text-gray-500">Searching...</div>
  {:else if $searchResults.length > 0}
    <div class="text-xs text-gray-500">{$searchResults.length} results</div>
    <div class="space-y-2">
      {#each $searchResults as result (result.topic)}
        <SearchResultCard {result} />
      {/each}
    </div>
  {:else if $searchQuery.trim()}
    <div class="text-center py-12 text-gray-500">No results found</div>
  {:else}
    <div class="text-center py-12 text-gray-500">Enter a query to search memories</div>
  {/if}
</div>
