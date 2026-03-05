<script lang="ts">
  import TierBadge from './TierBadge.svelte';
  import type { Memory } from '../lib/api';
  import { expandedMemory, client } from '../lib/stores';

  let { memory, ondelete }: { memory: Memory; ondelete?: (topic: string) => void } = $props();

  const isExpanded = $derived($expandedMemory === memory.d_tag);

  function toggle() {
    expandedMemory.set(isExpanded ? null : memory.d_tag);
  }

  function formatDate(iso: string): string {
    return new Date(iso).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  async function handleDelete() {
    if (confirm(`Delete memory "${memory.topic}"?`)) {
      ondelete?.(memory.topic);
    }
  }
</script>

<div
  class="border border-gray-800 rounded-lg hover:border-gray-700 transition-colors bg-gray-900/50"
>
  <!-- Header row -->
  <button
    class="w-full p-4 text-left"
    onclick={toggle}
  >
    <div class="flex items-start justify-between gap-3">
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2 flex-wrap">
          <h3 class="font-mono text-sm font-medium text-gray-200">{memory.topic}</h3>
          <TierBadge tier={memory.tier} scope={memory.scope} />
        </div>
        <p class="text-sm text-gray-400 mt-1 line-clamp-2">{memory.summary}</p>
      </div>
      <div class="text-right shrink-0">
        <div class="text-xs text-gray-500">{formatDate(memory.created_at)}</div>
        <div class="text-xs text-gray-600 mt-1">
          v{memory.version} &middot; {(memory.confidence * 100).toFixed(0)}%
        </div>
      </div>
    </div>
  </button>

  <!-- Expanded detail -->
  {#if isExpanded}
    <div class="px-4 pb-4 border-t border-gray-800/50 pt-3 space-y-3">
      <div>
        <span class="text-xs font-medium text-gray-500 uppercase tracking-wide">Detail</span>
        <p class="text-sm text-gray-300 mt-1">{memory.detail}</p>
      </div>
      <div class="flex items-center gap-4 text-xs text-gray-500">
        <span>Model: <span class="text-gray-400">{memory.model}</span></span>
        <span>Source: <span class="text-gray-400">{memory.source}</span></span>
      </div>
      <div class="flex justify-end">
        <button
          onclick={handleDelete}
          class="px-3 py-1.5 text-xs rounded-md bg-red-900/20 border border-red-800/30 text-red-400 hover:bg-red-900/40 transition-colors"
        >
          Delete
        </button>
      </div>
    </div>
  {/if}
</div>
