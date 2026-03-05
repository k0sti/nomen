<script lang="ts">
  import Layout from './components/Layout.svelte';
  import Memories from './pages/Memories.svelte';
  import Search from './pages/Search.svelte';
  import Messages from './pages/Messages.svelte';
  import Groups from './pages/Groups.svelte';
  import Agents from './pages/Agents.svelte';
  import Settings from './pages/Settings.svelte';
  import LoginModal from './components/LoginModal.svelte';
  import ProfileModal from './components/ProfileModal.svelte';
  import { currentPage } from './lib/stores';
</script>

<Layout>
  {#if $currentPage === 'memories'}
    <Memories />
  {:else if $currentPage === 'search'}
    <Search />
  {:else if $currentPage === 'messages'}
    <Messages />
  {:else if $currentPage === 'groups'}
    <Groups />
  {:else if $currentPage === 'agents'}
    <Agents />
  {:else if $currentPage === 'settings'}
    <Settings />
  {/if}
</Layout>

<LoginModal />
<ProfileModal />
