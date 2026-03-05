<script lang="ts">
  import { compressNpub } from '../lib/nostr';
  import { nip19 } from 'nostr-tools';

  let { members, onremove, onadd }: {
    members: string[];
    onremove?: (npub: string) => void;
    onadd?: (npub: string) => void;
  } = $props();

  let newNpub = $state('');
  let copied = $state<string | null>(null);

  // Convert hex pubkey to npub if needed
  function toNpub(key: string): string {
    if (key.startsWith('npub1')) return key;
    try {
      return nip19.npubEncode(key);
    } catch {
      return key;
    }
  }

  function handleAdd() {
    const v = newNpub.trim();
    if (v && v.startsWith('npub1')) {
      onadd?.(v);
      newNpub = '';
    }
  }

  function copyNpub(key: string) {
    const npub = toNpub(key);
    navigator.clipboard.writeText(npub);
    copied = key;
    setTimeout(() => copied = null, 1500);
  }
</script>

<div class="space-y-2">
  <div class="text-xs font-medium text-gray-500 uppercase tracking-wide">Members ({members.length})</div>

  <div class="space-y-1">
    {#each members as member}
      {@const npub = toNpub(member)}
      <div class="flex items-center gap-2 py-2 px-2 min-h-11 rounded-lg hover:bg-gray-800/50 group transition-colors duration-150">
        <div class="w-6 h-6 rounded-full bg-gray-700 flex items-center justify-center text-xs text-gray-400">
          {npub.slice(5, 6).toUpperCase()}
        </div>
        <code class="text-xs text-gray-400 flex-1 font-mono">{compressNpub(npub)}</code>
        <button
          onclick={() => copyNpub(member)}
          class="px-2 py-1 min-h-7 text-gray-600 hover:text-gray-300 text-xs opacity-0 group-hover:opacity-100 focus-visible:opacity-100 transition-all duration-150 rounded"
          title="Copy npub"
          aria-label="Copy npub"
        >
          {copied === member ? 'ok' : 'copy'}
        </button>
        {#if onremove}
          <button
            onclick={() => onremove?.(member)}
            class="px-2 py-1 min-h-7 text-red-600 hover:text-red-400 text-xs opacity-0 group-hover:opacity-100 focus-visible:opacity-100 transition-all duration-150 rounded"
            aria-label="Remove member"
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
        class="flex-1 px-3 py-2 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-xs text-gray-200 placeholder-gray-600 transition-colors duration-150 focus:border-accent-500"
      />
      <button
        onclick={handleAdd}
        disabled={!newNpub.trim().startsWith('npub1')}
        class="px-4 py-2 min-h-11 rounded-lg text-xs font-medium bg-accent-600 text-white hover:bg-accent-500 active:bg-accent-400 disabled:opacity-30 disabled:cursor-not-allowed transition-colors duration-150"
      >
        Add
      </button>
    </div>
  {/if}
</div>
