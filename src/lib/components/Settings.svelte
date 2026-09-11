<script lang="ts">
  import { onMount } from "svelte";
  import { settingsTab } from "$lib/stores/settings";
  import { settingsNavWidth, setSettingsNavWidth, saveSettingsNavWidth } from "$lib/stores/layout";
  import { theme, applyTheme, type Theme } from "$lib/stores/theme";
  import { m } from "$lib/i18n";
  import { setTheme, takeSettingsTab, onSettingsNavigate } from "$lib/tauri";
  import SettingsGeneral from "./SettingsGeneral.svelte";
  import SettingsData from "./SettingsData.svelte";
  import SettingsProviders from "./SettingsProviders.svelte";
  import SettingsModels from "./SettingsModels.svelte";
  import SettingsMcp from "./SettingsMcp.svelte";
  import SettingsTools from "./SettingsTools.svelte";
  import SettingsAgents from "./SettingsAgents.svelte";
  import SettingsPrompts from "./SettingsPrompts.svelte";
  import SettingsWebSearch from "./SettingsWebSearch.svelte";
  import SettingsWebHooks from "./SettingsWebHooks.svelte";
  import SettingsRules from "./SettingsRules.svelte";
  import SettingsSkills from "./SettingsSkills.svelte";
  import SettingsNetwork from "./SettingsNetwork.svelte";
  import SettingsLogs from "./SettingsLogs.svelte";
  import SettingsAbout from "./SettingsAbout.svelte";

  const tabs = [
    { id: "general", label: m.settings_tab_general() },
    { id: "providers", label: m.settings_tab_providers() },
    { id: "models", label: m.settings_tab_models() },
    { id: "mcp", label: m.settings_tab_mcp() },
    { id: "tools", label: m.settings_tab_tools() },
    { id: "agents", label: m.settings_tab_agents() },
    { id: "prompts", label: m.settings_tab_prompts() },
    { id: "web_search", label: m.settings_tab_web_search() },
    { id: "web_hooks", label: m.settings_tab_web_hooks() },
    { id: "rules", label: m.settings_tab_rules() },
    { id: "skills", label: m.settings_tab_skills() },
    { id: "appearance", label: m.settings_tab_appearance() },
    { id: "network", label: m.settings_tab_network() },
    { id: "logs", label: m.settings_tab_logs() },
    { id: "data", label: m.settings_tab_data() },
    { id: "about", label: m.settings_tab_about() },
  ] as const;

  const themes: Theme[] = ["system", "light", "dark"];

  let currentTabLabel = $derived(
    tabs.find((t) => t.id === $settingsTab)?.label ?? "",
  );

  onMount(() => {
    // Tab requested by the opener for a freshly-created window.
    void takeSettingsTab().then((t) => {
      if (t) settingsTab.set(t);
    });
  });

  // Tab switches requested while the window is already open.
  onMount(() => {
    let unlisten: (() => void) | undefined;
    void onSettingsNavigate((t) => settingsTab.set(t)).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  });

  async function chooseTheme(t: Theme) {
    theme.set(t);
    applyTheme(t);
    try {
      await setTheme(t);
    } catch {
      // Ignore write errors; config is still applied in-memory.
    }
  }

  let navEl: HTMLElement | undefined = $state();
  let resizing = $state(false);

  function startResize(e: PointerEvent) {
    e.preventDefault();
    resizing = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onResizeMove(e: PointerEvent) {
    if (!resizing || !navEl) return;
    const rect = navEl.getBoundingClientRect();
    setSettingsNavWidth(e.clientX - rect.left);
  }
  function onResizeUp(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId); } catch {}
    saveSettingsNavWidth($settingsNavWidth);
  }
</script>

<div class="settings-win">
  <nav class="settings-nav" bind:this={navEl} style="width:{$settingsNavWidth}px">
    {#each tabs as t}
      <button
        class="settings-nav-item"
        class:active={$settingsTab === t.id}
        onclick={() => settingsTab.set(t.id)}
      >
        {t.label}
      </button>
    {/each}
    <div
      class="resize-handle"
      class:active={resizing}
      role="separator"
      aria-orientation="vertical"
      tabindex="-1"
      onpointerdown={startResize}
      onpointermove={onResizeMove}
      onpointerup={onResizeUp}
      onpointercancel={onResizeUp}
    ></div>
  </nav>

  <section class="settings-content">
    {#if $settingsTab === "general"}
      <SettingsGeneral />
    {:else if $settingsTab === "appearance"}
      <div class="space-y-4">
        <div>
          <div class="mb-2 text-sm font-medium">
            {m.settings_appearance_theme()}
          </div>
          <div class="flex gap-2">
            {#each themes as t}
              <button
                class="theme-chip"
                class:active={$theme === t}
                onclick={() => chooseTheme(t)}
              >
                {t === "system"
                  ? m.settings_appearance_theme_system()
                  : t === "light"
                    ? m.settings_appearance_theme_light()
                    : m.settings_appearance_theme_dark()}
              </button>
            {/each}
          </div>
        </div>
      </div>
    {:else if $settingsTab === "network"}
      <SettingsNetwork />
    {:else if $settingsTab === "providers"}
      <SettingsProviders />
    {:else if $settingsTab === "models"}
      <SettingsModels />
    {:else if $settingsTab === "mcp"}
      <SettingsMcp />
    {:else if $settingsTab === "tools"}
      <SettingsTools />
    {:else if $settingsTab === "agents"}
      <SettingsAgents />
    {:else if $settingsTab === "logs"}
      <SettingsLogs />
    {:else if $settingsTab === "data"}
      <SettingsData />
    {:else if $settingsTab === "about"}
      <SettingsAbout />
    {:else if $settingsTab === "prompts"}
      <SettingsPrompts />
    {:else if $settingsTab === "web_search"}
      <SettingsWebSearch />
    {:else if $settingsTab === "web_hooks"}
      <SettingsWebHooks />
    {:else if $settingsTab === "rules"}
      <SettingsRules />
    {:else if $settingsTab === "skills"}
      <SettingsSkills />
    {:else}
      <div class="text-sm text-muted-foreground">
        {currentTabLabel}
      </div>
    {/if}
  </section>
</div>

<style>
  .settings-win {
    display: flex;
    height: 100vh;
    width: 100vw;
    overflow: hidden;
  }
  .settings-nav {
    border-right: 1px solid var(--border);
    padding: 0.5rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    flex-shrink: 0;
    position: relative;
  }
  .settings-nav-item {
    text-align: left;
    padding: 0.375rem 0.625rem;
    border-radius: var(--radius-md);
    font-size: 0.8125rem;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    cursor: default;
  }
  .settings-nav-item:hover {
    background-color: var(--accent);
    color: var(--accent-foreground);
  }
  .settings-nav-item.active {
    background-color: var(--secondary);
    color: var(--secondary-foreground);
  }
  .resize-handle {
    position: absolute;
    top: 0;
    right: -3px;
    bottom: 0;
    width: 7px;
    z-index: 10;
    cursor: col-resize;
    background: transparent;
  }
  .resize-handle:hover,
  .resize-handle.active {
    background-color: var(--ring);
    opacity: 0.35;
  }
  .settings-content {
    flex: 1;
    padding: 1rem 1.25rem;
    overflow-y: auto;
  }
  .theme-chip {
    padding: 0.375rem 0.75rem;
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
    background-color: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    cursor: default;
  }
  .theme-chip.active {
    border-color: var(--primary);
    color: var(--primary);
  }
  .space-y-4 > :global(*) + :global(*) {
    margin-top: 1rem;
  }
</style>