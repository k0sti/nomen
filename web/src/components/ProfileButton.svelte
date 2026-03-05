<script lang="ts">
  import { profile, isLoggedIn, showLoginModal, showProfileModal } from '../lib/stores';

  function handleClick() {
    if ($isLoggedIn) {
      showProfileModal.set(true);
    } else {
      showLoginModal.set(true);
    }
  }
</script>

<button
  onclick={handleClick}
  class="flex items-center gap-2 px-3 py-1.5 rounded-lg transition-colors hover:bg-gray-800"
>
  {#if $isLoggedIn && $profile}
    {#if $profile.picture}
      <img
        src={$profile.picture}
        alt="Profile"
        class="w-8 h-8 rounded-full object-cover border border-gray-700"
      />
    {:else}
      <div class="w-8 h-8 rounded-full bg-accent-600 flex items-center justify-center text-sm font-medium">
        {($profile.displayName || $profile.name || '?')[0].toUpperCase()}
      </div>
    {/if}
    <span class="text-sm text-gray-300 hidden sm:inline">{$profile.displayName || $profile.name || $profile.npubShort}</span>
  {:else}
    <div class="w-8 h-8 rounded-full bg-gray-700 flex items-center justify-center">
      <svg class="w-5 h-5 text-gray-400" fill="currentColor" viewBox="0 0 20 20">
        <path fill-rule="evenodd" d="M10 9a3 3 0 100-6 3 3 0 000 6zm-7 9a7 7 0 1114 0H3z" clip-rule="evenodd" />
      </svg>
    </div>
    <span class="text-sm text-gray-400 hidden sm:inline">Login</span>
  {/if}
</button>
