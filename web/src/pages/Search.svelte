<script lang="ts">
  import SearchResultCard from '../components/SearchResult.svelte';
  import { api, searchResults, searchQuery, loading, showError } from '../lib/stores';

  let mode = $state<'hybrid' | 'text'>('hybrid');

  async function doSearch() {
    const q = $searchQuery.trim();
    if (!q) return;
    loading.set(true);
    try {
      const results = await $api.search(q, { limit: 20, mode });
      searchResults.set(results);
    } catch (err: any) {
      showError('Search failed: ' + (err.message || err));
      searchResults.set([]);
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

  <div class="flex gap-3">
    <input
      type="text"
      placeholder="Search memories..."
      bind:value={$searchQuery}
      onkeydown={handleKeydown}
      class="flex-1 px-4 py-2.5 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 transition-colors duration-150 focus:border-accent-500"
    />
    <div class="flex rounded-lg border border-gray-700 overflow-hidden">
      <button
        onclick={() => mode = 'hybrid'}
        class="px-3 py-2 min-h-11 text-xs font-medium transition-colors duration-150 {mode === 'hybrid' ? 'bg-accent-600 text-white' : 'bg-gray-800 text-gray-400 hover:text-gray-200 active:bg-gray-700'}"
      >
        Hybrid
      </button>
      <button
        onclick={() => mode = 'text'}
        class="px-3 py-2 min-h-11 text-xs font-medium transition-colors duration-150 {mode === 'text' ? 'bg-accent-600 text-white' : 'bg-gray-800 text-gray-400 hover:text-gray-200 active:bg-gray-700'}"
      >
        Text
      </button>
    </div>
    <button
      onclick={doSearch}
      disabled={!$searchQuery.trim()}
      class="px-5 py-2.5 min-h-11 rounded-lg text-sm font-medium bg-accent-600 text-white hover:bg-accent-500 active:bg-accent-400 disabled:opacity-30 disabled:cursor-not-allowed transition-colors duration-150"
    >
      Search
    </button>
  </div>

  {#if $loading}
    <div class="space-y-2">
      {#each { length: 3 } as _}
        <div class="border border-gray-800 rounded-lg p-4 bg-gray-900/50">
          <div class="flex items-start justify-between gap-3">
            <div class="flex-1 space-y-2">
              <div class="flex items-center gap-2">
                <div class="skeleton h-4 w-36"></div>
                <div class="skeleton h-5 w-14"></div>
              </div>
              <div class="skeleton h-3.5 w-full max-w-sm"></div>
            </div>
            <div class="space-y-1.5 text-right">
              <div class="skeleton h-6 w-10 ml-auto"></div>
              <div class="skeleton h-3 w-14 ml-auto"></div>
            </div>
          </div>
        </div>
      {/each}
    </div>
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
