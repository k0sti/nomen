<script lang="ts">

  import GroupTree from '../components/GroupTree.svelte';
  import GroupMembers from '../components/GroupMembers.svelte';
  import { relay, groups, loading, profile, isLoggedIn, getSigner, ensureConnected, showError, showInfo } from '../lib/stores';

  let selectedGroup = $state<string | null>(null);

  const selected = $derived($groups.find(g => g.id === selectedGroup) ?? null);

  // Create group form
  let showCreateForm = $state(false);
  let newGroupName = $state('');
  let creating = $state(false);

  async function loadGroups() {
    loading.set(true);
    try {
      const r = await ensureConnected();
      const result = await r.listGroups();
      groups.set(result);
    } catch (err: any) {
      showError('Failed to load groups: ' + (err.message || err));
    } finally {
      loading.set(false);
    }
  }

  $effect(() => {
    if ($profile && $groups.length === 0 && !$loading) {
      loadGroups();
    }
  });

  async function createGroup() {
    if (!newGroupName.trim()) return;
    creating = true;
    try {
      // NIP-29 group creation is relay-managed — this is a placeholder
      // In practice, groups are created via relay admin commands
      showInfo('Group creation requires relay admin access (NIP-29). Contact your relay operator.');
      showCreateForm = false;
      newGroupName = '';
    } catch (err: any) {
      showError('Failed to create group: ' + (err.message || err));
    } finally {
      creating = false;
    }
  }
</script>

<div class="max-w-4xl mx-auto space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-2xl font-bold text-gray-100">Groups</h2>
      <p class="text-sm text-gray-500 mt-1">{$groups.length} groups</p>
    </div>
    {#if $isLoggedIn}
      <button
        onclick={() => showCreateForm = !showCreateForm}
        class="px-4 py-2 min-h-11 rounded-lg border border-accent-600/50 bg-accent-600/10 hover:bg-accent-600/20 text-accent-400 text-sm font-medium transition-colors duration-150"
      >
        {showCreateForm ? 'Cancel' : '+ New Group'}
      </button>
    {/if}
  </div>

  {#if showCreateForm}
    <div class="p-4 rounded-lg border border-gray-700 bg-gray-900/50 space-y-3">
      <label class="block">
        <span class="text-xs text-gray-400">Group Name</span>
        <input type="text" bind:value={newGroupName} placeholder="e.g. my-team" class="mt-1 w-full px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500" />
      </label>
      <p class="text-xs text-gray-500">NIP-29 groups are managed by the relay. This will request group creation from the relay operator.</p>
      <div class="flex justify-end">
        <button
          onclick={createGroup}
          disabled={creating || !newGroupName.trim()}
          class="px-4 py-2 rounded-lg bg-accent-600 hover:bg-accent-500 disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium transition-colors duration-150"
        >
          {creating ? 'Creating...' : 'Create Group'}
        </button>
      </div>
    </div>
  {/if}

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
