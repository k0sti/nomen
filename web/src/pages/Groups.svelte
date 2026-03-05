<script lang="ts">
  import { onMount } from 'svelte';
  import GroupTree from '../components/GroupTree.svelte';
  import GroupMembers from '../components/GroupMembers.svelte';
  import { relay, groups, loading, profile, isLoggedIn, getSigner } from '../lib/stores';

  let selectedGroup = $state<string | null>(null);

  const selected = $derived($groups.find(g => g.id === selectedGroup) ?? null);

  onMount(async () => {
    if (!$profile) return;
    loading.set(true);
    try {
      const r = $relay;
      await r.connect();
      const signer = getSigner();
      await r.authenticate(signer);

      const result = await r.listGroups();
      groups.set(result);
    } catch (err: any) {
      console.error('Failed to load groups:', err);
    } finally {
      loading.set(false);
    }
  });
</script>

<div class="max-w-4xl mx-auto space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-2xl font-bold text-gray-100">Groups</h2>
      <p class="text-sm text-gray-500 mt-1">{$groups.length} groups</p>
    </div>
  </div>

  {#if !$isLoggedIn}
    <div class="text-center py-12 text-gray-500">Login to view NIP-29 groups from the relay</div>
  {:else if $loading}
    <div class="text-center py-12 text-gray-500">Loading groups...</div>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-3 gap-6">
      <div class="md:col-span-1 border border-gray-800 rounded-lg p-3 bg-gray-900/50">
        <div class="text-xs font-medium text-gray-500 uppercase tracking-wide mb-3">Groups</div>
        {#if $groups.length === 0}
          <div class="text-sm text-gray-500 text-center py-4">No groups found on relay</div>
        {:else}
          <GroupTree groups={$groups} selected={selectedGroup} onselect={(id) => selectedGroup = id} />
        {/if}
      </div>

      <div class="md:col-span-2 border border-gray-800 rounded-lg p-4 bg-gray-900/50">
        {#if selected}
          <div class="space-y-4">
            <div>
              <h3 class="text-lg font-semibold text-gray-100">{selected.name}</h3>
              <code class="text-xs text-gray-500">{selected.id}</code>
              {#if selected.relay}
                <div class="text-xs text-gray-600 mt-1">Relay: {selected.relay}</div>
              {/if}
            </div>
            <GroupMembers members={selected.members} />
          </div>
        {:else}
          <div class="text-center py-12 text-gray-500">Select a group to view details</div>
        {/if}
      </div>
    </div>
  {/if}
</div>
