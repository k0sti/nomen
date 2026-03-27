<script lang="ts">
  import type { Message } from '../lib/api';

  let { message, compact = false }: { message: Message; compact?: boolean } = $props();

  function formatTime(iso: string): string {
    if (!iso) return '';
    return new Date(iso).toLocaleString('en-US', {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
    });
  }

  const sourceIcon: Record<string, string> = {
    telegram: 'T',
    nostr: 'N',
    webhook: 'W',
    nomen: 'A',
    cli: 'C',
  };

  const sourceColor: Record<string, string> = {
    telegram: 'bg-blue-600',
    nostr: 'bg-purple-600',
    webhook: 'bg-amber-600',
    nomen: 'bg-emerald-600',
    cli: 'bg-gray-600',
  };

  const truncated = $derived(
    compact && message.content.length > 120
      ? message.content.slice(0, 120) + '...'
      : message.content
  );

  const containerLabel = $derived.by(() => {
    if (message.thread) return `${message.chat || 'chat'} / ${message.thread}`;
    if (message.chat) return message.chat;
    if (message.community) return message.community;
    return message.channel;
  });
</script>

{#if compact}
  <div class="flex gap-2 py-1.5 {message.consolidated ? 'opacity-60' : ''}">
    <div class="shrink-0 w-5 h-5 rounded-full {sourceColor[message.source] || 'bg-gray-600'} flex items-center justify-center text-[10px] font-bold text-white">
      {sourceIcon[message.source] || '?'}
    </div>
    <div class="min-w-0 flex-1">
      <div class="flex items-center gap-1.5 text-xs">
        <span class="font-medium text-gray-400">{message.sender}</span>
        <span class="text-gray-600">{formatTime(message.created_at)}</span>
      </div>
      <p class="text-xs text-gray-400 mt-0.5 whitespace-pre-wrap break-words">{truncated}</p>
    </div>
  </div>
{:else}
  <div class="flex gap-3 py-3 {message.consolidated ? 'opacity-60' : ''}">
    <div class="shrink-0 w-8 h-8 rounded-full {sourceColor[message.source] || 'bg-gray-600'} flex items-center justify-center text-xs font-bold text-white">
      {sourceIcon[message.source] || '?'}
    </div>
    <div class="min-w-0 flex-1">
      <div class="flex items-center gap-2 text-xs flex-wrap">
        <span class="font-medium text-gray-300">{message.sender}</span>
        {#if containerLabel}
          <span class="text-gray-600">#{containerLabel}</span>
        {/if}
        {#if message.platform}
          <span class="px-1.5 py-0.5 rounded bg-gray-800 text-gray-500 text-[10px]">{message.platform}</span>
        {/if}
        {#if message.source}
          <span class="px-1.5 py-0.5 rounded bg-gray-800 text-gray-500 text-[10px]">{message.source}</span>
        {/if}
        <span class="text-gray-600">{formatTime(message.created_at)}</span>
        {#if message.consolidated}
          <span class="px-1.5 py-0.5 rounded bg-emerald-900/30 text-emerald-500 text-xs">consolidated</span>
        {/if}
      </div>
      <p class="text-sm text-gray-300 mt-0.5 whitespace-pre-wrap break-words">{message.content}</p>
    </div>
  </div>
{/if}
