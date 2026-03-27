<script lang="ts">
  import MessageItem from '../components/MessageItem.svelte';
  import { api, showError, showInfo } from '../lib/stores';
  import type { Message, MessageContextResult } from '../lib/api';

  // ── State ──────────────────────────────────────────────────────────
  let messages = $state<Message[]>([]);
  let msgCount = $state(0);
  let loadingList = $state(false);
  let loadingMore = $state(false);

  // ── Filters ────────────────────────────────────────────────────────
  let senderFilter = $state('');
  let chatFilter = $state('');
  let threadFilter = $state('');
  let platformFilter = $state('');
  let sincePreset = $state('');
  let includeConsolidated = $state(false);
  let messageLimit = $state(100);

  // ── Search ─────────────────────────────────────────────────────────
  let searchQuery = $state('');
  let isSearchMode = $state(false);

  // ── Context expand ─────────────────────────────────────────────────
  let expandedId = $state<string | null>(null);
  let contextMessages = $state<Message[]>([]);
  let contextTargetIndex = $state(-1);
  let loadingContext = $state(false);

  // ── Consolidation ──────────────────────────────────────────────────
  let consolidating = $state(false);

  const stats = $derived({
    total: msgCount,
    consolidated: messages.filter(m => m.consolidated).length,
  });

  function sinceFromPreset(preset: string): string | undefined {
    if (!preset) return undefined;
    const now = Date.now();
    const offsets: Record<string, number> = {
      '1h': 60 * 60 * 1000,
      '24h': 24 * 60 * 60 * 1000,
      '7d': 7 * 24 * 60 * 60 * 1000,
      '30d': 30 * 24 * 60 * 60 * 1000,
    };
    const off = offsets[preset];
    if (!off) return undefined;
    return new Date(now - off).toISOString();
  }

  async function loadMessages() {
    loadingList = true;
    isSearchMode = false;
    searchQuery = '';
    expandedId = null;
    try {
      const result = await $api.listMessages({
        platform: platformFilter || undefined,
        chat: chatFilter || undefined,
        thread: threadFilter || undefined,
        sender: senderFilter || undefined,
        since: sinceFromPreset(sincePreset),
        include_consolidated: includeConsolidated || undefined,
        limit: messageLimit,
      });
      messages = result.messages;
      msgCount = result.count;
    } catch (err: any) {
      showError('Failed to load messages: ' + (err.message || err));
    } finally {
      loadingList = false;
    }
  }

  async function handleSearch() {
    const q = searchQuery.trim();
    if (!q) {
      isSearchMode = false;
      loadMessages();
      return;
    }
    loadingList = true;
    isSearchMode = true;
    expandedId = null;
    try {
      // Full-text search is still approximated client-side in the web UI,
      // but the underlying fetch now uses canonical query filters.
      const result = await $api.listMessages({
        platform: platformFilter || undefined,
        chat: chatFilter || undefined,
        thread: threadFilter || undefined,
        sender: senderFilter || undefined,
        since: sinceFromPreset(sincePreset),
        include_consolidated: includeConsolidated || undefined,
        limit: 500,
      });
      const lower = q.toLowerCase();
      const filtered = result.messages.filter(
        m => m.content.toLowerCase().includes(lower) ||
             m.sender.toLowerCase().includes(lower) ||
             ((m.thread ? `${m.chat || ''}/${m.thread}` : (m.chat || m.channel || ''))).toLowerCase().includes(lower) ||
             (m.chat || '').toLowerCase().includes(lower) ||
             (m.thread || '').toLowerCase().includes(lower) ||
             (m.community || '').toLowerCase().includes(lower) ||
             (m.platform || '').toLowerCase().includes(lower)
      );
      messages = filtered;
      msgCount = filtered.length;
    } catch (err: any) {
      showError('Search failed: ' + (err.message || err));
    } finally {
      loadingList = false;
    }
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') handleSearch();
  }

  async function toggleContext(msg: Message) {
    if (expandedId === (msg.source_id || msg.id)) {
      expandedId = null;
      contextMessages = [];
      contextTargetIndex = -1;
      return;
    }

    if (!msg.chat) {
      expandedId = null;
      return;
    }

    expandedId = msg.source_id || msg.id;
    loadingContext = true;
    try {
      const result: MessageContextResult = await $api.getMessageContext({
        platform: msg.platform,
        community: msg.community,
        chat: msg.chat,
        thread: msg.thread,
        before: Math.floor(Date.now() / 1000),
        limit: 11,
      });
      contextMessages = result.messages;
      contextTargetIndex = result.target_index ?? -1;
    } catch (err: any) {
      showError('Failed to load context: ' + (err.message || err));
      expandedId = null;
    } finally {
      loadingContext = false;
    }
  }

  async function triggerConsolidate() {
    consolidating = true;
    try {
      const report = await $api.consolidate({
        platform: platformFilter || undefined,
        chat: chatFilter || undefined,
        thread: threadFilter || undefined,
      });
      showInfo(`Consolidated ${report.messages_processed} messages into ${report.memories_created} memories`);
      loadMessages();
    } catch (err: any) {
      showError('Consolidation failed: ' + (err.message || err));
    } finally {
      consolidating = false;
    }
  }

  function applyFilters() {
    if (isSearchMode && searchQuery.trim()) {
      handleSearch();
    } else {
      loadMessages();
    }
  }

  function clearFilters() {
    senderFilter = '';
    chatFilter = '';
    threadFilter = '';
    platformFilter = '';
    sincePreset = '';
    includeConsolidated = false;
    searchQuery = '';
    isSearchMode = false;
    loadMessages();
  }

  // Load on mount
  $effect(() => {
    // Only runs once on mount
    loadMessages();
    return () => {};
  });
