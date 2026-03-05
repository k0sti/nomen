<script lang="ts">
  import { compressNpub } from '../lib/nostr';

  let { members, onremove, onadd }: {
    members: string[];
    onremove?: (npub: string) => void;
    onadd?: (npub: string) => void;
  } = $props();

  let newNpub = $state('');
  let copied = $state<string | null>(null);

  function handleAdd() {
    const v = newNpub.trim();
    if (v && v.startsWith('npub1')) {
      onadd?.(v);
      newNpub = '';
    }
  }

  function copyNpub(npub: string) {
    navigator.clipboard.writeText(npub);
    copied = npub;
    setTimeout(() => copied = null, 1500);
  }
</script>

<div class="space-y-2">
  <div class="text-xs font-medium text-gray-500 uppercase tracking-wide">Members ({members.length})</div>

  <div class="space-y-1">
    {#each members as npub}
      <div class="flex items-center gap-2 py-1.5 px-2 rounded-lg hover:bg-gray-800/50 group">
        <div class="w-6 h-6 rounded-full bg-gray-700 flex items-center justify-center text-xs text-gray-400">
          {npub.slice(5, 6).toUpperCase()}
        </div>
        <code class="text-xs text-gray-400 flex-1 font-mono">{compressNpub(npub)}</code>
        <button
          onclick={() => copyNpub(npub)}
          class="text-gray-600 hover:text-gray-300 text-xs opacity-0 group-hover:opacity-100 transition-opacity"
          title="Copy npub"
        >
          {copied === npub ? 'ok' : 'copy'}
        </button>
        {#if onremove}
          <button
            onclick={() => onremove?.(npub)}
            class="text-red-600 hover:text-red-400 text-xs opacity-0 group-hover:opacity-100 transition-opacity"
          >
            remove
          </button>
        {/if}
      </div>
    {/each}
  </div>

  {#if onadd}
    <div class="flex gap-2 mt-3">
      <input
        type="text"
        placeholder="npub1..."
        bind:value={newNpub}
        onkeydown={(e) => e.key === 'Enter' && handleAdd()}
        class="flex-1 px-3 py-1.5 bg-gray-900 border border-gray-700 rounded-lg text-xs text-gray-200 placeholder-gray-600 focus:border-accent-500 focus:outline-none"
      />
      <button
        onclick={handleAdd}
        disabled={!newNpub.trim().startsWith('npub1')}
        class="px-3 py-1.5 rounded-lg text-xs font-medium bg-accent-600 text-white hover:bg-accent-500 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
      >
        Add
      </button>
    </div>
  {/if}
</div>
