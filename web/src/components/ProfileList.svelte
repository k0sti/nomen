<script lang="ts">
  import ProfileCard from './ProfileCard.svelte';

  export interface ProfileListEntry {
    key: string;
    pubkey: string;
    name?: string;
    displayName?: string;
    picture?: string;
    about?: string;
    nip05?: string;
    isBot?: boolean;
    isAgent?: boolean;
    isYou?: boolean;
    role?: string;
    ownerPubkey?: string;
    agentCount?: number;
    raw?: any;
  }

  import type { Snippet } from 'svelte';

  let {
    entries = [],
    onselect,
    emptyText = 'No entries',
    actions,
  }: {
    entries: ProfileListEntry[];
    onselect?: (entry: ProfileListEntry) => void;
    emptyText?: string;
    actions?: Snippet<[ProfileListEntry]>;
  } = $props();
</script>

{#if entries.length === 0}
  <div class="p-4 rounded-xl border border-gray-700 bg-gray-800/30 text-center text-sm text-gray-500">
    {emptyText}
  </div>
{:else}
  <div class="space-y-2">
    {#each entries as entry (entry.key)}
      <button class="w-full text-left" onclick={() => onselect?.(entry)}>
        <ProfileCard
          pubkey={entry.pubkey}
          name={entry.name}
          displayName={entry.displayName}
          picture={entry.picture}
          about={entry.about}
          nip05={entry.nip05}
          isBot={entry.isBot}
          isAgent={entry.isAgent}
          isYou={entry.isYou}
          role={entry.role}
          ownerPubkey={entry.ownerPubkey}
          agentCount={entry.agentCount}
        >
          {#snippet children()}
            {#if actions}
              {@render actions(entry)}
            {/if}
          {/snippet}
        </ProfileCard>
      </button>
    {/each}
  </div>
{/if}
