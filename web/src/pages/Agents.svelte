<script lang="ts">
  
  import { profile, signer, relay, ensureConnected, showError, showInfo } from '../lib/stores';
  import { nip19, getPublicKey } from 'nostr-tools';
  import { compressNpub, fetchProfileMetadata } from '../lib/nostr';
  import { fetchProfileFromRelay } from '../lib/relay';
  import type { NostrProfile } from '../lib/nostr';
  import ProfileCard from '../components/ProfileCard.svelte';

  const CONFIG_D_TAG = 'nomen:config:agents';

  interface AgentEntry {
    npub: string;
    role: 'agent' | 'guardian';
    added: number;
    nsec?: string; // optional nsec for decrypting private memories
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
  let selectedAgent = $state<AgentEntry | null>(null);
  let editingNsec = $state(false);
  let nsecInput = $state('');

  $effect(() => {
    if ($profile && agents.length === 0 && !loadingConfig) {
      loadConfig();
    }
  });

  async function loadConfig() {
    if (!$profile) return;
    loadingConfig = true;
    try {
      const r = await ensureConnected();
      const event = await r.fetchAppData($profile.pubkey, CONFIG_D_TAG);
      console.log('[Agents] loadConfig: fetchAppData returned', event ? `event ${event.id}` : 'null');
      if (event) {
        const data = JSON.parse(event.content);
        agents = (data.agents || []) as AgentEntry[];
        console.log('[Agents] loadConfig: loaded', agents.length, 'agents');
        // Resolve profiles in background
        resolveProfiles(agents);
        discoverAgents();
      } else {
        console.log('[Agents] loadConfig: no config event found for d-tag', CONFIG_D_TAG);
        discoverAgents();
      }
    } catch (e: any) {
      console.error('[Agents] loadConfig error:', e);
      showError('Failed to load agent config: ' + (e.message || e));
    } finally {
      loadingConfig = false;
    }
  }

  async function resolveProfiles(entries: AgentEntry[]) {
    for (const entry of entries) {
      try {
        const { data: pubkey } = nip19.decode(entry.npub);
        // Try zooid relay first, then public relays
        let meta = await fetchProfileFromRelay(pubkey as string);
        if (!meta) {
          meta = await fetchProfileMetadata(pubkey as string);
        }
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
      let nsec: string | undefined;

      if (input.startsWith('nsec1')) {
        // Derive npub from nsec
        const { type, data } = nip19.decode(input);
        if (type !== 'nsec') throw new Error('Invalid nsec');
        const secretKey = data as Uint8Array;
        pubkey = getPublicKey(secretKey);
        npub = nip19.npubEncode(pubkey);
        nsec = input;
      } else if (input.startsWith('npub1')) {
        const { data } = nip19.decode(input);
        pubkey = data as string;
      } else {
        // Try NIP-05 resolution
        const [name, domain] = input.split('@');
        if (!domain) throw new Error('Enter an npub, nsec, or NIP-05 (user@domain)');
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

      const entry: AgentEntry = { npub, role: 'agent', added: Math.floor(Date.now() / 1000), nsec };
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
    if (!$signer) {
      showError('Cannot save: not logged in');
      console.error('[Agents] saveConfig: no signer available');
      return;
    }
    saving = true;
    try {
      const content = JSON.stringify({
        agents: agents.map(a => {
          const entry: any = { npub: a.npub, role: a.role, added: a.added };
          if (a.nsec) entry.nsec = a.nsec;
          return entry;
        }),
      });
      const r = await ensureConnected();
      console.log('[Agents] saveConfig: publishing to d-tag', CONFIG_D_TAG, 'with', agents.length, 'agents');
      const eventId = await r.publishAppData(CONFIG_D_TAG, content, $signer);
      console.log('[Agents] saveConfig: published OK, event id:', eventId);
      showInfo('Agent config saved');
    } catch (e: any) {
      console.error('[Agents] saveConfig error:', e);
      showError('Failed to save agent config: ' + (e.message || e));
    } finally {
      saving = false;
    }
  }

  function openAgent(agent: AgentEntry) {
    selectedAgent = agent;
    editingNsec = false;
    nsecInput = agent.nsec || '';
  }

  function isOwnAgent(agent: AgentEntry): boolean {
    if (!$profile) return false;
    const owner = agent.meta?.owner || agent.meta?.guardian;
    return owner === $profile.npub || agent.role === 'guardian' || agents.some(a => a.npub === agent.npub);
  }

  async function saveNsec() {
    if (!selectedAgent) return;
    const trimmed = nsecInput.trim();
    // Validate nsec format
    if (trimmed && !trimmed.startsWith('nsec1')) {
      showError('Invalid nsec format — must start with nsec1');
      return;
    }
    if (trimmed) {
      try {
        nip19.decode(trimmed);
      } catch {
        showError('Invalid nsec — failed to decode');
        return;
      }
    }
    agents = agents.map(a =>
      a.npub === selectedAgent!.npub ? { ...a, nsec: trimmed || undefined } : a
    );
    selectedAgent = { ...selectedAgent, nsec: trimmed || undefined };
    editingNsec = false;
    await saveConfig();
    showInfo(trimmed ? 'Agent nsec saved' : 'Agent nsec removed');
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
      <span class="text-xs text-yellow-400 animate-pulse">Saving...</span>
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
            <button class="w-full text-left" onclick={() => openAgent(agent)}>
              <ProfileCard
                pubkey={agent.profile?.pubkey || (() => { try { return nip19.decode(agent.npub).data as string; } catch { return ''; } })()}
                name={agent.profile?.name}
                displayName={agent.profile?.displayName}
                picture={agent.profile?.picture}
                about={agent.profile?.about}
                isBot={isBot(agent)}
                role={agent.nsec ? `${agent.role} 🔑` : agent.role}
              >
                <button
                  onclick={(e) => { e.stopPropagation(); toggleRole(agent.npub); }}
                  class="text-[11px] px-2 py-1 rounded-lg border transition-colors duration-150
                    {agent.role === 'guardian'
                      ? 'bg-purple-900/30 border-purple-800/50 text-purple-400 hover:bg-purple-900/50'
                      : 'bg-accent-600/10 border-accent-600/30 text-accent-400 hover:bg-accent-600/20'}"
                  title="Click to toggle role"
                >
                  {agent.role}
                </button>
                <button
                  onclick={(e) => { e.stopPropagation(); removeAgent(agent.npub); }}
                  class="w-9 h-9 flex items-center justify-center rounded-lg text-gray-600 hover:text-red-400 hover:bg-red-900/20 transition-colors duration-150"
                  title="Remove agent"
                  aria-label="Remove agent"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </ProfileCard>
            </button>
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
            placeholder="npub1... or nsec1... or user@domain.com"
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

<!-- Agent profile modal -->
{#if selectedAgent}
  {@const a = selectedAgent}
  {@const pubkey = a.profile?.pubkey || (() => { try { return nip19.decode(a.npub).data as string; } catch { return ''; } })()}
  {@const label = a.profile?.displayName || a.profile?.name || compressNpub(a.npub)}
  <dialog open class="fixed inset-0 z-50 flex items-center justify-center bg-transparent">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="fixed inset-0 bg-black/60" role="presentation" onclick={() => selectedAgent = null}></div>
    <div class="relative bg-gray-900 border border-gray-700 rounded-xl p-6 w-full max-w-sm shadow-2xl mx-4">
      <div class="flex flex-col items-center">
        {#if a.profile?.picture}
          <img src={a.profile.picture} alt="" class="w-24 h-24 rounded-full object-cover border-2 border-gray-600 mb-4" />
        {:else}
          <div class="w-24 h-24 rounded-full bg-accent-600 flex items-center justify-center text-4xl font-bold text-white mb-4">
            {label[0].toUpperCase()}
          </div>
        {/if}

        <h2 class="text-xl font-semibold text-gray-100">{label}</h2>

        {#if a.profile?.name && a.profile.name !== label}
          <p class="text-sm text-gray-500">@{a.profile.name}</p>
        {/if}

        <div class="flex items-center gap-2 mt-2">
          <code class="text-sm text-gray-400 font-mono">{compressNpub(a.npub)}</code>
          <button
            onclick={() => navigator.clipboard.writeText(a.npub)}
            class="w-9 h-9 flex items-center justify-center rounded-lg text-gray-500 hover:text-gray-300 hover:bg-gray-800 transition-colors"
            title="Copy npub"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
            </svg>
          </button>
        </div>

        <div class="flex items-center gap-2 mt-2 flex-wrap justify-center">
          <span class="text-[10px] px-1.5 py-0.5 rounded border transition-colors
            {a.role === 'guardian'
              ? 'bg-purple-900/30 border-purple-800/50 text-purple-400'
              : 'bg-accent-600/10 border-accent-600/30 text-accent-400'}">
            {a.role}
          </span>
          {#if a.meta?.bot === true}
            <span class="text-[10px] px-1.5 py-0.5 rounded bg-blue-900/40 text-blue-400 border border-blue-800/50">bot</span>
          {/if}
          {#if a.nsec}
            <span class="text-[10px] px-1.5 py-0.5 rounded bg-green-900/40 text-green-400 border border-green-800/50">🔑 nsec</span>
          {/if}
        </div>

        {#if a.profile?.about}
          <p class="text-sm text-gray-400 mt-3 text-center whitespace-pre-wrap">{a.profile.about}</p>
        {/if}

        {#if a.meta?.website}
          <a href={a.meta.website} target="_blank" rel="noopener" class="text-sm text-accent-400 hover:underline mt-2">{a.meta.website}</a>
        {/if}

        {#if a.meta?.lud16}
          <p class="text-xs text-gray-500 mt-2">⚡ {a.meta.lud16}</p>
        {/if}
      </div>

      <!-- Nsec management -->
      <div class="mt-4 p-3 rounded-lg border border-gray-700 bg-gray-800/30">
        <div class="flex items-center justify-between mb-2">
          <span class="text-xs font-medium text-gray-400">Agent Secret Key</span>
          {#if !editingNsec}
            <button
              onclick={() => { editingNsec = true; nsecInput = a.nsec || ''; }}
              class="text-xs text-accent-400 hover:text-accent-300"
            >
              {a.nsec ? 'Change' : 'Add nsec'}
            </button>
          {/if}
        </div>
        {#if editingNsec}
          <div class="space-y-2">
            <input
              type="password"
              bind:value={nsecInput}
              placeholder="nsec1..."
              class="w-full px-3 py-2 bg-gray-900 border border-gray-700 rounded-lg text-sm text-gray-200 placeholder-gray-600 focus:border-accent-500 transition-colors font-mono"
            />
            <p class="text-[10px] text-gray-600">Used to decrypt private memories. Stored in your config event on the relay.</p>
            <div class="flex gap-2">
              <button
                onclick={saveNsec}
                class="flex-1 py-2 rounded-lg bg-accent-600/20 border border-accent-600/50 text-accent-300 hover:bg-accent-600/30 text-sm transition-colors"
              >
                Save
              </button>
              <button
                onclick={() => editingNsec = false}
                class="flex-1 py-2 rounded-lg border border-gray-700 text-gray-400 hover:text-gray-200 hover:bg-gray-800 text-sm transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        {:else if a.nsec}
          <p class="text-xs text-green-400/70 font-mono">nsec1•••••••{a.nsec.slice(-6)}</p>
        {:else}
          <p class="text-xs text-gray-600">No nsec configured — private memories won't be decryptable.</p>
        {/if}
      </div>

      <div class="mt-4">
        <button
          onclick={() => selectedAgent = null}
          class="w-full py-2.5 min-h-11 rounded-lg border border-gray-700 text-gray-400 hover:text-gray-200 hover:bg-gray-800 transition-colors text-sm"
        >
          Close
        </button>
      </div>
    </div>
  </dialog>
{/if}
