<script lang="ts">
  import type { Message } from '../lib/api';

  let { message }: { message: Message } = $props();

  function formatTime(iso: string): string {
    return new Date(iso).toLocaleString('en-US', {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
    });
  }

  const sourceIcon: Record<string, string> = {
    telegram: 'T',
    nostr: 'N',
    webhook: 'W',
    nomen: 'A',
  };

  const sourceColor: Record<string, string> = {
    telegram: 'bg-blue-600',
    nostr: 'bg-purple-600',
    webhook: 'bg-amber-600',
    nomen: 'bg-emerald-600',
  };
</script>

<div class="flex gap-3 py-3 {message.consolidated ? 'opacity-60' : ''}">
  <div class="shrink-0 w-8 h-8 rounded-full {sourceColor[message.source] || 'bg-gray-600'} flex items-center justify-center text-xs font-bold text-white">
    {sourceIcon[message.source] || '?'}
  </div>
  <div class="min-w-0 flex-1">
    <div class="flex items-center gap-2 text-xs">
      <span class="font-medium text-gray-300">{message.sender}</span>
      {#if message.channel}
        <span class="text-gray-600">#{message.channel}</span>
      {/if}
      <span class="text-gray-600">{formatTime(message.created_at)}</span>
      {#if message.consolidated}
        <span class="px-1.5 py-0.5 rounded bg-emerald-900/30 text-emerald-500 text-xs">consolidated</span>
      {/if}
    </div>
    <p class="text-sm text-gray-300 mt-0.5">{message.content}</p>
  </div>
</div>
