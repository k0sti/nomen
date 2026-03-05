<script lang="ts">
  import { onMount } from 'svelte';
  import MessageItem from '../components/MessageItem.svelte';
  import { client, messages, sourceFilter, channelFilter, loading } from '../lib/stores';

  let senderFilter = $state('');

  const filtered = $derived(
    $messages.filter((m) => {
      const matchesSource = !$sourceFilter || m.source === $sourceFilter;
      const matchesChannel = !$channelFilter || m.channel === $channelFilter;
      const matchesSender = !senderFilter || m.sender.toLowerCase().includes(senderFilter.toLowerCase());
      return matchesSource && matchesChannel && matchesSender;
    })
  );

  const sources = $derived([...new Set($messages.map(m => m.source))]);
  const channels = $derived([...new Set($messages.map(m => m.channel).filter(Boolean))]);

  const stats = $derived({
    total: $messages.length,
    consolidated: $messages.filter(m => m.consolidated).length,
  });

  onMount(async () => {
    loading.set(true);
    try {
      const result = await $client.getMessages({ limit: 100 });
      messages.set(result);
    } finally {
      loading.set(false);
    }
  });
</script>

<div class="max-w-4xl mx-auto space-y-6">
  <div>
    <h2 class="text-2xl font-bold text-gray-100">Messages</h2>
    <p class="text-sm text-gray-500 mt-1">
      {stats.total} messages &mdash; {stats.consolidated} consolidated
    </p>
  </div>

  <!-- Filters -->
  <div class="flex items-center gap-3 flex-wrap">
    <input
      type="text"
      placeholder="Filter by sender..."
      bind:value={senderFilter}
      class="flex-1 min-w-48 px-4 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 focus:border-accent-500 focus:outline-none"
    />
    <select
      bind:value={$sourceFilter}
      class="px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
    >
      <option value="">All sources</option>
      {#each sources as src}
        <option value={src}>{src}</option>
      {/each}
    </select>
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

  <!-- Message timeline -->
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
</div>
