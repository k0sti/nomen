<script lang="ts">
  import TierBadge from './TierBadge.svelte';
  import type { Memory } from '../lib/api';
  import { expandedMemory } from '../lib/stores';
  import { nip19 } from 'nostr-tools';
  import { fetchProfileMetadata, compressNpub } from '../lib/nostr';
  import { fetchProfileFromRelay } from '../lib/relay';

  let { memory, ondelete }: { memory: Memory; ondelete?: (memory: Memory) => void } = $props();

  const isExpanded = $derived($expandedMemory === memory.d_tag);
  let sourceMeta = $state<Record<string, any> | null>(null);

  const sourceNpub = $derived.by(() => {
    try {
      return memory.source && memory.source.length === 64 ? nip19.npubEncode(memory.source) : memory.source;
    } catch {
      return memory.source;
    }
  });

  const sourceShort = $derived(sourceNpub ? compressNpub(sourceNpub) : 'unknown');
  const sourceLabel = $derived(sourceMeta?.display_name || sourceMeta?.displayName || sourceMeta?.name || sourceShort);

  $effect(() => {
    if (!memory.source || memory.source.length !== 64) return;
    let cancelled = false;
    (async () => {
      const relayMeta = await fetchProfileFromRelay(memory.source).catch(() => null);
      const meta = relayMeta || await fetchProfileMetadata(memory.source).catch(() => null);
      if (!cancelled) sourceMeta = meta;
    })();
    return () => { cancelled = true; };
  });

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
      ondelete?.(memory);
    }
  }
</script>

<div
  class="border border-gray-800 rounded-lg hover:border-gray-700 transition-colors duration-150 bg-gray-900/50"
>
  <button
    class="w-full p-4 min-h-14 text-left transition-colors duration-150 rounded-t-lg hover:bg-gray-800/30 active:bg-gray-800/50"
    onclick={toggle}
    aria-expanded={isExpanded}
  >
    <div class="flex items-start justify-between gap-3">
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2 flex-wrap">
          <h3 class="font-mono text-sm font-medium text-gray-200">{memory.topic}</h3>
          <TierBadge tier={memory.tier} scope={memory.scope} />
        </div>
        <p class="text-sm text-gray-400 mt-1 {isExpanded ? '' : 'line-clamp-2'}">{memory.summary}</p>
        {#if memory.tier === 'private' || memory.tier === 'group'}
          <div class="mt-2 flex items-center gap-2 text-[11px] text-gray-500">
            {#if memory.tier === 'private'}
              <span class="inline-flex items-center gap-1">🔒 private</span>
              <span>•</span>
              {#if sourceMeta?.picture}
                <img src={sourceMeta.picture} alt="" class="w-3.5 h-3.5 rounded-full object-cover" />
              {:else}
                <span>👤</span>
              {/if}
              <span>{sourceLabel}</span>
              <span class="font-mono">{sourceShort}</span>
            {:else}
              <span class="inline-flex items-center gap-1">👥 group</span>
              <span class="font-mono">{memory.scope || 'unknown'}</span>
            {/if}
          </div>
        {/if}
      </div>
      <div class="text-right shrink-0">
        <div class="text-xs text-gray-500">{formatDate(memory.created_at)}</div>
        <div class="text-xs text-gray-600 mt-1">
          v{memory.version} &middot; {(memory.confidence * 100).toFixed(0)}%
        </div>
      </div>
    </div>
  </button>

  {#if isExpanded}
    <div class="px-4 pb-4 border-t border-gray-800/50 pt-3 space-y-3">
      <div>
        <span class="text-xs font-medium text-gray-500 uppercase tracking-wide">Detail</span>
        <p class="text-sm text-gray-300 mt-1">{memory.detail}</p>
      </div>
      <div class="flex items-center gap-4 text-xs text-gray-500">
        {#if memory.model}
          <span>Model: <span class="text-gray-400">{memory.model}</span></span>
        {/if}
        {#if memory.source}
          <span>Source: <span class="text-gray-400">{memory.source}</span></span>
        {/if}
      </div>
      <div class="flex justify-end">
        <button
          onclick={handleDelete}
          class="px-3 py-2 min-h-9 text-xs rounded-md bg-red-900/20 border border-red-800/30 text-red-400 hover:bg-red-900/40 active:bg-red-900/60 transition-colors duration-150"
        >
          Delete
        </button>
      </div>
    </div>
  {/if}
</div>
