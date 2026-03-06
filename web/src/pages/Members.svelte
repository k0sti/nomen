<script lang="ts">
  
  import { profile, signer, relay, ensureConnected } from '../lib/stores';
  import { nip19 } from 'nostr-tools';
  import ProfileCard from '../components/ProfileCard.svelte';

  const CONFIG_D_TAG = 'nomen:config:agents';

  interface MemberEntry {
    pubkey: string;
    meta: Record<string, any>;
    created_at: number;
    hasProfile: boolean;
  }

  let members = $state<MemberEntry[]>([]);
  let loading = $state(false);
  let error = $state('');
  let agentNpubs = $state<Set<string>>(new Set());
  let selectedMember = $state<MemberEntry | null>(null);

  // React to profile changes (handles async login restore after refresh)
  $effect(() => {
    if ($profile && members.length === 0 && !loading) {
      loadMembers();
      loadAgentConfig();
    }
  });

  async function loadAgentConfig() {
    if (!$profile) return;
    try {
      const r = await ensureConnected();
      const event = await r.fetchAppData($profile.pubkey, CONFIG_D_TAG);
      if (event) {
        const data = JSON.parse(event.content);
        const npubs = (data.agents || []).map((a: any) => a.npub);
        agentNpubs = new Set(npubs);
      }
    } catch {
      // No config yet
    }
  }

  async function loadMembers() {
    loading = true;
    error = '';
    try {
      const r = await ensureConnected();
      const profiles = await r.listProfiles();
      // Sort: profiles first, then bots last, alphabetical within each group
      members = profiles.sort((a, b) => {
        // Profiles first
        if (a.hasProfile !== b.hasProfile) return a.hasProfile ? -1 : 1;
        const aIsBot = a.meta.bot === true;
        const bIsBot = b.meta.bot === true;
        if (aIsBot !== bIsBot) return aIsBot ? 1 : -1;
        const aName = (a.meta.display_name || a.meta.displayName || a.meta.name || '').toLowerCase();
        const bName = (b.meta.display_name || b.meta.displayName || b.meta.name || '').toLowerCase();
        return aName.localeCompare(bName);
      });
    } catch (e: any) {
      error = e.message || 'Failed to load profiles';
    } finally {
      loading = false;
    }
  }

  function isAgent(pubkey: string): boolean {
    try {
      const npub = nip19.npubEncode(pubkey);
      return agentNpubs.has(npub);
    } catch {
      return false;
    }
  }

  function isYou(pubkey: string): boolean {
    return $profile?.pubkey === pubkey;
  }

  function getOwnerPubkey(meta: Record<string, any>): string | undefined {
    const ownerNpub = meta.owner || meta.guardian;
    if (!ownerNpub || typeof ownerNpub !== 'string') return undefined;
    try {
      const { data } = nip19.decode(ownerNpub);
      return data as string;
    } catch {
      return undefined;
    }
  }

  function getAgentCount(meta: Record<string, any>): number | undefined {
    if (meta.agents && Array.isArray(meta.agents)) return meta.agents.length;
    return undefined;
  }
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold text-gray-100">Members</h1>
    <span class="text-sm text-gray-500">{members.length} members</span>
  </div>

  {#if loading}
    <div class="space-y-3">
      {#each [1, 2, 3, 4] as _}
        <div class="h-20 rounded-xl bg-gray-800/50 animate-pulse"></div>
      {/each}
    </div>
  {:else if error}
    <div class="p-6 rounded-xl border border-red-800/50 bg-red-900/10 text-center">
      <p class="text-red-400">{error}</p>
      <button onclick={loadMembers} class="mt-3 text-sm text-gray-400 hover:text-gray-200 underline">Retry</button>
    </div>
  {:else if members.length === 0}
    <div class="p-6 rounded-xl border border-gray-700 bg-gray-800/30 text-center">
      <p class="text-gray-400">No profiles found on this relay.</p>
    </div>
  {:else}
    <div class="space-y-2">
      {#each members as member (member.pubkey)}
        <button class="w-full text-left" onclick={() => selectedMember = member}>
          <ProfileCard
            pubkey={member.pubkey}
            name={member.meta.name}
            displayName={member.meta.display_name || member.meta.displayName}
            picture={member.meta.picture}
            about={member.meta.about}
            nip05={member.meta.nip05}
            isBot={member.meta.bot === true}
            isAgent={isAgent(member.pubkey)}
            isYou={isYou(member.pubkey)}
            ownerPubkey={getOwnerPubkey(member.meta)}
            agentCount={getAgentCount(member.meta)}
            role={member.hasProfile ? 'profile' : undefined}
          />
        </button>
      {/each}
    </div>
  {/if}
</div>

<!-- Member profile modal -->
{#if selectedMember}
  {@const m = selectedMember}
  {@const npub = nip19.npubEncode(m.pubkey)}
  {@const npubShort = npub.slice(0, 12) + '…' + npub.slice(-6)}
  {@const label = m.meta.display_name || m.meta.displayName || m.meta.name || npubShort}
  <dialog open class="fixed inset-0 z-50 flex items-center justify-center bg-transparent">
    <!-- backdrop -->
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="fixed inset-0 bg-black/60" role="presentation" onclick={() => selectedMember = null}></div>
    <div class="relative bg-gray-900 border border-gray-700 rounded-xl p-6 w-full max-w-sm shadow-2xl mx-4">
      <div class="flex flex-col items-center">
        {#if m.meta.picture}
          <img src={m.meta.picture} alt="" class="w-24 h-24 rounded-full object-cover border-2 border-gray-600 mb-4" />
        {:else}
          <div class="w-24 h-24 rounded-full bg-accent-600 flex items-center justify-center text-4xl font-bold text-white mb-4">
            {label[0].toUpperCase()}
          </div>
        {/if}

        <h2 class="text-xl font-semibold text-gray-100">{label}</h2>

        {#if m.meta.name && m.meta.name !== label}
          <p class="text-sm text-gray-500">@{m.meta.name}</p>
        {/if}

        <div class="flex items-center gap-2 mt-2">
          <code class="text-sm text-gray-400 font-mono">{npubShort}</code>
          <button
            onclick={() => navigator.clipboard.writeText(npub)}
            class="w-9 h-9 flex items-center justify-center rounded-lg text-gray-500 hover:text-gray-300 hover:bg-gray-800 transition-colors"
            title="Copy npub"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
            </svg>
          </button>
        </div>

        {#if m.meta.nip05}
          <p class="text-sm text-gray-500 mt-1">{m.meta.nip05}</p>
        {/if}

        {#if m.meta.about}
          <p class="text-sm text-gray-400 mt-3 text-center whitespace-pre-wrap">{m.meta.about}</p>
        {/if}

        {#if m.meta.website}
          <a href={m.meta.website} target="_blank" rel="noopener" class="text-sm text-accent-400 hover:underline mt-2">{m.meta.website}</a>
        {/if}

        {#if m.meta.lud16}
          <p class="text-xs text-gray-500 mt-2">⚡ {m.meta.lud16}</p>
        {/if}

        <div class="flex items-center gap-2 mt-2 flex-wrap justify-center">
          {#if isYou(m.pubkey)}
            <span class="text-[10px] px-1.5 py-0.5 rounded bg-green-900/40 text-green-400 border border-green-800/50">you</span>
          {/if}
          {#if m.meta.bot === true}
            <span class="text-[10px] px-1.5 py-0.5 rounded bg-blue-900/40 text-blue-400 border border-blue-800/50">bot</span>
          {/if}
          {#if isAgent(m.pubkey)}
            <span class="text-[10px] px-1.5 py-0.5 rounded bg-accent-600/20 text-accent-400 border border-accent-600/40">agent</span>
          {/if}
        </div>
      </div>

      <div class="mt-6">
        <button
          onclick={() => selectedMember = null}
          class="w-full py-2.5 min-h-11 rounded-lg border border-gray-700 text-gray-400 hover:text-gray-200 hover:bg-gray-800 transition-colors text-sm"
        >
          Close
        </button>
      </div>
    </div>
  </dialog>
{/if}
