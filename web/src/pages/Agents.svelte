<script lang="ts">
  import { profile, signer, relay, relayConnected } from '../lib/stores';
  import { nip19 } from 'nostr-tools';
  import { compressNpub, fetchProfileMetadata } from '../lib/nostr';
  import type { NostrProfile } from '../lib/nostr';
  import ProfileCard from '../components/ProfileCard.svelte';

  const CONFIG_D_TAG = 'nomen:config:agents';

  interface AgentEntry {
    npub: string;
    role: 'agent' | 'guardian';
    added: number;
    // resolved profile metadata
    profile?: NostrProfile;
    meta?: Record<string, any>;
  }

  let agents = $state<AgentEntry[]>([]);
  let discovered = $state<AgentEntry[]>([]);
  let addInput = $state('');
  let addPreview = $state<AgentEntry | null>(null);
  let addLoading = $state(false);
  let addError = $state('');
  let saving = $state(false);
  let loadingConfig = $state(false);
  let discoverLoading = $state(false);

  // Load agent config on mount
  $effect(() => {
    if ($profile && $relayConnected) {
      loadConfig();
    }
  });

  async function loadConfig() {
    if (!$profile || !$relay) return;
    loadingConfig = true;
    try {
      await $relay.connect();
      if ($signer) await $relay.authenticate($signer);
      const event = await $relay.fetchAppData($profile.pubkey, CONFIG_D_TAG);
      if (event) {
        const data = JSON.parse(event.content);
        agents = (data.agents || []) as AgentEntry[];
        // Resolve profiles in background
        resolveProfiles(agents);
        discoverAgents();
      }
    } catch {
      // No config yet, that's fine
    } finally {
      loadingConfig = false;
    }
  }

  async function resolveProfiles(entries: AgentEntry[]) {
    for (const entry of entries) {
      try {
        const { data: pubkey } = nip19.decode(entry.npub);
        const meta = await fetchProfileMetadata(pubkey as string);
        if (meta) {
          entry.meta = meta;
          entry.profile = {
            pubkey: pubkey as string,
            npub: entry.npub,
            npubShort: compressNpub(entry.npub),
            name: meta.name,
            displayName: meta.display_name || meta.displayName,
            picture: meta.picture,
            about: meta.about,
          };
          agents = [...agents]; // trigger reactivity
        }
      } catch {
        // Skip failed lookups
      }
    }
  }

  async function discoverAgents() {
    if (!$profile) return;
    discoverLoading = true;
    discovered = [];
    try {
      // Check own kind 0 for "agents" field
      const ownMeta = await fetchProfileMetadata($profile.pubkey);
      if (ownMeta?.agents && Array.isArray(ownMeta.agents)) {
        for (const npub of ownMeta.agents) {
          if (typeof npub === 'string' && npub.startsWith('npub1') && !agents.some(a => a.npub === npub)) {
            const entry: AgentEntry = { npub, role: 'agent', added: 0 };
            try {
              const { data: pk } = nip19.decode(npub);
              const meta = await fetchProfileMetadata(pk as string);
              if (meta) {
                entry.meta = meta;
                entry.profile = {
                  pubkey: pk as string,
                  npub,
                  npubShort: compressNpub(npub),
                  name: meta.name,
                  displayName: meta.display_name || meta.displayName,
                  picture: meta.picture,
                  about: meta.about,
                };
              }
            } catch {}
            discovered = [...discovered, entry];
          }
        }
      }

      // Check each agent's kind 0 for owner/guardian referencing us
      for (const agent of agents) {
        if (agent.meta) {
          const ownerNpub = agent.meta.owner || agent.meta.guardian;
          if (ownerNpub === $profile.npub) {
            // Already added, just note the relationship
          }
        }
      }
    } catch {
      // Discovery is best-effort
    } finally {
      discoverLoading = false;
    }
  }

  async function resolveInput() {
    const input = addInput.trim();
    if (!input) return;
    addLoading = true;
    addError = '';
    addPreview = null;

    try {
      let npub = input;
      let pubkey: string;

      if (input.startsWith('npub1')) {
        const { data } = nip19.decode(input);
        pubkey = data as string;
      } else {
        // Try NIP-05 resolution
        const [name, domain] = input.split('@');
        if (!domain) throw new Error('Enter an npub or NIP-05 (user@domain)');
        const res = await fetch(`https://${domain}/.well-known/nostr.json?name=${encodeURIComponent(name)}`);
        const json = await res.json();
        pubkey = json.names?.[name];
        if (!pubkey) throw new Error(`NIP-05 not found: ${input}`);
        npub = nip19.npubEncode(pubkey);
      }

      if (agents.some(a => a.npub === npub)) {
        addError = 'Agent already added';
        return;
      }

      const entry: AgentEntry = { npub, role: 'agent', added: Math.floor(Date.now() / 1000) };
      const meta = await fetchProfileMetadata(pubkey);
      if (meta) {
        entry.meta = meta;
        entry.profile = {
          pubkey,
          npub,
          npubShort: compressNpub(npub),
          name: meta.name,
          displayName: meta.display_name || meta.displayName,
          picture: meta.picture,
          about: meta.about,
        };
      }
      addPreview = entry;
    } catch (e: any) {
      addError = e.message || 'Failed to resolve';
    } finally {
      addLoading = false;
    }
  }

  async function addAgent(entry?: AgentEntry) {
    const toAdd = entry || addPreview;
    if (!toAdd) return;
    agents = [...agents, { ...toAdd, added: toAdd.added || Math.floor(Date.now() / 1000) }];
    addPreview = null;
    addInput = '';
    // Remove from discovered if present
    discovered = discovered.filter(d => d.npub !== toAdd.npub);
    await saveConfig();
  }

  async function removeAgent(npub: string) {
    agents = agents.filter(a => a.npub !== npub);
    await saveConfig();
  }

  function toggleRole(npub: string) {
    agents = agents.map(a =>
      a.npub === npub ? { ...a, role: a.role === 'agent' ? 'guardian' : 'agent' } : a
    );
    saveConfig();
  }

  async function saveConfig() {
    if (!$signer || !$relay) return;
    saving = true;
    try {
      const content = JSON.stringify({
        agents: agents.map(a => ({ npub: a.npub, role: a.role, added: a.added })),
      });
      await $relay.connect();
      await $relay.authenticate($signer);
      await $relay.publishAppData(CONFIG_D_TAG, content, $signer);
    } catch (e: any) {
      console.error('Failed to save agent config:', e);
    } finally {
      saving = false;
    }
  }

  function displayName(entry: AgentEntry): string {
    return entry.profile?.displayName || entry.profile?.name || compressNpub(entry.npub);
  }

  function isBot(entry: AgentEntry): boolean {
    return entry.meta?.bot === true;
  }

  function ownerOf(entry: AgentEntry): string | null {
    if (!$profile) return null;
    const owner = entry.meta?.owner || entry.meta?.guardian;
    if (owner === $profile.npub) return 'you';
    return null;
  }
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold text-gray-100">Agents</h1>
    {#if saving}
      <span class="text-xs text-gray-500">Saving...</span>
    {/if}
  </div>

  {#if !$profile}
    <div class="p-6 rounded-xl border border-gray-700 bg-gray-800/30 text-center">
      <p class="text-gray-400">Login to manage your agents.</p>
    </div>
  {:else if loadingConfig}
    <div class="space-y-3">
      {#each [1, 2] as _}
        <div class="h-20 rounded-xl bg-gray-800/50 animate-pulse"></div>
      {/each}
    </div>
  {:else}
    <!-- Agent List -->
    <section>
      <h2 class="text-sm font-medium text-gray-400 mb-3">Configured Agents ({agents.length})</h2>
      {#if agents.length === 0}
        <div class="p-4 rounded-xl border border-gray-700 bg-gray-800/30 text-center text-sm text-gray-500">
          No agents configured yet. Add one below.
        </div>
      {:else}
        <div class="space-y-2">
          {#each agents as agent (agent.npub)}
            <ProfileCard
              pubkey={agent.profile?.pubkey || (() => { try { return nip19.decode(agent.npub).data as string; } catch { return ''; } })()}
              name={agent.profile?.name}
              displayName={agent.profile?.displayName}
              picture={agent.profile?.picture}
              about={agent.profile?.about}
              isBot={isBot(agent)}
              role={agent.role}
            >
              <button
                onclick={() => toggleRole(agent.npub)}
                class="text-[11px] px-2 py-1 rounded-lg border transition-colors duration-150
                  {agent.role === 'guardian'
                    ? 'bg-purple-900/30 border-purple-800/50 text-purple-400 hover:bg-purple-900/50'
                    : 'bg-accent-600/10 border-accent-600/30 text-accent-400 hover:bg-accent-600/20'}"
                title="Click to toggle role"
              >
                {agent.role}
              </button>
              <button
                onclick={() => removeAgent(agent.npub)}
                class="w-9 h-9 flex items-center justify-center rounded-lg text-gray-600 hover:text-red-400 hover:bg-red-900/20 transition-colors duration-150"
                title="Remove agent"
                aria-label="Remove agent"
              >
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </ProfileCard>
          {/each}
        </div>
      {/if}
    </section>

    <!-- Add Agent -->
    <section>
      <h2 class="text-sm font-medium text-gray-400 mb-3">Add Agent</h2>
      <div class="p-4 rounded-xl border border-gray-700 bg-gray-800/30 space-y-3">
        <div class="flex gap-2">
          <input
            type="text"
            bind:value={addInput}
            placeholder="npub1... or user@domain.com"
            class="flex-1 px-3 py-2.5 min-h-11 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-600 focus:border-accent-500 transition-colors duration-150"
            onkeydown={(e) => e.key === 'Enter' && resolveInput()}
          />
          <button
            onclick={resolveInput}
            disabled={addLoading || !addInput.trim()}
            class="px-4 py-2.5 min-h-11 rounded-lg border border-accent-600/50 bg-accent-600/10 hover:bg-accent-600/20 active:bg-accent-600/30 text-gray-100 text-sm font-medium transition-colors duration-150 disabled:opacity-50 disabled:cursor-not-allowed shrink-0"
          >
            {addLoading ? '...' : 'Lookup'}
          </button>
        </div>

        {#if addError}
          <p class="text-sm text-red-400">{addError}</p>
        {/if}

        {#if addPreview}
          <ProfileCard
            pubkey={addPreview.profile?.pubkey || (() => { try { return nip19.decode(addPreview.npub).data as string; } catch { return ''; } })()}
            name={addPreview.profile?.name}
            displayName={addPreview.profile?.displayName}
            picture={addPreview.profile?.picture}
            about={addPreview.profile?.about}
          >
            <button
              onclick={() => addAgent()}
              class="px-4 py-2 rounded-lg bg-accent-600/20 border border-accent-600/50 text-accent-300 hover:bg-accent-600/30 text-sm font-medium transition-colors duration-150 shrink-0"
            >
              Add Agent
            </button>
          </ProfileCard>
        {/if}
      </div>
    </section>

    <!-- Agent Discovery -->
    <section>
      <div class="flex items-center gap-2 mb-3">
        <h2 class="text-sm font-medium text-gray-400">Discovered Agents</h2>
        {#if discoverLoading}
          <span class="text-xs text-gray-600">scanning...</span>
        {/if}
      </div>

      {#if discovered.length === 0}
        <div class="p-4 rounded-xl border border-gray-700 bg-gray-800/30 text-center text-sm text-gray-500">
          {discoverLoading ? 'Checking your profile metadata for agent references...' : 'No new agents discovered from your profile metadata.'}
        </div>
      {:else}
        <div class="space-y-2">
          {#each discovered as disc (disc.npub)}
            <ProfileCard
              pubkey={disc.profile?.pubkey || (() => { try { return nip19.decode(disc.npub).data as string; } catch { return ''; } })()}
              name={disc.profile?.name}
              displayName={disc.profile?.displayName}
              picture={disc.profile?.picture}
              about={disc.profile?.about}
              isBot={isBot(disc)}
            >
              <button
                onclick={() => addAgent(disc)}
                class="px-3 py-1.5 rounded-lg border border-accent-600/50 bg-accent-600/10 hover:bg-accent-600/20 text-accent-300 text-sm transition-colors duration-150 shrink-0"
              >
                Add
              </button>
            </ProfileCard>
          {/each}
        </div>
      {/if}
    </section>
  {/if}
</div>
