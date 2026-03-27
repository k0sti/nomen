<script lang="ts">
  import { relayUrl, apiBaseUrl, relay, relayConnected, api, showError, showInfo } from '../lib/stores';
  import { NomenRelay } from '../lib/relay';
  import type { SystemStats, PruneResult, NomenConfig } from '../lib/api';

  let saved = $state(false);
  let nip46Relays = $state(localStorage.getItem('nomen:nip46Relays') || 'wss://relay.nsec.app');

  // Stats
  let stats = $state<SystemStats | null>(null);
  let statsLoading = $state(false);

  // Consolidation
  let consolidating = $state(false);

  // Pruning
  let pruneDays = $state(90);
  let pruneDryRun = $state(true);
  let pruning = $state(false);
  let pruneResult = $state<PruneResult | null>(null);

  // Config
  let config = $state<NomenConfig | null>(null);
  let configExpanded = $state(false);
  let configLoading = $state(false);
  let reloading = $state(false);

  async function loadStats() {
    statsLoading = true;
    try {
      stats = await $api.getStats();
    } catch (err: any) {
      showError('Failed to load stats: ' + (err.message || err));
    } finally {
      statsLoading = false;
    }
  }

  async function runConsolidation() {
    consolidating = true;
    try {
      const report = await $api.consolidate({});
      showInfo(`Consolidated ${report.messages_processed} messages into ${report.memories_created} memories`);
      await loadStats();
    } catch (err: any) {
      showError('Consolidation failed: ' + (err.message || err));
    } finally {
      consolidating = false;
    }
  }

  async function runPrune() {
    pruning = true;
    pruneResult = null;
    try {
      pruneResult = await $api.prune(pruneDays, pruneDryRun);
      if (pruneDryRun) {
        showInfo(`Dry run: would prune ${pruneResult.memories_pruned} memories`);
      } else {
        showInfo(`Pruned ${pruneResult.memories_pruned} memories`);
        await loadStats();
      }
    } catch (err: any) {
      showError('Prune failed: ' + (err.message || err));
    } finally {
      pruning = false;
    }
  }

  async function loadConfig() {
    configLoading = true;
    try {
      config = await $api.getConfig();
    } catch (err: any) {
      showError('Failed to load config: ' + (err.message || err));
    } finally {
      configLoading = false;
    }
  }

  async function reloadConfig() {
    reloading = true;
    try {
      config = await $api.reloadConfig();
      showInfo('Config reloaded from disk');
    } catch (err: any) {
      showError('Config reload failed: ' + (err.message || err));
    } finally {
      reloading = false;
    }
  }

  function saveConnection() {
    if ($relay.url !== $relayUrl) {
      $relay.disconnect();
      const newRelay = new NomenRelay($relayUrl);
      newRelay.onConnectionChange = (connected) => relayConnected.set(connected);
      relay.set(newRelay);
      relayConnected.set(false);
    }
    localStorage.setItem('nomen:nip46Relays', nip46Relays.trim());
    saved = true;
    setTimeout(() => saved = false, 2000);
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return bytes + ' B';
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
    return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
  }

  function formatTime(ts: string | null): string {
    if (!ts) return 'Never';
    try {
      return new Date(ts).toLocaleString();
    } catch {
      return ts;
    }
  }

  // Load data on mount
  $effect(() => {
    loadStats();
    loadConfig();
  });
</script>