</script>

<div class="max-w-5xl mx-auto space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-2xl font-bold text-gray-100">Messages</h2>
      <p class="text-sm text-gray-500 mt-1">
        {stats.total} messages{#if stats.consolidated > 0} &mdash; {stats.consolidated} consolidated{/if}
        {#if isSearchMode}<span class="text-accent-400 ml-1">(search results)</span>{/if}
      </p>
    </div>
    <button
      onclick={triggerConsolidate}
      disabled={consolidating || messages.length === 0}
      class="px-4 py-2 min-h-11 rounded-lg border border-accent-600/50 bg-accent-600/10 hover:bg-accent-600/20 text-accent-400 text-sm font-medium transition-colors duration-150 disabled:opacity-30 disabled:cursor-not-allowed"
      title="Run server-side consolidation to create memories from messages"
    >
      {consolidating ? 'Consolidating...' : 'Consolidate'}
    </button>
  </div>

  <!-- Search bar -->
  <div class="flex items-center gap-3">
    <div class="relative flex-1">
      <input
        type="text"
        placeholder="Search messages..."
        bind:value={searchQuery}
        onkeydown={handleSearchKeydown}
        class="w-full px-4 py-2.5 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 transition-colors duration-150 focus:border-accent-500 focus:outline-none"
      />
    </div>
    <button
      onclick={handleSearch}
      class="px-4 py-2 min-h-11 rounded-lg border border-gray-700 bg-gray-800/50 hover:bg-gray-700 text-gray-300 text-sm font-medium transition-colors duration-150"
    >
      Search
    </button>
    {#if isSearchMode}
      <button
        onclick={clearFilters}
        class="px-3 py-2 min-h-11 rounded-lg border border-gray-700 bg-gray-800/50 hover:bg-gray-700 text-gray-400 text-sm transition-colors duration-150"
      >
        Clear
      </button>
    {/if}
  </div>

  <!-- Filters row -->
  <div class="flex items-center gap-3 flex-wrap">
    <input
      type="text"
      placeholder="Sender..."
      bind:value={senderFilter}
      onchange={applyFilters}
      class="px-3 py-2 min-h-11 w-40 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 focus:border-accent-500 focus:outline-none"
    />
    <input
      type="text"
      placeholder="Chat..."
      bind:value={chatFilter}
      onchange={applyFilters}
      class="px-3 py-2 min-h-11 w-40 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 focus:border-accent-500 focus:outline-none"
    />
    <input
      type="text"
      placeholder="Thread..."
      bind:value={threadFilter}
      onchange={applyFilters}
      class="px-3 py-2 min-h-11 w-40 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 focus:border-accent-500 focus:outline-none"
    />
    <select
      bind:value={platformFilter}
      onchange={applyFilters}
      class="px-3 py-2 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
    >
      <option value="">All platforms</option>
      <option value="telegram">Telegram</option>
      <option value="nostr">Nostr</option>
      <option value="webhook">Webhook</option>
      <option value="cli">CLI</option>
      <option value="nomen">Nomen</option>
    </select>
    <select
      bind:value={sincePreset}
      onchange={applyFilters}
      class="px-3 py-2 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
    >
      <option value="">All time</option>
      <option value="1h">Last hour</option>
      <option value="24h">Last 24h</option>
      <option value="7d">Last 7 days</option>
      <option value="30d">Last 30 days</option>
    </select>
    <label class="flex items-center gap-2 text-sm text-gray-400 cursor-pointer select-none">
      <input
        type="checkbox"
        bind:checked={includeConsolidated}
        onchange={applyFilters}
        class="rounded border-gray-600 bg-gray-800 text-accent-500 focus:ring-accent-500 focus:ring-offset-0"
      />
      Include consolidated
    </label>
  </div>

  <!-- Messages list -->
  {#if loadingList}
    <div class="space-y-2">
      {#each { length: 5 } as _}
        <div class="border border-gray-800 rounded-lg p-4 bg-gray-900/50">
          <div class="flex items-start gap-3">
            <div class="skeleton w-8 h-8 rounded-full"></div>
            <div class="flex-1 space-y-2">
              <div class="flex items-center gap-2">
                <div class="skeleton h-3.5 w-24"></div>
                <div class="skeleton h-3 w-16"></div>
                <div class="skeleton h-3 w-28"></div>
              </div>
              <div class="skeleton h-3.5 w-full max-w-lg"></div>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {:else if messages.length === 0}
    <div class="text-center py-12 text-gray-500">
      {#if isSearchMode}
        No messages match your search
      {:else}
        No messages in the database
      {/if}
    </div>
  {:else}
    <div class="divide-y divide-gray-800/50">
      {#each messages as message (message.source_id || message.id || message.created_at)}
        <button
          type="button"
          class="w-full text-left hover:bg-gray-800/30 transition-colors duration-100 {expandedId === (message.source_id || message.id) ? 'bg-gray-800/20' : ''}"
          onclick={() => toggleContext(message)}
        >
          <MessageItem {message} />
        </button>

        {#if expandedId === message.source_id}
          <div class="bg-gray-900/70 border-l-2 border-accent-500/50 pl-4 py-3">
            {#if loadingContext}
              <div class="text-sm text-gray-500 py-4 text-center">Loading context...</div>
            {:else if contextMessages.length > 0}
              <div class="text-xs text-gray-500 mb-2 font-medium">Surrounding messages</div>
              <div class="space-y-0.5">
                {#each contextMessages as ctxMsg, idx}
                  <div class="{idx === contextTargetIndex ? 'bg-accent-500/10 rounded-md px-2 -mx-2 border-l-2 border-accent-400' : 'opacity-70'}">
                    <MessageItem message={ctxMsg} compact />
                  </div>
                {/each}
              </div>
            {:else}
              <div class="text-sm text-gray-500 py-2">No context available</div>
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  {/if}
</div>
