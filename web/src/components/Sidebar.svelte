<script lang="ts">
  import { currentPage } from '../lib/stores';
  import type { Page } from '../lib/stores';

  const navItems: { page: Page; label: string; icon: string }[] = [
    { page: 'memories', label: 'Memories', icon: '🧠' },
    { page: 'search', label: 'Search', icon: '🔍' },
    { page: 'messages', label: 'Messages', icon: '💬' },
    { page: 'groups', label: 'Groups', icon: '👥' },
    { page: 'settings', label: 'Settings', icon: '⚙️' },
  ];
</script>

<nav class="w-56 bg-gray-900 border-r border-gray-800 flex flex-col shrink-0">
  <div class="p-4 border-b border-gray-800">
    <div class="flex items-center gap-2">
      <span class="text-2xl">🧠</span>
      <span class="font-bold text-lg text-gray-100">Nomen</span>
    </div>
    <p class="text-xs text-gray-500 mt-1">Agent Memory System</p>
  </div>

  <div class="flex-1 py-2">
    {#each navItems as item}
      <button
        class="w-full flex items-center gap-3 px-4 py-2.5 text-sm transition-colors {$currentPage === item.page
          ? 'bg-accent-500/10 text-accent-400 border-r-2 border-accent-500'
          : 'text-gray-400 hover:text-gray-200 hover:bg-gray-800/50'}"
        onclick={() => currentPage.set(item.page)}
      >
        <span class="text-base">{item.icon}</span>
        <span>{item.label}</span>
      </button>
    {/each}
  </div>

  <div class="p-4 border-t border-gray-800 text-xs text-gray-600">
    Nomen v0.1.0
  </div>
</nav>
