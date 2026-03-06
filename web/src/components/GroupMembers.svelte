<script lang="ts">
  import { onMount } from 'svelte';
  import ProfileCard from './ProfileCard.svelte';
  import { compressNpub, fetchProfilesBatch } from '../lib/nostr';
  import { nip19 } from 'nostr-tools';

  let { members, onremove, onadd }: {
    members: string[];
    onremove?: (npub: string) => void;
    onadd?: (npub: string) => void;
  } = $props();

  let newNpub = $state('');
  let profiles = $state<Map<string, Record<string, any>>>(new Map());

  // Convert hex pubkey to npub if needed
  function toHex(key: string): string {
    if (key.startsWith('npub1')) {
      try { return nip19.decode(key).data as string; } catch { return key; }
    }
    return key;
  }

  function handleAdd() {
    const v = newNpub.trim();
    if (v && v.startsWith('npub1')) {
      onadd?.(v);
      newNpub = '';
    }
  }

  // Fetch profiles for all members
  $effect(() => {
    if (members.length === 0) return;
    const hexKeys = members.map(toHex);
    fetchProfilesBatch(hexKeys).then((result) => {
      profiles = result;
    });
  });
</script>

<div class="space-y-2">
  <div class="text-xs font-medium text-gray-500 uppercase tracking-wide">Members ({members.length})</div>

  <div class="space-y-2">
    {#each members as member}
      {@const hex = toHex(member)}
      {@const meta = profiles.get(hex)}
      <ProfileCard
        pubkey={hex}
        name={meta?.name}
        displayName={meta?.display_name || meta?.displayName}
        picture={meta?.picture}
        about={meta?.about}
        nip05={meta?.nip05}
      >
        {#snippet children()}
          {#if onremove}
            <button
              onclick={() => onremove?.(member)}
              class="px-2 py-1 min-h-7 text-red-600 hover:text-red-400 text-xs transition-colors duration-150 rounded"
              aria-label="Remove member"
            >
              remove
            </button>
          {/if}
        {/snippet}
      </ProfileCard>
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
