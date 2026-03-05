<script lang="ts">
  import { onMount } from 'svelte';
  import { profile, signer, relay, ensureConnected } from '../lib/stores';
  import { nip19 } from 'nostr-tools';
  import ProfileCard from '../components/ProfileCard.svelte';

  const CONFIG_D_TAG = 'nomen:config:agents';

  interface MemberEntry {
    pubkey: string;
    meta: Record<string, any>;
    created_at: number;
  }

  let members = $state<MemberEntry[]>([]);
  let loading = $state(false);
  let error = $state('');
  let agentNpubs = $state<Set<string>>(new Set());

  onMount(() => {
    if ($profile) {
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
      // Sort: humans first, then bots/agents, alphabetical within each group
      members = profiles.sort((a, b) => {
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
    <span class="text-sm text-gray-500">{members.length} profiles</span>
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
        />
      {/each}
    </div>
  {/if}
</div>
