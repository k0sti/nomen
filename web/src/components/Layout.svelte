<script lang="ts">
  import Sidebar from './Sidebar.svelte';
  import ProfileButton from './ProfileButton.svelte';
  import { mockMode } from '../lib/stores';
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();
</script>

<div class="flex h-screen overflow-hidden">
  <Sidebar />

  <div class="flex-1 flex flex-col overflow-hidden">
    <!-- Top bar -->
    <header class="h-14 flex items-center justify-between px-6 border-b border-gray-800 bg-gray-950/80 backdrop-blur-sm shrink-0">
      <div class="flex items-center gap-3">
        <h1 class="text-lg font-semibold text-gray-100">Nomen</h1>
        {#if $mockMode}
          <span class="text-xs px-2 py-0.5 rounded-full bg-amber-900/50 text-amber-400 border border-amber-800/50">
            Mock Data
          </span>
        {/if}
      </div>
      <ProfileButton />
    </header>

    <!-- Main content -->
    <main class="flex-1 overflow-y-auto p-6">
      {@render children()}
    </main>
  </div>
</div>
