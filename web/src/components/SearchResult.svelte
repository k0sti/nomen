<script lang="ts">
  import TierBadge from './TierBadge.svelte';
  import type { SearchResult } from '../lib/api';

  let { result }: { result: SearchResult } = $props();

  function formatDate(iso: string): string {
    return new Date(iso).toLocaleDateString('en-US', {
      year: 'numeric', month: 'short', day: 'numeric',
    });
  }

  const scoreColor = $derived(
    result.score >= 0.8 ? 'text-emerald-400' :
    result.score >= 0.5 ? 'text-amber-400' : 'text-gray-500'
  );
</script>

<div class="border border-gray-800 rounded-lg p-4 hover:border-gray-700 transition-colors bg-gray-900/50">
  <div class="flex items-start justify-between gap-3">
    <div class="min-w-0 flex-1">
      <div class="flex items-center gap-2 flex-wrap">
        <h3 class="font-mono text-sm font-medium text-gray-200">{result.topic}</h3>
        <TierBadge tier={result.tier} scope={result.scope} />
        <span class="text-xs px-1.5 py-0.5 rounded bg-gray-800 text-gray-400">{result.match_type}</span>
      </div>
      <p class="text-sm text-gray-400 mt-1">{result.summary}</p>
      {#if result.detail}
        <p class="text-xs text-gray-500 mt-1 line-clamp-2">{result.detail}</p>
      {/if}
    </div>
    <div class="text-right shrink-0">
      <div class="text-lg font-bold {scoreColor}">{(result.score * 100).toFixed(0)}</div>
      <div class="text-xs text-gray-600">score</div>
      <div class="text-xs text-gray-500 mt-1">{(result.confidence * 100).toFixed(0)}% conf</div>
      <div class="text-xs text-gray-600 mt-1">{formatDate(result.created_at)}</div>
    </div>
  </div>
</div>
