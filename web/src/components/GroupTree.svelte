<script lang="ts">
  import type { Group } from '../lib/api';

  let { groups, selected, onselect }: {
    groups: Group[];
    selected: string | null;
    onselect: (id: string) => void;
  } = $props();

  interface TreeNode {
    group: Group;
    children: TreeNode[];
  }

  let expanded = $state<Set<string>>(new Set());

  const tree = $derived.by(() => {
    const nodes: TreeNode[] = [];
    const byId = new Map(groups.map(g => [g.id, g]));

    for (const g of groups) {
      const dotIdx = g.id.lastIndexOf('.');
      if (dotIdx === -1 || !byId.has(g.id.slice(0, dotIdx))) {
        nodes.push({ group: g, children: [] });
      }
    }

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
          <button
            class="text-gray-500 hover:text-gray-300 text-xs w-8 h-11 flex items-center justify-center rounded transition-colors duration-150"
            onclick={() => toggleExpand(node.group.id)}
            aria-label={expanded.has(node.group.id) ? 'Collapse' : 'Expand'}
          >
            <svg class="w-3.5 h-3.5 transition-transform duration-150 {expanded.has(node.group.id) ? 'rotate-90' : ''}" fill="currentColor" viewBox="0 0 20 20"><path fill-rule="evenodd" d="M7.293 14.707a1 1 0 010-1.414L10.586 10 7.293 6.707a1 1 0 011.414-1.414l4 4a1 1 0 010 1.414l-4 4a1 1 0 01-1.414 0z" clip-rule="evenodd" /></svg>
          </button>
        {:else}
          <span class="w-8"></span>
        {/if}
        <button
          class="flex-1 flex items-center gap-2 px-2 py-2.5 min-h-11 rounded-lg text-sm transition-colors duration-150 {selected === node.group.id ? 'bg-accent-500/15 text-accent-400' : 'text-gray-300 hover:bg-gray-800/50 active:bg-gray-800'}"
          onclick={() => onselect(node.group.id)}
          aria-current={selected === node.group.id ? 'true' : undefined}
        >
          <span>{node.group.name}</span>
          <span class="ml-auto text-xs text-gray-600">{node.group.members.length}</span>
        </button>
      </div>

      {#if expanded.has(node.group.id) && node.children.length > 0}
        <div class="ml-8">
          {#each node.children as child}
            <button
              class="w-full flex items-center gap-2 px-3 py-2.5 min-h-11 rounded-lg text-sm transition-colors duration-150 {selected === child.group.id ? 'bg-accent-500/15 text-accent-400' : 'text-gray-300 hover:bg-gray-800/50 active:bg-gray-800'}"
              onclick={() => onselect(child.group.id)}
              aria-current={selected === child.group.id ? 'true' : undefined}
            >
              <span>{child.group.name}</span>
              <span class="ml-auto text-xs text-gray-600">{child.group.members.length}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/each}
</div>
