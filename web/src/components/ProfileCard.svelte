<script lang="ts">
  import { compressNpub } from '../lib/nostr';
  import { nip19 } from 'nostr-tools';

  interface Props {
    pubkey: string;
    name?: string;
    displayName?: string;
    picture?: string;
    about?: string;
    nip05?: string;
    isBot?: boolean;
    isAgent?: boolean;
    isYou?: boolean;
    role?: string;
    ownerPubkey?: string;
    agentCount?: number;
    children?: import('svelte').Snippet;
  }

  let {
    pubkey,
    name,
    displayName,
    picture,
    about,
    nip05,
    isBot = false,
    isAgent = false,
    isYou = false,
    role,
    ownerPubkey,
    agentCount,
    children,
  }: Props = $props();

  let copied = $state(false);

  const npub = $derived(nip19.npubEncode(pubkey));
  const npubShort = $derived(compressNpub(npub));
  const label = $derived(displayName || name || npubShort);

  function copyNpub() {
    navigator.clipboard.writeText(npub);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

<div class="flex items-center gap-3 p-3 rounded-xl border border-gray-700 bg-gray-800/30 hover:bg-gray-800/50 transition-colors duration-150">
  <!-- Avatar -->
  {#if picture}
    <img src={picture} alt="" class="w-10 h-10 rounded-full object-cover shrink-0" />
  {:else}
    <div class="w-10 h-10 rounded-full bg-accent-600/30 flex items-center justify-center text-accent-400 font-bold shrink-0">
      {label[0].toUpperCase()}
    </div>
  {/if}

  <!-- Info -->
  <div class="flex-1 min-w-0">
    <div class="flex items-center gap-2 flex-wrap">
      <span class="text-sm font-medium text-gray-100 truncate">{label}</span>
      {#if isYou}
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-green-900/40 text-green-400 border border-green-800/50">you</span>
      {/if}
      {#if isBot}
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-blue-900/40 text-blue-400 border border-blue-800/50">bot</span>
      {/if}
      {#if isAgent}
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-accent-600/20 text-accent-400 border border-accent-600/40">agent</span>
      {/if}
      {#if role}
        <span class="text-[10px] px-1.5 py-0.5 rounded border transition-colors duration-150
          {role === 'guardian'
            ? 'bg-purple-900/30 border-purple-800/50 text-purple-400'
            : role === 'profile'
            ? 'bg-teal-900/30 border-teal-800/50 text-teal-400'
            : role.includes('mutual')
            ? 'bg-green-900/30 border-green-800/50 text-green-400'
            : role.includes('claimed')
            ? 'bg-amber-900/30 border-amber-800/50 text-amber-400'
            : role.includes('recognized')
            ? 'bg-teal-900/30 border-teal-800/50 text-teal-400'
            : role.includes('none')
            ? 'bg-gray-700/30 border-gray-600/50 text-gray-500'
            : 'bg-accent-600/10 border-accent-600/30 text-accent-400'}">
          {role}
        </span>
      {/if}
      {#if agentCount != null && agentCount > 0}
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-gray-700/50 text-gray-400 border border-gray-600/50">{agentCount} agents</span>
      {/if}
    </div>
    <div class="flex items-center gap-1">
      <button
        onclick={copyNpub}
        class="text-xs text-gray-500 font-mono hover:text-gray-300 transition-colors duration-150 cursor-pointer"
        title="Copy npub"
      >
        {copied ? 'copied!' : npubShort}
      </button>
    </div>
    {#if nip05}
      <div class="text-[11px] text-gray-500">{nip05}</div>
    {/if}
    {#if ownerPubkey}
      <div class="text-[10px] text-green-500">Owner: {compressNpub(nip19.npubEncode(ownerPubkey))}</div>
    {/if}
    {#if about}
      <p class="text-xs text-gray-500 mt-1 truncate">{about}</p>
    {/if}
  </div>

  <!-- Action slot -->
  {#if children}
    {@render children()}
  {/if}
</div>
