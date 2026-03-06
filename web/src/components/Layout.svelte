<script lang="ts">
  import Sidebar from './Sidebar.svelte';
  import ProfileButton from './ProfileButton.svelte';
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();
</script>

<div class="flex h-screen overflow-hidden">
  <Sidebar />

  <div class="flex-1 flex flex-col overflow-hidden">
    <header class="h-14 flex items-center justify-between px-6 border-b border-gray-800 bg-gray-950/80 backdrop-blur-sm shrink-0">
      <div class="flex items-center gap-3">
        <a href="/" class="text-lg font-semibold text-gray-100 hover:text-accent-400 transition-colors">Nomen</a>
      </div>
      <ProfileButton />
    </header>

    <main class="flex-1 overflow-y-auto p-6">
      {@render children()}
    </main>
  </div>
</div>
