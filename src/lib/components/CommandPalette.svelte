<script lang="ts">
  import { tick } from "svelte";
  import {
    Download,
    Folder,
    FolderPlus,
    MessageSquare,
    MessageSquarePlus,
    PanelLeft,
    Search,
    Settings,
  } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import { commandPaletteOpen, osTag } from "$lib/stores/app";
  import { config } from "$lib/stores/config";
  import { openSettings, openSettingsTab } from "$lib/actions";
  import { chats, currentChatId, newChat, openChat, exportMarkdown } from "$lib/stores/chat";
  import { projects, currentProjectId, openProject } from "$lib/stores/project";
  import { formatHotkey } from "$lib/hotkeys";
  import { EXPAND_PROJECT_EVENT } from "$lib/events";
  import { newProjectAction, toggleSidebar } from "$lib/actions";

  interface Command {
    id: string;
    label: string;
    icon: typeof MessageSquare;
    section: "commands" | "recent" | "projects";
    hotkeyName?: string;
    action: () => void;
  }

  let query = $state("");
  let active = $state(0);
  let inputEl: HTMLInputElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();

  let isMac = $derived($osTag === "mac");
  let hotkeys = $derived($config?.hotkeys ?? {});

  let commands = $derived.by<Command[]>(() => {
    const list: Command[] = [
      {
        id: "new_chat",
        label: m.cmd_new_chat(),
        icon: MessageSquarePlus,
        section: "commands",
        hotkeyName: "new_chat",
        action: () => void newChat(),
      },
      {
        id: "new_project",
        label: m.cmd_new_project(),
        icon: FolderPlus,
        section: "commands",
        hotkeyName: "new_project",
        action: () => void newProjectAction(),
      },
    ];
    if ($currentProjectId) {
      list.push({
        id: "new_chat_in_project",
        label: m.cmd_new_chat_in_project(),
        icon: MessageSquarePlus,
        section: "commands",
        action: () => void newChat($currentProjectId),
      });
    }
    if ($currentChatId) {
      list.push({
        id: "export_chat",
        label: m.cmd_export_chat(),
        icon: Download,
        section: "commands",
        action: () => void exportMarkdown(),
      });
    }
    list.push(
      {
        id: "toggle_sidebar",
        label: m.cmd_toggle_sidebar(),
        icon: PanelLeft,
        section: "commands",
        hotkeyName: "toggle_sidebar",
        action: () => toggleSidebar(),
      },
      {
        id: "open_settings",
        label: m.cmd_open_settings(),
        icon: Settings,
        section: "commands",
        hotkeyName: "settings",
        action: () => openSettings(),
      },
    );
    for (const [tab, label] of [
      ["general", m.cmd_settings_general()],
      ["providers", m.cmd_settings_providers()],
      ["models", m.cmd_settings_models()],
      ["mcp", m.cmd_settings_mcp()],
      ["tools", m.cmd_settings_tools()],
      ["agents", m.cmd_settings_agents()],
      ["appearance", m.cmd_settings_appearance()],
      ["data", m.cmd_settings_data()],
    ] as const) {
      list.push({
        id: `open_settings_${tab}`,
        label,
        icon: Settings,
        section: "commands",
        action: () => openSettingsTab(tab),
      });
    }
    for (const c of $chats.slice(0, 10)) {
      list.push({
        id: `recent:${c.id}`,
        label: c.title,
        icon: MessageSquare,
        section: "recent",
        action: () => void openChat(c.id),
      });
    }
    for (const p of $projects) {
      list.push({
        id: `project:${p.id}`,
        label: p.name,
        icon: Folder,
        section: "projects",
        action: () => {
          void openProject(p.id);
          window.dispatchEvent(new CustomEvent(EXPAND_PROJECT_EVENT, { detail: p.id }));
        },
      });
    }
    return list;
  });

  let filtered = $derived(commands.filter((c) => matches(c.label, query)));

  function matches(text: string, q: string): boolean {
    const needle = q.trim().toLowerCase();
    if (!needle) return true;
    const hay = text.toLowerCase();
    if (hay.includes(needle)) return true;
    let i = 0;
    for (const ch of hay) {
      if (ch === needle[i]) i += 1;
      if (i === needle.length) return true;
    }
    return false;
  }

  $effect(() => {
    if ($commandPaletteOpen) {
      query = "";
      active = 0;
      void tick().then(() => inputEl?.focus());
    }
  });

  function close() {
    commandPaletteOpen.set(false);
  }

  function runByIndex(i: number) {
    const cmd = filtered[i];
    if (cmd) {
      close();
      void cmd.action();
    }
  }

  function onInputKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      active = Math.min(active + 1, filtered.length - 1);
      void tick().then(scrollActiveIntoView);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      active = Math.max(active - 1, 0);
      void tick().then(scrollActiveIntoView);
    } else if (e.key === "Enter") {
      e.preventDefault();
      runByIndex(active);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }

  function scrollActiveIntoView() {
    listEl?.querySelector(`[data-idx="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }

  function sectionTitle(section: Command["section"]): string {
    if (section === "recent") return m.cmd_recent_chats();
    if (section === "projects") return m.cmd_projects();
    return "";
  }
</script>

{#if $commandPaletteOpen}
  <div
    class="overlay"
    role="presentation"
    onclick={close}
    onkeydown={(e) => e.key === "Escape" && close()}
  >
    <div
      class="palette"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.key === "Escape" && close()}
    >
      <div class="search-row">
        <Search size={15} />
        <input
          bind:this={inputEl}
          bind:value={query}
          oninput={() => (active = 0)}
          onkeydown={onInputKeydown}
          placeholder={m.palette_placeholder()}
          spellcheck="false"
        />
      </div>
      <div class="list" bind:this={listEl}>
        {#each filtered as cmd, i (cmd.id)}
          {#if sectionTitle(cmd.section) && (i === 0 || filtered[i - 1].section !== cmd.section)}
            <div class="section-label">{sectionTitle(cmd.section)}</div>
          {/if}
          {@const Icon = cmd.icon}
          {@const hk = cmd.hotkeyName ? formatHotkey(hotkeys[cmd.hotkeyName] ?? "", isMac) : ""}
          <button
            class="row"
            class:active={i === active}
            data-idx={i}
            onmouseover={() => (active = i)}
            onfocus={() => (active = i)}
            onclick={() => runByIndex(i)}
          >
            <Icon size={15} />
            <span class="row-label">{cmd.label}</span>
            {#if hk}
              <span class="row-hk">{hk}</span>
            {/if}
          </button>
        {/each}
        {#if filtered.length === 0}
          <div class="empty">{m.palette_empty()}</div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 12vh;
    background: rgb(0 0 0 / 0.35);
  }
  .palette {
    display: flex;
    flex-direction: column;
    width: min(560px, calc(100vw - 2rem));
    max-height: 60vh;
    background: var(--popover);
    color: var(--popover-foreground);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.3);
    overflow: hidden;
  }
  .search-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    height: 2.75rem;
    padding: 0 0.75rem;
    border-bottom: 1px solid var(--border);
    color: var(--muted-foreground);
    flex-shrink: 0;
  }
  .search-row input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    font-size: 0.875rem;
    color: var(--foreground);
    font-family: var(--font-sans);
  }
  .list {
    flex: 1;
    overflow-y: auto;
    padding: 0.25rem 0.25rem 0.4rem;
  }
  .section-label {
    padding: 0.5rem 0.6rem 0.2rem;
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted-foreground);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    width: 100%;
    padding: 0.4rem 0.6rem;
    border: none;
    background: transparent;
    color: inherit;
    font-size: 0.8125rem;
    text-align: left;
    border-radius: var(--radius-md);
    cursor: default;
  }
  .row:hover,
  .row.active {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .row-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-hk {
    font-size: 0.7rem;
    color: var(--muted-foreground);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.05rem 0.3rem;
    flex-shrink: 0;
  }
  .empty {
    padding: 0.9rem 0.6rem;
    font-size: 0.8125rem;
    color: var(--muted-foreground);
    text-align: center;
  }
</style>