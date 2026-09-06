<script lang="ts">
  import Toolbar from "$lib/components/Toolbar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ProjectSettings from "$lib/components/ProjectSettings.svelte";
  import ChatView from "$lib/components/ChatView.svelte";
  import Toaster from "$lib/components/Toaster.svelte";
  import Home from "$lib/components/Home.svelte";
  import ProjectBoard from "$lib/components/ProjectBoard.svelte";
  import PromptsView from "$lib/components/PromptsView.svelte";
  import { chats, currentChatId } from "$lib/stores/chat";
  import { homeView, sidebarVisible, boardProjectId, promptsView } from "$lib/stores/app";

  $: current = $chats.find((c) => c.id === $currentChatId) ?? null;
</script>

<div class="flex h-screen w-screen flex-col overflow-hidden">
  <Toolbar />

  <div class="flex min-h-0 flex-1">
    {#if $sidebarVisible}
      <Sidebar />
    {/if}

    <main class="flex min-w-0 flex-1 flex-col bg-background">
      {#if $homeView}
        <Home />
      {:else if $promptsView}
        <PromptsView />
      {:else if $boardProjectId}
        <ProjectBoard projectId={$boardProjectId} />
      {:else if current}
        <ChatView chat={current} />
      {:else}
        <Home />
      {/if}
    </main>
  </div>

  <ProjectSettings />
  <Toaster />
</div>