<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
import { PanelLeft, Settings, Globe, Home, MessageSquareText } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import { goHome, openPromptsView, openSettings, toggleSidebar } from "$lib/actions";
  import { mcpWebUiList, mcpWebUiOpen, onMcpChanged, type McpWebUiEntry } from "$lib/tauri";
  import { theme } from "$lib/stores/theme";
  import { homeView, promptsView } from "$lib/stores/app";

  let webuis = $state<McpWebUiEntry[]>([]);
  let unlisten: (() => void) | undefined;

  async function load() {
    try {
      webuis = await mcpWebUiList();
    } catch {
      webuis = [];
    }
  }

  onMount(() => {
    load();
    onMcpChanged(() => load()).then((u) => (unlisten = () => u()));
  });
  onDestroy(() => unlisten?.());
</script>

<header class="toolbar flex h-[var(--header-height)] shrink-0 items-center gap-1 border-b border-border bg-sidebar px-2">
  <button class="toolbar-btn" title={m.cmd_toggle_sidebar()} onclick={() => toggleSidebar()}>
    <PanelLeft size={16} />
  </button>

  <button class="toolbar-btn" class:active={$homeView} title={m.home_title()} onclick={() => goHome()}>
    <Home size={16} />
  </button>

  <button class="toolbar-btn" class:active={$promptsView} title={m.prompts_title()} onclick={() => openPromptsView()}>
    <MessageSquareText size={16} />
  </button>

  <div class="flex-1"></div>

  {#each webuis as w (w.server_id)}
    <button
      class="toolbar-btn"
      title={w.title}
      onclick={() => mcpWebUiOpen(w.server_id, get(theme))}
    >
      {#if w.icon_data_url}
        <img class="webui-icon" src={w.icon_data_url} alt={w.title} />
      {:else}
        <Globe size={16} />
      {/if}
    </button>
  {/each}

  <button class="toolbar-btn" title={m.settings_title()} onclick={() => openSettings()}>
    <Settings size={16} />
  </button>
</header>

<style>
  .toolbar-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 1.75rem;
    width: 1.75rem;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .toolbar-btn:hover {
    background-color: var(--accent);
    color: var(--accent-foreground);
  }
  .toolbar-btn.active {
    background-color: var(--accent);
    color: var(--accent-foreground);
  }
  .webui-icon {
    width: 16px;
    height: 16px;
    object-fit: contain;
    border-radius: 3px;
  }
</style>