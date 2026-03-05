<script lang="ts">
  import { onMount } from 'svelte';
  import MessageItem from '../components/MessageItem.svelte';
  import { relay, messages, channelFilter, loading, profile, isLoggedIn, groups, getSigner } from '../lib/stores';

  let senderFilter = $state('');
  let selectedGroup = $state('');

  const filtered = $derived(
    $messages.filter((m) => {
      const matchesChannel = !$channelFilter || m.channel === $channelFilter;
      const matchesSender = !senderFilter || m.sender.toLowerCase().includes(senderFilter.toLowerCase());
      return matchesChannel && matchesSender;
    })
  );

  const channels = $derived([...new Set($messages.map(m => m.channel).filter(Boolean))]);

  const stats = $derived({
    total: $messages.length,
    consolidated: $messages.filter(m => m.consolidated).length,
  });

  onMount(async () => {
    if (!$profile) return;
    loading.set(true);
    try {
      const r = $relay;
      await r.connect();
      const signer = getSigner();
      await r.authenticate(signer);

      if ($groups.length === 0) {
        const grps = await r.listGroups();
        groups.set(grps);
      }

      if ($groups.length > 0) {
        selectedGroup = $groups[0].id;
        const msgs = await r.getGroupMessages(selectedGroup, 100);
        messages.set(msgs);
      }
    } catch (err: any) {
      console.error('Failed to load messages:', err);
    } finally {
      loading.set(false);
    }
  });

  async function loadGroupMessages() {
    if (!selectedGroup) return;
    loading.set(true);
    try {
      const msgs = await $relay.getGroupMessages(selectedGroup, 100);
      messages.set(msgs);
    } catch (err: any) {
      console.error('Failed to load messages:', err);
    } finally {
      loading.set(false);
    }
  }
</script>

<div class="max-w-4xl mx-auto space-y-6">
  <div>
    <h2 class="text-2xl font-bold text-gray-100">Messages</h2>
    <p class="text-sm text-gray-500 mt-1">
      {stats.total} messages &mdash; {stats.consolidated} consolidated
    </p>
  </div>

  {#if !$isLoggedIn}
    <div class="text-center py-12 text-gray-500">Login to view group messages from the relay</div>
  {:else}
    <div class="flex items-center gap-3 flex-wrap">
      <select
        bind:value={selectedGroup}
        onchange={loadGroupMessages}
        class="px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
      >
        <option value="">Select group...</option>
        {#each $groups as g}
          <option value={g.id}>{g.name || g.id}</option>
        {/each}
      </select>
      <input
        type="text"
        placeholder="Filter by sender..."
        bind:value={senderFilter}
        class="flex-1 min-w-48 px-4 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 focus:border-accent-500 focus:outline-none"
      />
      <select
        bind:value={$channelFilter}
        class="px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
      >
        <option value="">All channels</option>
        {#each channels as ch}
          <option value={ch}>#{ch}</option>
        {/each}
      </select>
    </div>

    {#if $loading}
      <div class="text-center py-12 text-gray-500">Loading messages...</div>
    {:else if filtered.length === 0}
      <div class="text-center py-12 text-gray-500">
        {$messages.length === 0 ? 'No messages yet' : 'No messages match your filters'}
      </div>
    {:else}
      <div class="divide-y divide-gray-800/50">
        {#each filtered as message (message.id)}
          <MessageItem {message} />
        {/each}
      </div>
    {/if}
  {/if}
</div>
