<script lang="ts">
  import { relayUrl, apiBaseUrl, relay, relayConnected } from '../lib/stores';
  import { NomenRelay } from '../lib/relay';

  let saved = $state(false);
  let nip46Relays = $state(localStorage.getItem('nomen:nip46Relays') || 'wss://relay.nsec.app');

  function save() {
    // Recreate relay instance if URL changed
    if ($relay.url !== $relayUrl) {
      $relay.disconnect();
      const newRelay = new NomenRelay($relayUrl);
      newRelay.onConnectionChange = (connected) => relayConnected.set(connected);
      relay.set(newRelay);
      relayConnected.set(false);
    }
    // Save NIP-46 relay list
    localStorage.setItem('nomen:nip46Relays', nip46Relays.trim());
    saved = true;
    setTimeout(() => saved = false, 2000);
  }
</script>

<div class="max-w-2xl mx-auto space-y-6">
  <div>
    <h2 class="text-2xl font-bold text-gray-100">Settings</h2>
    <p class="text-sm text-gray-500 mt-1">Configure Nomen relay and API connections</p>
  </div>

  <div class="space-y-5">
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
  </div>

  <div class="flex items-center gap-3 pt-2">
    <button
      onclick={save}
      class="px-5 py-2.5 rounded-lg text-sm font-medium bg-accent-600 text-white hover:bg-accent-500 transition-colors"
    >
      Save Settings
    </button>
    {#if saved}
      <span class="text-sm text-emerald-400">Saved</span>
    {/if}
  </div>
</div>
