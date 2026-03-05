<script lang="ts">
  import { showLoginModal, profile, mockMode } from '../lib/stores';
  import { hasNip07, hasAmber, loginWithNip07 } from '../lib/nostr';

  let error = $state('');
  let loading = $state(false);
  let connectRelay = $state('wss://relay.nsec.app');
  let showConnect = $state(false);

  function close() {
    showLoginModal.set(false);
    error = '';
    showConnect = false;
  }

  async function loginExtension() {
    loading = true;
    error = '';
    try {
      const p = await loginWithNip07();
      profile.set(p);
      close();
    } catch (e: any) {
      error = e.message || 'Failed to login with extension';
    } finally {
      loading = false;
    }
  }

  function loginAmber() {
    error = 'Amber login not yet implemented — requires clipboard-based signing flow';
  }

  function toggleConnect() {
    showConnect = !showConnect;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50" onclick={close} onkeydown={() => {}}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="bg-gray-900 border border-gray-700 rounded-xl p-6 w-full max-w-md shadow-2xl" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
    <div class="flex items-center justify-between mb-6">
      <h2 class="text-xl font-semibold text-gray-100">Login to Nomen</h2>
      <button onclick={close} class="text-gray-500 hover:text-gray-300 text-xl">&times;</button>
    </div>

    {#if error}
      <div class="mb-4 p-3 rounded-lg bg-red-900/30 border border-red-800/50 text-red-400 text-sm">
        {error}
      </div>
    {/if}

    <div class="space-y-3">
      <!-- NIP-07 Extension -->
      <button
        onclick={loginExtension}
        disabled={loading}
        class="w-full flex items-center gap-3 p-4 rounded-lg border transition-colors
          {hasNip07()
            ? 'border-accent-600/50 bg-accent-600/10 hover:bg-accent-600/20 text-gray-100'
            : 'border-gray-700 bg-gray-800/50 text-gray-500 cursor-not-allowed'}"
      >
        <span class="text-xl">🔑</span>
        <div class="text-left">
          <div class="font-medium">Login with Extension</div>
          <div class="text-xs {hasNip07() ? 'text-gray-400' : 'text-gray-600'}">
            {hasNip07() ? 'NIP-07 extension detected' : 'No NIP-07 extension found'}
          </div>
        </div>
        {#if loading}
          <span class="ml-auto text-sm text-gray-400">...</span>
        {/if}
      </button>

      <!-- Amber -->
      <button
        onclick={loginAmber}
        class="w-full flex items-center gap-3 p-4 rounded-lg border transition-colors
          {hasAmber()
            ? 'border-amber-600/50 bg-amber-600/10 hover:bg-amber-600/20 text-gray-100'
            : 'border-gray-700 bg-gray-800/50 text-gray-500 cursor-not-allowed'}"
      >
        <span class="text-xl">📱</span>
        <div class="text-left">
          <div class="font-medium">Login with Amber</div>
          <div class="text-xs {hasAmber() ? 'text-gray-400' : 'text-gray-600'}">
            {hasAmber() ? 'Amber detected' : 'Amber not available'}
          </div>
        </div>
      </button>

      <!-- Nostr Connect -->
      <button
        onclick={toggleConnect}
        class="w-full flex items-center gap-3 p-4 rounded-lg border border-gray-700 bg-gray-800/50 hover:bg-gray-800 text-gray-100 transition-colors"
      >
        <span class="text-xl">🔗</span>
        <div class="text-left">
          <div class="font-medium">Nostr Connect</div>
          <div class="text-xs text-gray-400">NIP-46 remote signer</div>
        </div>
        <span class="ml-auto text-gray-500">{showConnect ? '▲' : '▼'}</span>
      </button>

      {#if showConnect}
        <div class="p-4 rounded-lg border border-gray-700 bg-gray-800/30 space-y-3">
          <label class="block">
            <span class="text-xs text-gray-400">Relay</span>
            <input
              type="text"
              bind:value={connectRelay}
              class="mt-1 w-full px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500 focus:outline-none"
            />
          </label>
          <div class="flex items-center justify-center p-6 bg-white rounded-lg">
            <div class="text-center text-gray-800">
              <div class="text-4xl mb-2">📷</div>
              <div class="text-sm font-mono break-all">nostrconnect://...</div>
              <div class="text-xs text-gray-500 mt-1">QR code placeholder — scan with signer app</div>
            </div>
          </div>
          <p class="text-xs text-gray-500">
            Open your Nostr signer app and scan the QR code, or paste the connection URL.
          </p>
        </div>
      {/if}
    </div>

    {#if $mockMode}
      <div class="mt-4 pt-4 border-t border-gray-800">
        <button
          onclick={() => {
            profile.set({
              pubkey: 'a'.repeat(64),
              npub: 'npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqz4s3f5',
              npubShort: 'npub1qqqqqqqqq...s3f5',
              name: 'Demo User',
              displayName: 'Demo User',
              picture: undefined,
            });
            close();
          }}
          class="w-full p-3 rounded-lg border border-dashed border-gray-700 text-gray-400 hover:text-gray-200 hover:border-gray-500 text-sm transition-colors"
        >
          Login as Demo User (mock mode)
        </button>
      </div>
    {/if}
  </div>
</div>
