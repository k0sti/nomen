<script lang="ts">
  let { tier, scope = '' }: { tier: string; scope?: string } = $props();

  const styles: Record<string, string> = {
    public: 'bg-emerald-900/40 text-emerald-400 border-emerald-800/50',
    group: 'bg-blue-900/40 text-blue-400 border-blue-800/50',
    private: 'bg-purple-900/40 text-purple-400 border-purple-800/50',
  };

  const style = $derived(styles[tier] || 'bg-gray-800 text-gray-400 border-gray-700');
  const label = $derived(tier === 'group' && scope ? `group:${scope}` : tier);
</script>

<span class="inline-flex items-center px-2 py-0.5 rounded-md text-xs font-medium border {style}">
  {label}
</span>
