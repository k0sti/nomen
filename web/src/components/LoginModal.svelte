<script lang="ts">
  import { showLoginModal, profile, signer, relay } from '../lib/stores';

  const isMobile = typeof navigator !== 'undefined' && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
  import { fetchProfileEvent } from '../lib/nostr';
  import {
    hasNip07,
    loginWithNip07,
    createNostrConnectSession,
    waitForNostrConnect,
    type NostrConnectSession,
  } from '../lib/nostr';
  import QRCode from 'qrcode';

  let error = $state('');
  let loading = $state(false);
  let connectRelay = $state('wss://relay.nsec.app');
  let showConnect = $state(isMobile);
  let connectSession = $state<NostrConnectSession | null>(null);
  let qrSvg = $state('');
  let connectStatus = $state<'idle' | 'waiting' | 'connected'>('idle');
  let abortController = $state<AbortController | null>(null);
  let copied = $state(false);
  let dialogEl = $state<HTMLDialogElement>();

  $effect(() => {
    if ($showLoginModal) {
      dialogEl?.showModal();
    } else {
      dialogEl?.close();
    }
  });

  function close() {
    abortController?.abort();
    abortController = null;
    showLoginModal.set(false);
    error = '';
    showConnect = isMobile;
    connectSession = null;
    qrSvg = '';
    connectStatus = 'idle';
    copied = false;
  }

  function handleCancel(e: Event) {
    e.preventDefault();
    close();
  }

  async function loginExtension() {
    loading = true;
    error = '';
    try {
      const result = await loginWithNip07();
      profile.set(result.profile);
      signer.set(result.signer);
      localStorage.setItem('nomen_login_method', 'nip07');
      // Mirror profile to zooid (best-effort, don't block)
      mirrorProfile(result.profile.pubkey, result.signer);
      close();
    } catch (e: any) {
      error = e.message || 'Failed to login with extension';
    } finally {
      loading = false;
    }
  }

  async function startNostrConnect() {
    showConnect = !showConnect;
    if (!showConnect) {
      abortController?.abort();
      abortController = null;
      connectSession = null;
      qrSvg = '';
      connectStatus = 'idle';
      return;
    }

    error = '';
    connectStatus = 'idle';
    await generateConnectURI();
  }

  async function generateConnectURI() {
    try {
      // Abort any previous session
      abortController?.abort();

      const session = createNostrConnectSession(connectRelay);
      connectSession = session;

      // Generate QR code SVG
      qrSvg = await QRCode.toString(session.uri, {
        type: 'svg',
        margin: 1,
        color: { dark: '#000000', light: '#ffffff' },
      });

      // Start waiting for connection
      connectStatus = 'waiting';
      const ac = new AbortController();
      abortController = ac;

      const result = await waitForNostrConnect(session);
      connectStatus = 'connected';
      profile.set(result.profile);
      signer.set(result.signer);
      localStorage.setItem('nomen_login_method', 'nip46');
      localStorage.setItem('nomen_nip46_relay', session.relay);
      // Mirror profile to zooid (best-effort, don't block)
      mirrorProfile(result.profile.pubkey, result.signer);

      // Brief pause to show success state
      setTimeout(close, 500);
    } catch (e: any) {
      if (e.name === 'AbortError') return;
      connectStatus = 'idle';
      error = e.message || 'Nostr Connect failed';
    }
  }

  async function copyURI() {
    if (!connectSession) return;
    try {
      await navigator.clipboard.writeText(connectSession.uri);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      error = 'Failed to copy to clipboard';
    }
  }

  async function mirrorProfile(pubkey: string, signerInstance: any) {
    try {
      const event = await fetchProfileEvent(pubkey);
      if (!event) return;
      await $relay.connect();
      await $relay.authenticate(signerInstance);
      await $relay.publishEvent(event);
    } catch {
      // Best-effort
    }
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
      <!-- NIP-07 Extension (hide on mobile if no extension) -->
      {#if hasNip07() || !isMobile}
        <button
          onclick={loginExtension}
          disabled={loading || !hasNip07()}
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
      {/if}

      <!-- Nostr Connect / NIP-46 (primary on mobile) -->
      <button
        onclick={startNostrConnect}
        class="w-full flex items-center gap-3 p-4 min-h-14 rounded-lg border transition-colors duration-150
          {isMobile
            ? 'border-accent-600/50 bg-accent-600/10 hover:bg-accent-600/20 active:bg-accent-600/30 text-gray-100'
            : 'border-gray-700 bg-gray-800/50 hover:bg-gray-800 active:bg-gray-700 text-gray-100'}"
      >
        <svg class="w-5 h-5 shrink-0" fill="currentColor" viewBox="0 0 20 20"><path fill-rule="evenodd" d="M12.586 4.586a2 2 0 112.828 2.828l-3 3a2 2 0 01-2.828 0 1 1 0 00-1.414 1.414 4 4 0 005.656 0l3-3a4 4 0 00-5.656-5.656l-1.5 1.5a1 1 0 101.414 1.414l1.5-1.5zm-5 5a2 2 0 012.828 0 1 1 0 101.414-1.414 4 4 0 00-5.656 0l-3 3a4 4 0 105.656 5.656l1.5-1.5a1 1 0 10-1.414-1.414l-1.5 1.5a2 2 0 11-2.828-2.828l3-3z" clip-rule="evenodd" /></svg>
        <div class="text-left">
          <div class="font-medium">{isMobile ? 'Login with Amber' : 'Nostr Connect'}</div>
          <div class="text-xs text-gray-400">{isMobile ? 'Scan QR or paste URI in Amber' : 'NIP-46 remote signer / Amber'}</div>
        </div>
        <svg class="ml-auto w-4 h-4 text-gray-500 transition-transform duration-150 {showConnect ? 'rotate-180' : ''}" fill="currentColor" viewBox="0 0 20 20"><path fill-rule="evenodd" d="M5.293 7.293a1 1 0 011.414 0L10 10.586l3.293-3.293a1 1 0 111.414 1.414l-4 4a1 1 0 01-1.414 0l-4-4a1 1 0 010-1.414z" clip-rule="evenodd" /></svg>
      </button>

      {#if showConnect}
        <div class="p-4 rounded-lg border border-gray-700 bg-gray-800/30 space-y-3">
          <!-- Relay selector -->
          <label class="block">
            <span class="text-xs text-gray-400">Relay</span>
            <input
              type="text"
              bind:value={connectRelay}
              class="mt-1 w-full px-3 py-2.5 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 transition-colors duration-150 focus:border-accent-500"
            />
          </label>

          {#if connectSession && qrSvg}
            <!-- QR Code -->
            <div class="flex items-center justify-center p-4 bg-white rounded-lg">
              <div class="w-48 h-48">
                {@html qrSvg}
              </div>
            </div>

            <!-- Status -->
            {#if connectStatus === 'waiting'}
              <div class="flex items-center gap-2 text-sm text-amber-400">
                <span class="inline-block w-2 h-2 rounded-full bg-amber-400 animate-pulse"></span>
                Waiting for signer to connect...
              </div>
            {:else if connectStatus === 'connected'}
              <div class="flex items-center gap-2 text-sm text-green-400">
                <span class="inline-block w-2 h-2 rounded-full bg-green-400"></span>
                Connected!
              </div>
            {/if}

            <!-- URI + Copy button -->
            <div class="flex gap-2">
              <input
                type="text"
                readonly
                value={connectSession.uri}
                class="flex-1 px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-xs text-gray-400 font-mono truncate"
              />
              <button
                onclick={copyURI}
                class="px-3 py-2 rounded-lg border border-gray-700 bg-gray-800 hover:bg-gray-700 text-sm text-gray-300 transition-colors duration-150 shrink-0"
              >
                {copied ? 'Copied!' : 'Copy'}
              </button>
            </div>

            <!-- Open in Amber (mobile link) -->
            <a
              href={connectSession.uri}
              class="flex items-center justify-center gap-2 w-full p-3 min-h-11 rounded-lg border border-amber-600/50 bg-amber-600/10 hover:bg-amber-600/20 active:bg-amber-600/30 text-amber-300 text-sm font-medium transition-colors duration-150"
            >
              <svg class="w-4 h-4 shrink-0" fill="currentColor" viewBox="0 0 20 20"><path fill-rule="evenodd" d="M7 2a2 2 0 00-2 2v12a2 2 0 002 2h6a2 2 0 002-2V4a2 2 0 00-2-2H7zm3 14a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd" /></svg>
              Open in Amber
            </a>
          {:else}
            <!-- Generate button -->
            <button
              onclick={generateConnectURI}
              class="w-full p-3 min-h-11 rounded-lg border border-accent-600/50 bg-accent-600/10 hover:bg-accent-600/20 active:bg-accent-600/30 text-gray-100 text-sm font-medium transition-colors duration-150"
            >
              Generate QR Code
            </button>
          {/if}

          <p class="text-xs text-gray-500">
            Scan the QR code with your Nostr signer app, or tap "Open in Amber" on Android.
          </p>
        </div>
      {/if}
    </div>
  </div>
</dialog>
