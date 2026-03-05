<script lang="ts">
  import { showLoginModal, profile, mockMode } from '../lib/stores';
  import { hasNip07, hasAmber, loginWithNip07 } from '../lib/nostr';

  let error = $state('');
  let loading = $state(false);
  let connectRelay = $state('wss://relay.nsec.app');
  let showConnect = $state(false);
  let dialogEl = $state<HTMLDialogElement>();

  $effect(() => {
    if ($showLoginModal) {
      dialogEl?.showModal();
    } else {
      dialogEl?.close();
    }
  });

  function close() {
    showLoginModal.set(false);
    error = '';
    showConnect = false;
  }

  function handleCancel(e: Event) {
    e.preventDefault();
    close();
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

<dialog
  bind:this={dialogEl}
  oncancel={handleCancel}
  class="w-full max-w-md"
>
  <div class="bg-gray-900 border border-gray-700 rounded-xl p-6 w-full shadow-2xl">
    <div class="flex items-center justify-between mb-6">
      <h2 class="text-xl font-semibold text-gray-100">Login to Nomen</h2>
      <button
        onclick={close}
        class="w-11 h-11 flex items-center justify-center rounded-lg text-gray-500 hover:text-gray-300 hover:bg-gray-800 active:bg-gray-700 text-xl transition-colors duration-150"
        aria-label="Close"
      >&times;</button>
    </div>

    {#if error}
      <div class="mb-4 p-3 rounded-lg bg-red-900/30 border border-red-800/50 text-red-400 text-sm" role="alert">
        {error}
      </div>
    {/if}

    <div class="space-y-3">
      <!-- NIP-07 Extension -->
      <button
        onclick={loginExtension}
        disabled={loading}
        class="w-full flex items-center gap-3 p-4 min-h-14 rounded-lg border transition-colors duration-150
          {hasNip07()
            ? 'border-accent-600/50 bg-accent-600/10 hover:bg-accent-600/20 active:bg-accent-600/30 text-gray-100'
            : 'border-gray-700 bg-gray-800/50 text-gray-500 cursor-not-allowed'}"
      >
        <svg class="w-5 h-5 shrink-0" fill="currentColor" viewBox="0 0 20 20"><path fill-rule="evenodd" d="M18 8a6 6 0 01-7.743 5.743L10 14l-1 1-1 1H6v2H2v-4l4.257-4.257A6 6 0 1118 8zm-6-4a1 1 0 100 2 2 2 0 012 2 1 1 0 102 0 4 4 0 00-4-4z" clip-rule="evenodd" /></svg>
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
        class="w-full flex items-center gap-3 p-4 min-h-14 rounded-lg border transition-colors duration-150
          {hasAmber()
            ? 'border-amber-600/50 bg-amber-600/10 hover:bg-amber-600/20 active:bg-amber-600/30 text-gray-100'
            : 'border-gray-700 bg-gray-800/50 text-gray-500 cursor-not-allowed'}"
      >
        <svg class="w-5 h-5 shrink-0" fill="currentColor" viewBox="0 0 20 20"><path fill-rule="evenodd" d="M7 2a2 2 0 00-2 2v12a2 2 0 002 2h6a2 2 0 002-2V4a2 2 0 00-2-2H7zm3 14a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd" /></svg>
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
        class="w-full flex items-center gap-3 p-4 min-h-14 rounded-lg border border-gray-700 bg-gray-800/50 hover:bg-gray-800 active:bg-gray-700 text-gray-100 transition-colors duration-150"
      >
        <svg class="w-5 h-5 shrink-0" fill="currentColor" viewBox="0 0 20 20"><path fill-rule="evenodd" d="M12.586 4.586a2 2 0 112.828 2.828l-3 3a2 2 0 01-2.828 0 1 1 0 00-1.414 1.414 4 4 0 005.656 0l3-3a4 4 0 00-5.656-5.656l-1.5 1.5a1 1 0 101.414 1.414l1.5-1.5zm-5 5a2 2 0 012.828 0 1 1 0 101.414-1.414 4 4 0 00-5.656 0l-3 3a4 4 0 105.656 5.656l1.5-1.5a1 1 0 10-1.414-1.414l-1.5 1.5a2 2 0 11-2.828-2.828l3-3z" clip-rule="evenodd" /></svg>
        <div class="text-left">
          <div class="font-medium">Nostr Connect</div>
          <div class="text-xs text-gray-400">NIP-46 remote signer</div>
        </div>
        <svg class="ml-auto w-4 h-4 text-gray-500 transition-transform duration-150 {showConnect ? 'rotate-180' : ''}" fill="currentColor" viewBox="0 0 20 20"><path fill-rule="evenodd" d="M5.293 7.293a1 1 0 011.414 0L10 10.586l3.293-3.293a1 1 0 111.414 1.414l-4 4a1 1 0 01-1.414 0l-4-4a1 1 0 010-1.414z" clip-rule="evenodd" /></svg>
      </button>

      {#if showConnect}
        <div class="p-4 rounded-lg border border-gray-700 bg-gray-800/30 space-y-3">
          <label class="block">
            <span class="text-xs text-gray-400">Relay</span>
            <input
              type="text"
              bind:value={connectRelay}
              class="mt-1 w-full px-3 py-2.5 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 transition-colors duration-150 focus:border-accent-500"
            />
          </label>
          <div class="flex items-center justify-center p-6 bg-white rounded-lg">
            <div class="text-center text-gray-800">
              <svg class="w-10 h-10 mx-auto mb-2 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 4v1m6 11h2m-6 0h-2v4m0-11v3m0 0h.01M12 12h4.01M16 20h4M4 12h4m12 0h.01M5 8h2a1 1 0 001-1V5a1 1 0 00-1-1H5a1 1 0 00-1 1v2a1 1 0 001 1zm12 0h2a1 1 0 001-1V5a1 1 0 00-1-1h-2a1 1 0 00-1 1v2a1 1 0 001 1zM5 20h2a1 1 0 001-1v-2a1 1 0 00-1-1H5a1 1 0 00-1 1v2a1 1 0 001 1z" /></svg>
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
          class="w-full p-3 min-h-11 rounded-lg border border-dashed border-gray-700 text-gray-400 hover:text-gray-200 hover:border-gray-500 active:bg-gray-800 text-sm transition-colors duration-150"
        >
          Login as Demo User (mock mode)
        </button>
      </div>
    {/if}
  </div>
</dialog>
