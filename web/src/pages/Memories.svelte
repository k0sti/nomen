<script lang="ts">
  import { onDestroy } from 'svelte';
  import MemoryCard from '../components/MemoryCard.svelte';
  import { relay, memories, visibilityFilter, loading, profile, isLoggedIn, getSigner, ensureConnected, showError } from '../lib/stores';
  import { nip19, nip44, nip04 } from 'nostr-tools';
  import type { Memory } from '../lib/api';
  import type { Subscription } from '../lib/relay';

  let filterText = $state('');
  let sub: Subscription | null = null;

  const filtered = $derived(
    $memories.filter((m) => {
      const matchesVisibility = !$visibilityFilter || m.visibility === $visibilityFilter;
      const matchesText =
        !filterText ||
        m.topic.toLowerCase().includes(filterText.toLowerCase()) ||
        m.summary.toLowerCase().includes(filterText.toLowerCase());
      return matchesVisibility && matchesText;
    })
  );

  const stats = $derived({
    total: $memories.length,
    public: $memories.filter((m) => m.visibility === 'public').length,
    group: $memories.filter((m) => m.visibility === 'group').length,
    personal: $memories.filter((m) => m.visibility === 'personal' || m.visibility === 'private').length,
  });

  function bytesToHex(bytes: Uint8Array): string {
    return Array.from(bytes).map((b) => b.toString(16).padStart(2, '0')).join('');
  }

  function looksEncrypted(v: string): boolean {
    if (!v) return false;
    if (v.startsWith('nip44:') || v.startsWith('nip04:')) return true;
    if (v.startsWith('{') || v.startsWith('[')) return false;
    // Heuristic for encoded ciphertext blobs
    return v.length > 80 && /^[A-Za-z0-9+/=:_-]+$/.test(v);
  }

  async function tryDecryptWithNsec(cipherText: string, sourcePubkey: string, nsec: string): Promise<string | null> {
    try {
      const decoded = nip19.decode(nsec);
      if (decoded.type !== 'nsec') return null;
      const secret = decoded.data as Uint8Array;

      const is44 = cipherText.startsWith('nip44:');
      const is04 = cipherText.startsWith('nip04:');
      const body = is44 || is04 ? cipherText.slice(6) : cipherText;

      // Try NIP-44 first
      try {
        const convKey = nip44.getConversationKey(secret, sourcePubkey);
        const plain = nip44.decrypt(body, convKey);
        if (plain) return plain;
      } catch {}

      // Then NIP-04
      try {
        const plain = await nip04.decrypt(bytesToHex(secret), sourcePubkey, body);
        if (plain) return plain;
      } catch {}

      return null;
    } catch {
      return null;
    }
  }

  async function decryptPrivateMemories(ms: Memory[], r: any): Promise<Memory[]> {
    if (!$profile) return ms;
    const cfg = await r.fetchAppData($profile.pubkey, 'nomen:config:agents').catch(() => null);
    if (!cfg) return ms;

    let agentNsecs: Record<string, string> = {};
    try {
      const parsed = JSON.parse(cfg.content);
      for (const a of parsed.agents || []) {
        if (a?.npub && a?.nsec) {
          const pk = nip19.decode(a.npub).data as string;
          agentNsecs[pk] = a.nsec;
        }
      }
    } catch {
      return ms;
    }

    const out: Memory[] = [];
    for (const m of ms) {
      if (m.visibility !== 'personal' && m.visibility !== 'private') {
        out.push(m);
        continue;
      }
      const nsec = agentNsecs[m.source];
      if (!nsec) {
        out.push(m);
        continue;
      }

      let summary = m.summary;
      let detail = m.detail;

      if (looksEncrypted(summary)) {
        const dec = await tryDecryptWithNsec(summary, m.source, nsec);
        if (dec) summary = dec;
      }
      if (looksEncrypted(detail)) {
        const dec = await tryDecryptWithNsec(detail, m.source, nsec);
        if (dec) detail = dec;
      }

      out.push({ ...m, summary, detail });
    }

    return out;
  }

  async function loadMemories() {
    loading.set(true);
    try {
      const r = await ensureConnected();

      const result = await r.listMemories($profile!.pubkey);
      const decrypted = await decryptPrivateMemories(result, r);
      memories.set(decrypted);

      // Live subscription for new memories
      sub = r.subscribeMemories($profile!.pubkey, (m: Memory) => {
        memories.update((ms) => {
          const idx = ms.findIndex((x) => x.d_tag === m.d_tag);
          if (idx >= 0) {
            const updated = [...ms];
            updated[idx] = m;
            return updated;
          }
          return [m, ...ms];
        });
      });
    } catch (err: any) {
      showError('Failed to load memories: ' + (err.message || err));
    } finally {
      loading.set(false);
    }
  }

  // React to async profile restore after refresh
  $effect(() => {
    if (!$profile) return;
    if ($memories.length > 0) return; // already loaded
    loadMemories();
  });

  onDestroy(() => {
    sub?.close();
  });

  async function handleDelete(memory: Memory) {
    if (!memory.id) return;
    try {
      const signer = getSigner();
      await $relay.deleteMemory(memory.id, signer, memory.d_tag);
      memories.update((ms) => ms.filter((m) => m.d_tag !== memory.d_tag));
    } catch (err: any) {
      showError('Failed to delete memory: ' + (err.message || err));
    }
  }

  function setVisibilityFilter(vis: string) {
    visibilityFilter.set($visibilityFilter === vis ? '' : vis);
  }

  // ── Create memory form ─────────────────────────────────────────
  let showCreateForm = $state(false);
  let newTopic = $state('');
  let newSummary = $state('');
  let newDetail = $state('');
  let newVisibility = $state('public');
  let creating = $state(false);

  async function createMemory() {
    if (!newTopic.trim() || !newSummary.trim()) return;
    creating = true;
    try {
      const signer = getSigner();
      await $relay.storeMemory(newTopic.trim(), newSummary.trim(), newDetail.trim(), newVisibility, signer);
      // Reload memories to include the new one
      const result = await $relay.listMemories($profile!.pubkey);
      memories.set(result);
      // Reset form
      newTopic = '';
      newSummary = '';
      newDetail = '';
      newVisibility = 'public';
      showCreateForm = false;
    } catch (err: any) {
      showError('Failed to create memory: ' + (err.message || err));
    } finally {
      creating = false;
    }
  }