<div class="max-w-3xl mx-auto space-y-6">
  <div>
    <h2 class="text-2xl font-bold text-gray-100">Settings</h2>
    <p class="text-sm text-gray-500 mt-1">Operations dashboard and configuration</p>
  </div>

  <!-- Section 1: Status Overview -->
  <div class="bg-gray-800/50 border border-gray-700 rounded-lg p-5">
    <div class="flex items-center justify-between mb-4">
      <h3 class="text-lg font-semibold text-gray-200">Status Overview</h3>
      <button
        onclick={loadStats}
        disabled={statsLoading}
        class="text-xs text-gray-400 hover:text-gray-200 transition-colors"
      >
        {statsLoading ? 'Loading...' : 'Refresh'}
      </button>
    </div>

    {#if stats}
      <div class="grid grid-cols-2 sm:grid-cols-4 gap-4 mb-4">
        <div class="text-center">
          <div class="text-2xl font-bold text-accent-400">{stats.total_memories}</div>
          <div class="text-xs text-gray-500">Memories</div>
        </div>
        <div class="text-center">
          <div class="text-2xl font-bold text-yellow-400">{stats.ephemeral_messages}</div>
          <div class="text-xs text-gray-500">Pending Messages</div>
        </div>
        <div class="text-center">
          <div class="text-2xl font-bold text-blue-400">{stats.entities}</div>
          <div class="text-xs text-gray-500">Entities</div>
        </div>
        <div class="text-center">
          <div class="text-2xl font-bold text-emerald-400">{stats.groups}</div>
          <div class="text-xs text-gray-500">Groups</div>
        </div>
      </div>
      <div class="flex flex-wrap gap-4 text-xs text-gray-500">
        <span>Last consolidation: {formatTime(stats.last_consolidation)}</span>
        <span>Last prune: {formatTime(stats.last_prune)}</span>
        <span>DB size: {formatBytes(stats.db_size_bytes)}</span>
      </div>
    {:else if statsLoading}
      <div class="text-sm text-gray-500">Loading stats...</div>
    {:else}
      <div class="text-sm text-gray-500">Unable to load stats</div>
    {/if}
  </div>

  <!-- Section 2: Consolidation -->
  <div class="bg-gray-800/50 border border-gray-700 rounded-lg p-5">
    <div class="flex items-center justify-between mb-4">
      <h3 class="text-lg font-semibold text-gray-200">Consolidation</h3>
    </div>

    <p class="text-sm text-gray-400 mb-4">Run consolidation to merge pending ephemeral messages into named memories.</p>

    <button
      onclick={runConsolidation}
      disabled={consolidating}
      class="px-4 py-2 rounded-lg text-sm font-medium bg-accent-600 text-white hover:bg-accent-500 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
    >
      {consolidating ? 'Running...' : 'Run Now'}
    </button>
  </div>

  <!-- Section 3: Pruning -->
  <div class="bg-gray-800/50 border border-gray-700 rounded-lg p-5">
    <h3 class="text-lg font-semibold text-gray-200 mb-4">Pruning</h3>

    <div class="flex flex-wrap items-end gap-4 mb-4">
      <label class="block">
        <span class="text-xs text-gray-400">Days</span>
        <input
          type="number"
          bind:value={pruneDays}
          min="1"
          class="mt-1 w-24 px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
        />
      </label>

      <label class="flex items-center gap-2 pb-2">
        <input
          type="checkbox"
          bind:checked={pruneDryRun}
          class="rounded border-gray-600 bg-gray-900 text-accent-500 focus:ring-accent-500"
        />
        <span class="text-sm text-gray-400">Dry run</span>
      </label>

      <button
        onclick={runPrune}
        disabled={pruning}
        class="px-4 py-2 rounded-lg text-sm font-medium {pruneDryRun ? 'bg-yellow-600 hover:bg-yellow-500' : 'bg-red-600 hover:bg-red-500'} text-white transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {pruning ? 'Pruning...' : pruneDryRun ? 'Preview Prune' : 'Prune'}
      </button>
    </div>

    {#if pruneResult}
      <div class="text-sm text-gray-400 mb-2">
        {pruneResult.dry_run ? 'Would prune' : 'Pruned'}: {pruneResult.memories_pruned} memories
      </div>
      {#if pruneResult.pruned.length > 0}
        <div class="max-h-48 overflow-y-auto border border-gray-700 rounded-lg">
          <table class="w-full text-xs">
            <thead class="bg-gray-900/50 sticky top-0">
              <tr class="text-gray-500">
                <th class="text-left px-3 py-1.5">Topic</th>
                <th class="text-right px-3 py-1.5">Confidence</th>
                <th class="text-right px-3 py-1.5">Age (days)</th>
              </tr>
            </thead>
            <tbody>
              {#each pruneResult.pruned as item}
                <tr class="border-t border-gray-700/50 text-gray-300">
                  <td class="px-3 py-1.5 truncate max-w-[200px]">{item.topic}</td>
                  <td class="px-3 py-1.5 text-right">{item.age_days}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  </div>

  <!-- Section 4: Connection -->
  <div class="bg-gray-800/50 border border-gray-700 rounded-lg p-5">
    <h3 class="text-lg font-semibold text-gray-200 mb-4">Connection</h3>

    <div class="space-y-4">
      <label class="block">
        <span class="text-sm font-medium text-gray-300">Relay URL</span>
        <span class="text-xs text-gray-600 ml-2">Nostr relay for direct event access</span>
        <input
          type="text"
          bind:value={$relayUrl}
          class="mt-1 w-full px-4 py-2.5 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
        />
      </label>

      <label class="block">
        <span class="text-sm font-medium text-gray-300">API Base URL</span>
        <span class="text-xs text-gray-600 ml-2">Nomen server for search, entities, consolidation</span>
        <input
          type="text"
          bind:value={$apiBaseUrl}
          class="mt-1 w-full px-4 py-2.5 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
        />
      </label>

      <label class="block">
        <span class="text-sm font-medium text-gray-300">NIP-46 Relay</span>
        <span class="text-xs text-gray-600 ml-2">Relay for Nostr Connect / remote signing</span>
        <input
          type="text"
          bind:value={nip46Relays}
          placeholder="wss://relay.nsec.app"
          class="mt-1 w-full px-4 py-2.5 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
        />
      </label>

      <div class="flex items-center gap-3">
        <button
          onclick={saveConnection}
          class="px-5 py-2.5 rounded-lg text-sm font-medium bg-accent-600 text-white hover:bg-accent-500 transition-colors"
        >
          Save Connection
        </button>
        {#if saved}
          <span class="text-sm text-emerald-400">Saved</span>
        {/if}
        <span class="ml-auto text-xs {$relayConnected ? 'text-emerald-400' : 'text-gray-500'}">
          {$relayConnected ? 'Connected' : 'Disconnected'}
        </span>
      </div>
    </div>
  </div>

  <!-- Section 5: Configuration -->
  <div class="bg-gray-800/50 border border-gray-700 rounded-lg p-5">
    <div class="flex items-center justify-between mb-4">
      <button
        onclick={() => configExpanded = !configExpanded}
        class="text-lg font-semibold text-gray-200 flex items-center gap-2 hover:text-gray-100 transition-colors"
      >
        <span class="text-sm text-gray-500 transition-transform {configExpanded ? 'rotate-90' : ''}">&rsaquo;</span>
        Configuration
      </button>
      <button
        onclick={reloadConfig}
        disabled={reloading}
        class="px-3 py-1.5 rounded-lg text-xs font-medium bg-gray-700 text-gray-300 hover:bg-gray-600 transition-colors disabled:opacity-50"
      >
        {reloading ? 'Reloading...' : 'Reload Config'}
      </button>
    </div>

    {#if config}
      <div class="text-xs text-gray-500 mb-3">{config.config_path}</div>
    {/if}

    {#if configExpanded}
      {#if config}
        <pre class="bg-gray-900 border border-gray-700 rounded-lg p-4 text-xs text-gray-300 overflow-x-auto max-h-96">{JSON.stringify(config, null, 2)}</pre>
      {:else if configLoading}
        <div class="text-sm text-gray-500">Loading...</div>
      {:else}
        <div class="text-sm text-gray-500">Unable to load config</div>
      {/if}
    {/if}
  </div>
</div>
