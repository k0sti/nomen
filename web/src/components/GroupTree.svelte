<script lang="ts">
  import type { Group } from '../lib/api';

  let { groups, selected, onselect }: {
    groups: Group[];
    selected: string | null;
    onselect: (id: string) => void;
  } = $props();

  // Build hierarchy from dot-separated IDs (e.g. "techteam.infra" is child of "techteam")
  interface TreeNode {
    group: Group;
    children: TreeNode[];
  }

  let expanded = $state<Set<string>>(new Set());

  const tree = $derived.by(() => {
    const nodes: TreeNode[] = [];
    const byId = new Map(groups.map(g => [g.id, g]));

    // Find root groups (no dot or parent doesn't exist)
    for (const g of groups) {
      const dotIdx = g.id.lastIndexOf('.');
      if (dotIdx === -1 || !byId.has(g.id.slice(0, dotIdx))) {
        nodes.push({ group: g, children: [] });
      }
    }

    // Attach children
    for (const g of groups) {
      const dotIdx = g.id.lastIndexOf('.');
      if (dotIdx !== -1) {
        const parentId = g.id.slice(0, dotIdx);
        const parent = nodes.find(n => n.group.id === parentId);
        if (parent) {
          parent.children.push({ group: g, children: [] });
        }
      }
    }

    return nodes;
  });

  function toggleExpand(id: string) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id); else next.add(id);
    expanded = next;
  }
</script>

<div class="space-y-0.5">
  {#each tree as node}
    <div>
      <div class="flex items-center gap-0">
        {#if node.children.length > 0}
          <button class="text-gray-500 hover:text-gray-300 text-xs w-6 h-8 flex items-center justify-center" onclick={() => toggleExpand(node.group.id)}>
            {expanded.has(node.group.id) ? '▼' : '▶'}
          </button>
        {:else}
          <span class="w-6"></span>
        {/if}
        <button
          class="flex-1 flex items-center gap-2 px-2 py-2 rounded-lg text-sm transition-colors {selected === node.group.id ? 'bg-accent-500/15 text-accent-400' : 'text-gray-300 hover:bg-gray-800/50'}"
          onclick={() => onselect(node.group.id)}
        >
          <span>{node.group.name}</span>
          <span class="ml-auto text-xs text-gray-600">{node.group.members.length}</span>
        </button>
      </div>

      {#if expanded.has(node.group.id) && node.children.length > 0}
        <div class="ml-6">
          {#each node.children as child}
            <button
              class="w-full flex items-center gap-2 px-3 py-2 rounded-lg text-sm transition-colors {selected === child.group.id ? 'bg-accent-500/15 text-accent-400' : 'text-gray-300 hover:bg-gray-800/50'}"
              onclick={() => onselect(child.group.id)}
            >
              <span class="w-4"></span>
              <span>{child.group.name}</span>
              <span class="ml-auto text-xs text-gray-600">{child.group.members.length}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/each}
</div>
