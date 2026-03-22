<script lang="ts">
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';
  import TierBadge from './TierBadge.svelte';
  import type { Memory } from '../lib/api';
  import { expandedMemory } from '../lib/stores';
  import { nip19 } from 'nostr-tools';

  marked.setOptions({ breaks: true, gfm: true });
  import { fetchProfileMetadata, compressNpub } from '../lib/nostr';
  import { fetchProfileFromRelay } from '../lib/relay';

  let { memory, ondelete, ontogglepin }: { memory: Memory; ondelete?: (memory: Memory) => void; ontogglepin?: (memory: Memory) => void } = $props();

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
    if (!iso) return '';
    return new Date(iso).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  function formatSourceTimeRange(start: string | null, end: string | null): string | null {
    if (!start && !end) return null;
    const fmt = (s: string) => new Date(s).toLocaleDateString('en-US', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
    if (start && end) return `${fmt(start)} — ${fmt(end)}`;
    return fmt(start || end!);
  }

  const sourceTimeRange = $derived(formatSourceTimeRange(memory.source_time_start, memory.source_time_end));

  /** Strip topic and summary from start of detail to avoid redundancy */
  const cleanDetail = $derived.by(() => {
    if (!memory.detail) return '';
    let d = memory.detail;
    // Strip leading line if it matches topic or d_tag
    const lines = d.split('\n');
    while (lines.length > 0) {
      const first = lines[0].trim().replace(/^#+\s*/, '');
      if (!first) { lines.shift(); continue; }
      if (first === memory.topic || first === memory.d_tag || first === memory.summary) {
        lines.shift();
        continue;
      }
      break;
    }
    return lines.join('\n').trim();
  });

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
          {#if memory.pinned}
            <span class="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-medium bg-amber-900/40 text-amber-400 border border-amber-800/50">PIN</span>
          {/if}
          <h3 class="font-mono text-sm font-medium text-gray-200">{memory.topic}</h3>
          <TierBadge visibility={memory.visibility} scope={memory.scope} />
          {#if memory.embedded}
            <span class="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-medium bg-cyan-900/40 text-cyan-400 border border-cyan-800/50">VEC</span>
          {/if}
        </div>
        <p class="text-sm text-gray-400 mt-1 {isExpanded ? '' : 'line-clamp-2'}">{memory.summary}</p>
        {#if memory.visibility === 'personal' || memory.visibility === 'private' || memory.visibility === 'group'}
          <div class="mt-2 flex items-center gap-2 text-[11px] text-gray-500">
            {#if memory.visibility === 'personal' || memory.visibility === 'private'}
              <span class="inline-flex items-center gap-1">locked private</span>
              <span>·</span>
              {#if sourceMeta?.picture}
                <img src={sourceMeta.picture} alt="" class="w-3.5 h-3.5 rounded-full object-cover" />
              {/if}
              <span>{sourceLabel}</span>
              <span class="font-mono">{sourceShort}</span>
            {:else}
              <span class="inline-flex items-center gap-1">group</span>
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
      {#if cleanDetail}
        <div class="text-sm text-gray-300 markdown-detail">{@html DOMPurify.sanitize(marked.parse(cleanDetail) as string)}</div>
      {/if}
      <div class="flex flex-wrap gap-x-3 gap-y-1 text-xs text-gray-500">
        {#if memory.model}<span>Model: <span class="text-gray-400">{memory.model}</span></span>{/if}
        {#if memory.source && memory.source !== memory.model}<span>Source: <span class="text-gray-400">{memory.source}</span></span>{/if}
        {#if memory.updated_at && memory.updated_at !== memory.created_at}<span>Updated: <span class="text-gray-400">{formatDate(memory.updated_at)}</span></span>{/if}
        {#if memory.importance != null}<span>Importance: <span class="text-gray-400">{memory.importance}</span></span>{/if}
        {#if memory.access_count > 0}<span>Accessed: <span class="text-gray-400">{memory.access_count}×</span></span>{/if}
        {#if sourceTimeRange}<span>Time: <span class="text-gray-400">{sourceTimeRange}</span></span>{/if}
        {#if memory.consolidated_from}<span>From: <span class="text-gray-400">{memory.consolidated_from}</span></span>{/if}
        {#if memory.consolidated_at}<span>Consolidated: <span class="text-gray-400">{formatDate(memory.consolidated_at)}</span></span>{/if}
      </div>
      <div class="flex justify-end gap-2">
        <button
          onclick={() => ontogglepin?.(memory)}
          class="px-3 py-2 min-h-9 text-xs rounded-md border transition-colors duration-150
            {memory.pinned
              ? 'bg-amber-900/20 border-amber-800/30 text-amber-400 hover:bg-amber-900/40'
              : 'bg-gray-800/50 border-gray-700 text-gray-400 hover:bg-gray-700'}"
        >
          {memory.pinned ? '📌 Unpin' : '📌 Pin'}
        </button>
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

<style>
  :global(.markdown-detail h1),
  :global(.markdown-detail h2),
  :global(.markdown-detail h3) {
    font-weight: 600;
    margin-top: 0.75em;
    margin-bottom: 0.25em;
    color: #e5e7eb;
  }
  :global(.markdown-detail h1) { font-size: 1.1em; }
  :global(.markdown-detail h2) { font-size: 1em; }
  :global(.markdown-detail h3) { font-size: 0.95em; }
  :global(.markdown-detail p) { margin: 0.4em 0; }
  :global(.markdown-detail ul),
  :global(.markdown-detail ol) {
    padding-left: 1.5em;
    margin: 0.4em 0;
  }
  :global(.markdown-detail ul) { list-style: disc; }
  :global(.markdown-detail ol) { list-style: decimal; }
  :global(.markdown-detail li) { margin: 0.15em 0; }
  :global(.markdown-detail code) {
    background: rgba(107, 114, 128, 0.2);
    padding: 0.1em 0.35em;
    border-radius: 0.25em;
    font-size: 0.9em;
  }
  :global(.markdown-detail pre) {
    background: rgba(17, 24, 39, 0.8);
    padding: 0.75em;
    border-radius: 0.375em;
    overflow-x: auto;
    margin: 0.5em 0;
  }
  :global(.markdown-detail pre code) {
    background: none;
    padding: 0;
  }
  :global(.markdown-detail a) {
    color: #60a5fa;
    text-decoration: underline;
  }
  :global(.markdown-detail blockquote) {
    border-left: 3px solid #4b5563;
    padding-left: 0.75em;
    margin: 0.5em 0;
    color: #9ca3af;
  }
  :global(.markdown-detail table) {
    border-collapse: collapse;
    width: 100%;
    margin: 0.5em 0;
  }
  :global(.markdown-detail th),
  :global(.markdown-detail td) {
    border: 1px solid #374151;
    padding: 0.35em 0.6em;
    font-size: 0.9em;
  }
  :global(.markdown-detail th) {
    background: rgba(55, 65, 81, 0.4);
    font-weight: 600;
  }
</style>
