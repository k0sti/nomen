<script lang="ts">
  import { showProfileModal, profile } from '../lib/stores';

  function close() {
    showProfileModal.set(false);
  }

  function logout() {
    profile.set(null);
    close();
  }

  function copyNpub() {
    if ($profile?.npub) {
      navigator.clipboard.writeText($profile.npub);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    }
  }

  let copied = $state(false);
</script>

{#if $profile}
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50" onclick={close} onkeydown={() => {}}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="bg-gray-900 border border-gray-700 rounded-xl p-6 w-full max-w-sm shadow-2xl" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
    <div class="flex flex-col items-center">
      <!-- Profile image -->
      {#if $profile.picture}
        <img
          src={$profile.picture}
          alt="Profile"
          class="w-24 h-24 rounded-full object-cover border-2 border-gray-600 mb-4"
        />
      {:else}
        <div class="w-24 h-24 rounded-full bg-accent-600 flex items-center justify-center text-4xl font-bold text-white mb-4">
          {($profile.displayName || $profile.name || '?')[0].toUpperCase()}
        </div>
      {/if}

      <!-- Display name -->
      <h2 class="text-xl font-semibold text-gray-100">
        {$profile.displayName || $profile.name || 'Anonymous'}
      </h2>

      <!-- npub -->
      <div class="flex items-center gap-2 mt-2">
        <code class="text-sm text-gray-400 font-mono">{$profile.npubShort}</code>
        <button
          onclick={copyNpub}
          class="text-gray-500 hover:text-gray-300 transition-colors"
          title="Copy full npub"
        >
          {#if copied}
            <svg class="w-4 h-4 text-green-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
            </svg>
          {:else}
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
            </svg>
          {/if}
        </button>
      </div>

      {#if $profile.about}
        <p class="text-sm text-gray-400 mt-3 text-center">{$profile.about}</p>
      {/if}
    </div>

    <div class="mt-6 space-y-2">
      <button
        onclick={logout}
        class="w-full py-2.5 rounded-lg bg-red-900/30 border border-red-800/50 text-red-400 hover:bg-red-900/50 transition-colors text-sm"
      >
        Logout
      </button>
      <button
        onclick={close}
        class="w-full py-2.5 rounded-lg border border-gray-700 text-gray-400 hover:text-gray-200 hover:bg-gray-800 transition-colors text-sm"
      >
        Close
      </button>
    </div>
  </div>
</div>
{/if}
