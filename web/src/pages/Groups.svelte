<script lang="ts">
  import { onMount } from 'svelte';
  import GroupTree from '../components/GroupTree.svelte';
  import GroupMembers from '../components/GroupMembers.svelte';
  import { client, groups, loading } from '../lib/stores';

  let selectedGroup = $state<string | null>(null);
  let showCreate = $state(false);
  let newId = $state('');
  let newName = $state('');

  const selected = $derived($groups.find(g => g.id === selectedGroup) ?? null);

  onMount(async () => {
    loading.set(true);
    try {
      const result = await $client.listGroups();
      groups.set(result);
    } finally {
      loading.set(false);
    }
  });

  async function handleCreate() {
    const id = newId.trim();
    const name = newName.trim();
    if (!id || !name) return;
    await $client.createGroup(id, name, []);
    groups.update(gs => [...gs, { id, name, members: [], created_at: new Date().toISOString() }]);
    newId = '';
    newName = '';
    showCreate = false;
    selectedGroup = id;
  }

  async function handleAddMember(npub: string) {
    if (!selectedGroup) return;
    await $client.addMember(selectedGroup, npub);
    groups.update(gs => gs.map(g =>
      g.id === selectedGroup ? { ...g, members: [...g.members, npub] } : g
    ));
  }

  async function handleRemoveMember(npub: string) {
    if (!selectedGroup || !confirm(`Remove member?`)) return;
    await $client.removeMember(selectedGroup, npub);
    groups.update(gs => gs.map(g =>
      g.id === selectedGroup ? { ...g, members: g.members.filter(m => m !== npub) } : g
    ));
  }
</script>

<div class="max-w-4xl mx-auto space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-2xl font-bold text-gray-100">Groups</h2>
      <p class="text-sm text-gray-500 mt-1">{$groups.length} groups</p>
    </div>
    <button
      onclick={() => showCreate = !showCreate}
      class="px-4 py-2 rounded-lg text-sm font-medium bg-accent-600 text-white hover:bg-accent-500 transition-colors"
    >
      {showCreate ? 'Cancel' : 'Create Group'}
    </button>
  </div>

  {#if showCreate}
    <div class="p-4 border border-gray-700 rounded-lg bg-gray-900/50 space-y-3">
      <div class="grid grid-cols-2 gap-3">
        <input
          type="text"
          placeholder="Group ID (e.g. techteam)"
          bind:value={newId}
          class="px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 focus:border-accent-500 focus:outline-none"
        />
        <input
          type="text"
          placeholder="Display name"
          bind:value={newName}
          class="px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 focus:border-accent-500 focus:outline-none"
        />
      </div>
      <button
        onclick={handleCreate}
        disabled={!newId.trim() || !newName.trim()}
        class="px-4 py-2 rounded-lg text-sm font-medium bg-accent-600 text-white hover:bg-accent-500 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
      >
        Create
      </button>
    </div>
  {/if}

  {#if $loading}
    <div class="text-center py-12 text-gray-500">Loading groups...</div>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-3 gap-6">
      <!-- Group tree -->
      <div class="md:col-span-1 border border-gray-800 rounded-lg p-3 bg-gray-900/50">
        <div class="text-xs font-medium text-gray-500 uppercase tracking-wide mb-3">Groups</div>
        {#if $groups.length === 0}
          <div class="text-sm text-gray-500 text-center py-4">No groups yet</div>
        {:else}
          <GroupTree groups={$groups} selected={selectedGroup} onselect={(id) => selectedGroup = id} />
        {/if}
      </div>

      <!-- Group detail -->
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
            <GroupMembers
              members={selected.members}
              onadd={handleAddMember}
              onremove={handleRemoveMember}
            />
          </div>
        {:else}
          <div class="text-center py-12 text-gray-500">Select a group to view details</div>
        {/if}
      </div>
    </div>
  {/if}
</div>
