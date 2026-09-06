<script lang="ts">
  import { onMount } from "svelte";
  import { settingsTab } from "$lib/stores/settings";
  import { theme, applyTheme, type Theme } from "$lib/stores/theme";
  import {
    loadLanguageSetting,
    saveLanguageSetting,
    applyLanguageSetting,
    SUPPORTED_LOCALES,
    type LanguageSetting,
    type Locale,
  } from "$lib/stores/lang";
  import { m } from "$lib/i18n";
  import {
    setTheme,
    takeSettingsTab,
    onSettingsNavigate,
    emitLangChanged,
  } from "$lib/tauri";
  import SettingsProviders from "./SettingsProviders.svelte";
  import SettingsModels from "./SettingsModels.svelte";
  import SettingsMcp from "./SettingsMcp.svelte";
  import SettingsTools from "./SettingsTools.svelte";
  import SettingsAgents from "./SettingsAgents.svelte";
  import SettingsPrompts from "./SettingsPrompts.svelte";
  import SettingsWebSearch from "./SettingsWebSearch.svelte";
  import SettingsWebHooks from "./SettingsWebHooks.svelte";
  import SettingsSkills from "./SettingsSkills.svelte";
  import SettingsNetwork from "./SettingsNetwork.svelte";
  import SettingsLogs from "./SettingsLogs.svelte";
  import SettingsAbout from "./SettingsAbout.svelte";
  import Select from "./Select.svelte";

  const tabs = [
    { id: "providers", label: m.settings_tab_providers() },
    { id: "models", label: m.settings_tab_models() },
    { id: "mcp", label: m.settings_tab_mcp() },
    { id: "tools", label: m.settings_tab_tools() },
    { id: "agents", label: m.settings_tab_agents() },
    { id: "prompts", label: m.settings_tab_prompts() },
    { id: "web_search", label: m.settings_tab_web_search() },
    { id: "web_hooks", label: m.settings_tab_web_hooks() },
    { id: "skills", label: m.settings_tab_skills() },
    { id: "appearance", label: m.settings_tab_appearance() },
    { id: "network", label: m.settings_tab_network() },
    { id: "logs", label: m.settings_tab_logs() },
    { id: "about", label: m.settings_tab_about() },
  ] as const;

  const themes: Theme[] = ["system", "light", "dark"];
  const languageOptions: { value: LanguageSetting; label: string }[] = [
    { value: "system", label: m.settings_appearance_language_system() },
    ...(Object.keys(SUPPORTED_LOCALES) as Locale[]).map((code) => ({
      value: code,
      label: SUPPORTED_LOCALES[code],
    })),
  ];

  let langSetting = $state<LanguageSetting>("system");

  let currentTabLabel = $derived(
    tabs.find((t) => t.id === $settingsTab)?.label ?? "",
  );

  onMount(() => {
    langSetting = loadLanguageSetting();
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

  async function chooseLanguage(value: string) {
    const setting: LanguageSetting =
      value in SUPPORTED_LOCALES ? (value as Locale) : "system";
    langSetting = setting;
    saveLanguageSetting(setting);
    // paraglide reloads this window; notify the main window to reload too.
    await emitLangChanged();
    await applyLanguageSetting(setting);
  }

  async function chooseTheme(t: Theme) {
    theme.set(t);
    applyTheme(t);
    try {
      await setTheme(t);
    } catch {
      // Ignore write errors; config is still applied in-memory.
    }
  }
</script>

<div class="settings-win">
  <nav class="settings-nav">
    {#each tabs as t}
      <button
        class="settings-nav-item"
        class:active={$settingsTab === t.id}
        onclick={() => settingsTab.set(t.id)}
      >
        {t.label}
      </button>
    {/each}
  </nav>

  <section class="settings-content">
    {#if $settingsTab === "appearance"}
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
        <div>
          <div class="mb-2 text-sm font-medium">{m.settings_appearance_language()}</div>
          <Select
            class="w-full max-w-45"
            value={langSetting}
            items={languageOptions}
            onchange={(v) => void chooseLanguage(v)}
          />
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
    {:else if $settingsTab === "about"}
      <SettingsAbout />
    {:else if $settingsTab === "prompts"}
      <SettingsPrompts />
    {:else if $settingsTab === "web_search"}
      <SettingsWebSearch />
    {:else if $settingsTab === "web_hooks"}
      <SettingsWebHooks />
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
    width: 160px;
    border-right: 1px solid var(--border);
    padding: 0.5rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
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