</script>

<div class="max-w-4xl mx-auto space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-2xl font-bold text-gray-100">Memories</h2>
      <p class="text-sm text-gray-500 mt-1">
        {stats.total} memories &mdash; {stats.public} public, {stats.group} group, {stats.personal} personal
      </p>
    </div>
    {#if $isLoggedIn}
      <button
        onclick={() => showCreateForm = !showCreateForm}
        class="px-4 py-2 min-h-11 rounded-lg border border-accent-600/50 bg-accent-600/10 hover:bg-accent-600/20 text-accent-400 text-sm font-medium transition-colors duration-150"
      >
        {showCreateForm ? 'Cancel' : '+ New Memory'}
      </button>
    {/if}
  </div>

  {#if showCreateForm}
    <div class="p-4 rounded-lg border border-gray-700 bg-gray-900/50 space-y-3">
      <div class="grid grid-cols-2 gap-3">
        <label class="block">
          <span class="text-xs text-gray-400">Topic</span>
          <input type="text" bind:value={newTopic} placeholder="e.g. project/nomen/overview" class="mt-1 w-full px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500" />
        </label>
        <label class="block">
          <span class="text-xs text-gray-400">Visibility</span>
          <select bind:value={newVisibility} class="mt-1 w-full px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500">
            <option value="public">Public</option>
            <option value="group">Group</option>
            <option value="personal">Personal</option>
          </select>
        </label>
      </div>
      <label class="block">
        <span class="text-xs text-gray-400">Summary</span>
        <input type="text" bind:value={newSummary} placeholder="One-line summary..." class="mt-1 w-full px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 focus:border-accent-500" />
      </label>
      <label class="block">
        <span class="text-xs text-gray-400">Detail (optional)</span>
        <textarea bind:value={newDetail} rows="3" placeholder="Full detail..." class="mt-1 w-full px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 resize-y focus:border-accent-500"></textarea>
      </label>
      <div class="flex justify-end">
        <button
          onclick={createMemory}
          disabled={creating || !newTopic.trim() || !newSummary.trim()}
          class="px-4 py-2 rounded-lg bg-accent-600 hover:bg-accent-500 disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium transition-colors duration-150"
        >
          {creating ? 'Creating...' : 'Create Memory'}
        </button>
      </div>
    </div>
  {/if}

  <div class="flex items-center gap-3">
    <input
      type="text"
      placeholder="Filter by topic or summary..."
      bind:value={filterText}
      class="flex-1 px-4 py-2.5 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-500 transition-colors duration-150 focus:border-accent-500"
    />
    <div class="flex gap-1.5">
      {#each ['public', 'group', 'personal'] as vis}
        <button
          onclick={() => setVisibilityFilter(vis)}
          class="px-3 py-2 min-h-11 rounded-md text-xs font-medium border transition-colors duration-150
            {$visibilityFilter === vis
              ? 'border-accent-500 bg-accent-500/20 text-accent-400'
              : 'border-gray-700 bg-gray-800/50 text-gray-400 hover:text-gray-200 active:bg-gray-700'}"
        >
          {vis}
        </button>
      {/each}
    </div>
  </div>

  {#if !$isLoggedIn}
    <div class="text-center py-12 text-gray-500">Login to view memories from the relay</div>
  {:else if $loading}
    <div class="space-y-2">
      {#each { length: 4 } as _}
        <div class="border border-gray-800 rounded-lg p-4 bg-gray-900/50">
          <div class="flex items-start justify-between gap-3">
            <div class="flex-1 space-y-2">
              <div class="flex items-center gap-2">
                <div class="skeleton h-4 w-40"></div>
                <div class="skeleton h-5 w-16"></div>
              </div>
              <div class="skeleton h-3.5 w-full max-w-md"></div>
            </div>
            <div class="space-y-1.5 text-right">
              <div class="skeleton h-3 w-24 ml-auto"></div>
              <div class="skeleton h-3 w-16 ml-auto"></div>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {:else if filtered.length === 0}
    <div class="text-center py-12 text-gray-500">
      {$memories.length === 0 ? 'No memories yet' : 'No memories match your filters'}
    </div>
  {:else}
    <div class="space-y-2">
      {#each filtered as memory (memory.d_tag)}
        <MemoryCard {memory} ondelete={handleDelete} />
      {/each}
    </div>
  {/if}
</div>
