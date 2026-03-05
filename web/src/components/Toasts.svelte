<script lang="ts">
  import { toasts } from '../lib/stores';
</script>

{#if $toasts.length > 0}
  <div class="fixed bottom-4 right-4 z-50 space-y-2 max-w-sm">
    {#each $toasts as toast (toast.id)}
      <div
        class="px-4 py-3 rounded-lg text-sm shadow-lg border {toast.type === 'error'
          ? 'bg-red-900/90 border-red-800/50 text-red-200'
          : 'bg-gray-800/90 border-gray-700/50 text-gray-200'}"
      >
        {toast.message}
      </div>
    {/each}
  </div>
{/if}
