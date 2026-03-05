<script lang="ts">
  import { apiBaseUrl, relayUrl, embeddingProvider, defaultChannel } from '../lib/stores';

  let saved = $state(false);

  function save() {
    // Stores auto-persist to localStorage, just show confirmation
    saved = true;
    setTimeout(() => saved = false, 2000);
  }
</script>

<div class="max-w-2xl mx-auto space-y-6">
  <div>
    <h2 class="text-2xl font-bold text-gray-100">Settings</h2>
    <p class="text-sm text-gray-500 mt-1">Configure Nomen connection and defaults</p>
  </div>

  <div class="space-y-5">
    <!-- API Base URL -->
    <label class="block">
      <span class="text-sm font-medium text-gray-300">API Base URL</span>
      <span class="text-xs text-gray-600 ml-2">Nomen MCP server endpoint</span>
      <input
        type="text"
        bind:value={$apiBaseUrl}
        class="mt-1 w-full px-4 py-2.5 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
      />
    </label>

    <!-- Relay URL -->
    <label class="block">
      <span class="text-sm font-medium text-gray-300">Relay URL</span>
      <span class="text-xs text-gray-600 ml-2">Nostr relay for sync</span>
      <input
        type="text"
        bind:value={$relayUrl}
        class="mt-1 w-full px-4 py-2.5 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
      />
    </label>

    <!-- Embedding Provider -->
    <label class="block">
      <span class="text-sm font-medium text-gray-300">Embedding Provider</span>
      <select
        bind:value={$embeddingProvider}
        class="mt-1 w-full px-4 py-2.5 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
      >
        <option value="openai">OpenAI</option>
        <option value="local">Local (ONNX)</option>
        <option value="none">None (text-only search)</option>
      </select>
    </label>

    <!-- Default Channel -->
    <label class="block">
      <span class="text-sm font-medium text-gray-300">Default Channel</span>
      <span class="text-xs text-gray-600 ml-2">Used when channel not specified</span>
      <select
        bind:value={$defaultChannel}
        class="mt-1 w-full px-4 py-2.5 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
      >
        <option value="nostr">Nostr</option>
        <option value="telegram">Telegram</option>
      </select>
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
