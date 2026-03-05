<script lang="ts">
  import Layout from './components/Layout.svelte';
  import Landing from './pages/Landing.svelte';
  import Memories from './pages/Memories.svelte';
  import Search from './pages/Search.svelte';
  import Messages from './pages/Messages.svelte';
  import Groups from './pages/Groups.svelte';
  import Members from './pages/Members.svelte';
  import Agents from './pages/Agents.svelte';
  import Settings from './pages/Settings.svelte';
  import LoginModal from './components/LoginModal.svelte';
  import ProfileModal from './components/ProfileModal.svelte';
  import Toasts from './components/Toasts.svelte';
  import { currentPage } from './lib/stores';
</script>

{#if $currentPage === 'landing'}
  <Landing />
{:else}
  <Layout>
    {#if $currentPage === 'memories'}
      <Memories />
    {:else if $currentPage === 'search'}
      <Search />
    {:else if $currentPage === 'messages'}
      <Messages />
    {:else if $currentPage === 'members'}
      <Members />
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
{/if}
<Toasts />
