<script lang="ts">
  import {
    Plus,
    Search,
    MessageSquare,
    Trash2,
    FolderPlus,
    ChevronRight,
    MoreVertical,
    Star,
    Pencil,
    Settings,
  } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import ContextMenu, { type ContextMenuItem } from "./ContextMenu.svelte";
  import RenameModal from "./RenameModal.svelte";
  import Select from "./Select.svelte";
  import {
    chats,
    currentChatId,
    statusByChat,
    openChat,
    newChat,
    deleteChat,
    renameChat,
    moveChatToProject,
    toggleChatPinned,
    reorderChats,
    chatSortMode,
    sortChats,
    type ChatSortMode,
  } from "$lib/stores/chat";
  import {
    projects,
    projectChats,
    currentProjectId,
    loadProjects,
    createProject,
    openProject,
    openProjectSettings,
    deleteProject,
    renameProject,
    toggleProjectPinned,
    reorderProjects,
    projectSortMode,
    sortProjects,
    type ProjectSortMode,
  } from "$lib/stores/project";
  import * as ipc from "$lib/tauri";
  import type { Chat, ChatStatus, FtsHit, Project } from "$lib/tauri";
  import { onMount } from "svelte";
  import { EXPAND_PROJECT_EVENT } from "$lib/events";
  import { sidebarWidth, setSidebarWidth, saveSidebarWidth } from "$lib/stores/layout";
  import { homeView, boardProjectId, promptsView } from "$lib/stores/app";
  import { loadProjectTasks } from "$lib/stores/projectTasks";

  let query = $state("");
  let ftsResults = $state<FtsHit[]>([]);
  let ftsOpen = $state(false);
  let expanded = $state<Record<string, boolean>>({});
  let searchTimer: ReturnType<typeof setTimeout> | undefined = undefined;

  let asideEl: HTMLElement | undefined = $state();
  let resizing = $state(false);

  let ctxMenu = $state<{ x: number; y: number; items: ContextMenuItem[] } | null>(null);
  let renameTarget = $state<{ kind: "chat" | "project"; id: string; name: string } | null>(null);
  let createProjectOpen = $state(false);

  let dragId = $state<string | null>(null);
  let dragOverId = $state<string | null>(null);
  let dragOverProjectId = $state<string | null>(null);

  function startResize(e: PointerEvent) {
    e.preventDefault();
    resizing = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onResizeMove(e: PointerEvent) {
    if (!resizing || !asideEl) return;
    setSidebarWidth(e.clientX - asideEl.getBoundingClientRect().left);
  }
  function onResizeUp(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId); } catch {}
    saveSidebarWidth($sidebarWidth);
  }

  function statusClass(s: ChatStatus | undefined): string {
    return s ?? "idle";
  }

  let standaloneChats = $derived($chats.filter((c) => !c.project_id));

  $effect(() => {
    const q = query.trim();
    if (searchTimer) clearTimeout(searchTimer);
    if (!q) {
      ftsResults = [];
      ftsOpen = false;
      return;
    }
    searchTimer = setTimeout(async () => {
      try {
        ftsResults = await ipc.searchMessages(q, null);
        ftsOpen = true;
      } catch (e) {
        console.error("searchMessages failed", e);
        ftsResults = [];
      }
    }, 250);
  });

  let titleFiltered = $derived(
    query.trim()
      ? standaloneChats.filter((c) =>
          c.title.toLowerCase().includes(query.trim().toLowerCase()),
        )
      : standaloneChats,
  );

  let listChats = $derived(sortChats(titleFiltered, $chatSortMode));
  let listProjects = $derived(sortProjects($projects, $projectSortMode));

  function onCreateProject() {
    createProjectOpen = true;
  }

  async function onCreateProjectSave(name: string) {
    createProjectOpen = false;
    const project = await createProject(name);
    if (project) {
      expanded[project.id] = true;
      expanded = { ...expanded };
    }
  }

  async function onNewProjectChat(projectId: string) {
    await newChat(projectId);
    expanded[projectId] = true;
    expanded = { ...expanded };
  }

  function toggleProject(id: string) {
    void openProject(id);
    expanded[id] = !expanded[id];
    expanded = { ...expanded };
  }

  function openProjectBoard(id: string) {
    void openProject(id);          // sets currentProjectId + loads project chats
    currentChatId.set(null);
    homeView.set(false);
    promptsView.set(false);
    boardProjectId.set(id);
    expanded[id] = true;
    expanded = { ...expanded };
    void loadProjectTasks(id);
  }

  // Auto-expand the owning project when one of its chats becomes active
  // (e.g. a chat created by "run task" on the board).
  $effect(() => {
    const id = $currentChatId;
    if (!id) return;
    const chat = $chats.find((c) => c.id === id);
    if (chat?.project_id && !expanded[chat.project_id]) {
      expanded[chat.project_id] = true;
      expanded = { ...expanded };
    }
  });

  const sortItems = [
    { value: "updated", label: m.sidebar_sort_updated() },
    { value: "created", label: m.sidebar_sort_created() },
    { value: "alpha", label: m.sidebar_sort_alpha() },
    { value: "manual", label: m.sidebar_sort_manual() },
  ];

  function setChatSort(value: string) {
    chatSortMode.set(value as ChatSortMode);
  }

  function setProjectSort(value: string) {
    projectSortMode.set(value as ProjectSortMode);
  }

  function openCtx(e: MouseEvent, items: ContextMenuItem[]) {
    e.preventDefault();
    e.stopPropagation();
    ctxMenu = { x: e.clientX, y: e.clientY, items };
  }

  function chatMenuItems(c: Chat): ContextMenuItem[] {
    return [
      {
        label: c.pinned ? m.ctx_unpin() : m.ctx_pin(),
        icon: Star,
        onclick: () => void toggleChatPinned(c.id, !c.pinned),
      },
      {
        label: m.ctx_rename(),
        icon: Pencil,
        onclick: () => {
          renameTarget = { kind: "chat", id: c.id, name: c.title };
        },
      },
      { label: "", separator: true },
      {
        label: m.ctx_delete(),
        icon: Trash2,
        danger: true,
        onclick: () => void deleteChat(c.id),
      },
    ];
  }

  function projectMenuItems(p: Project): ContextMenuItem[] {
    return [
      {
        label: p.pinned ? m.ctx_unpin() : m.ctx_pin(),
        icon: Star,
        onclick: () => void toggleProjectPinned(p.id, !p.pinned),
      },
      {
        label: m.ctx_rename(),
        icon: Pencil,
        onclick: () => {
          renameTarget = { kind: "project", id: p.id, name: p.name };
        },
      },
      {
        label: m.ctx_settings(),
        icon: Settings,
        onclick: () => openProjectSettings(p.id),
      },
      { label: "", separator: true },
      {
        label: m.ctx_delete(),
        icon: Trash2,
        danger: true,
        onclick: () => {
          if (window.confirm(m.common_delete() + ": " + p.name + "?")) void deleteProject(p.id);
        },
      },
    ];
  }

  async function onRenameSave(name: string) {
    const target = renameTarget;
    renameTarget = null;
    if (!target) return;
    if (target.kind === "chat") await renameChat(target.id, name);
    else await renameProject(target.id, name);
  }

  function onDragStart(e: DragEvent, id: string) {
    dragId = id;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", id);
    }
  }

  function clearDragState() {
    dragId = null;
    dragOverId = null;
    dragOverProjectId = null;
  }

  function onDragOverChatRow(e: DragEvent, id: string) {
    if (!dragId) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dragOverId = id;
    dragOverProjectId = null;
  }

  function onDragOverProjectHead(e: DragEvent, id: string) {
    if (!dragId) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dragOverProjectId = id;
    dragOverId = null;
  }

  function spliceMove(ids: string[], from: string, target: string): string[] | null {
    const next = [...ids];
    const fi = next.indexOf(from);
    const ti = next.indexOf(target);
    if (fi < 0 || ti < 0 || fi === ti) return null;
    next.splice(fi, 1);
    next.splice(next.indexOf(target), 0, from);
    return next.some((id, i) => id !== ids[i]) ? next : null;
  }

  async function dropOnChatRow(e: DragEvent, targetId: string, groupId: string | null) {
    e.preventDefault();
    const from = dragId;
    clearDragState();
    if (!from || from === targetId) return;
    const dragged = $chats.find((c) => c.id === from);
    if (!dragged) return;
    if (groupId !== null) {
      if (dragged.project_id === groupId) {
        const ids = sortChats($projectChats[groupId] ?? [], $chatSortMode).map((c) => c.id);
        const next = spliceMove(ids, from, targetId);
        if (next) {
          if ($chatSortMode !== "manual") chatSortMode.set("manual");
          await reorderChats(next);
        }
      } else {
        expanded[groupId] = true;
        expanded = { ...expanded };
        await moveChatToProject(from, groupId);
      }
      return;
    }
    if (dragged.project_id) {
      await moveChatToProject(from, null);
      return;
    }
    const next = spliceMove(listChats.map((c) => c.id), from, targetId);
    if (next) {
      if ($chatSortMode !== "manual") chatSortMode.set("manual");
      await reorderChats(next);
    }
  }

  async function dropOnProjectHead(e: DragEvent, projectId: string) {
    e.preventDefault();
    const from = dragId;
    clearDragState();
    if (!from) return;
    if ($chats.some((c) => c.id === from)) {
      const dragged = $chats.find((c) => c.id === from);
      if (dragged?.project_id === projectId) return;
      expanded[projectId] = true;
      expanded = { ...expanded };
      await moveChatToProject(from, projectId);
      return;
    }
    if (from === projectId) return;
    const next = spliceMove(listProjects.map((p) => p.id), from, projectId);
    if (next) {
      if ($projectSortMode !== "manual") projectSortMode.set("manual");
      await reorderProjects(next);
    }
  }

  function onSearchFocus() {
    if (ftsResults.length) ftsOpen = true;
  }

  onMount(() => {
    // Command palette opens a project: auto-expand it in the tree.
    const onExpandProject = (e: Event) => {
      const id = (e as CustomEvent<string>).detail;
      expanded[id] = true;
      expanded = { ...expanded };
    };
    window.addEventListener(EXPAND_PROJECT_EVENT, onExpandProject);
    // Keep sidebar within 50% of the window when the window is resized.
    const onWinResize = () => setSidebarWidth($sidebarWidth);
    window.addEventListener("resize", onWinResize);
    return () => {
      window.removeEventListener(EXPAND_PROJECT_EVENT, onExpandProject);
      window.removeEventListener("resize", onWinResize);
    };
  });

  function closeSearch() {
    ftsOpen = false;
  }

  function openHit(hit: FtsHit) {
    void openChat(hit.chat_id);
    ftsOpen = false;
    query = "";
  }
</script>

<aside
  bind:this={asideEl}
  class="relative flex h-full shrink-0 flex-col border-r border-border bg-sidebar"
  style="width:{$sidebarWidth}px"
>
  <div class="flex items-center gap-2 px-3 py-2">
    <button class="sidebar-btn-primary flex-1" onclick={() => newChat()}>
      <Plus size={16} />
      <span>{m.sidebar_new_chat()}</span>
    </button>
    <button class="sidebar-btn-icon" title={m.sidebar_new_project()} onclick={onCreateProject}>
      <FolderPlus size={16} />
    </button>
  </div>

  <div class="px-3 pb-2">
    <div class="sidebar-search">
      <Search size={14} />
      <input
        type="text"
        bind:value={query}
        onfocus={onSearchFocus}
        placeholder={m.sidebar_search()}
        spellcheck="false"
      />
      {#if ftsOpen && (ftsResults.length || query.trim())}
        <button class="search-close" onclick={closeSearch}>×</button>
      {/if}
    </div>
    {#if ftsOpen}
      <div class="fts-dropdown">
        <div class="fts-head">{m.search_results()}</div>
        {#if ftsResults.length === 0}
          <div class="fts-empty">{m.search_no_results()}</div>
        {:else}
          {#each ftsResults as hit (hit.message_id)}
            <button class="fts-row" onclick={() => openHit(hit)}>
              <span class="fts-snippet">{hit.snippet}</span>
              <span class="fts-meta">{$chats.find((c) => c.id === hit.chat_id)?.title ?? hit.chat_id}</span>
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  </div>

  <div class="flex-1 overflow-y-auto px-2 py-1">
    {#if $projects.length > 0}
      <div class="section-head px-1 pt-3 pb-1">
        <span class="label">{m.sidebar_projects()}</span>
        <Select
          class="sort-select"
          value={$projectSortMode}
          items={sortItems}
          onchange={setProjectSort}
          title={m.sidebar_sort_label()}
        />
      </div>
      {#each listProjects as p (p.id)}
        <div class="project-row">
          <div
            class="project-head"
            class:drag-over={dragOverProjectId === p.id}
            draggable="true"
            role="button"
            tabindex="0"
            ondragstart={(e) => onDragStart(e, p.id)}
            ondragend={clearDragState}
            ondragover={(e) => onDragOverProjectHead(e, p.id)}
            ondrop={(e) => void dropOnProjectHead(e, p.id)}
            oncontextmenu={(e) => openCtx(e, projectMenuItems(p))}
          >
            <button
              class="chev"
              onclick={(e) => { e.stopPropagation(); toggleProject(p.id); }}
              aria-label="expand"
            >
              <ChevronRight size={12} class={expanded[p.id] ? "rot" : ""} />
            </button>
            <button
              class="pmain"
              onclick={() => openProjectBoard(p.id)}
              onkeydown={(e) => e.key === "Enter" && openProjectBoard(p.id)}
            >
              <span class="pcolor" style="background:{p.color || '#6366f1'}"></span>
              <span class="ptitle">{p.name}</span>
            </button>
            <Star size={13} class={p.pinned ? "star filled" : "star"} title={p.pinned ? m.ctx_unpin() : m.ctx_pin()} />
            <button
              class="padd"
              title={m.ctx_new_chat_in_project()}
              onclick={(e) => { e.stopPropagation(); onNewProjectChat(p.id); }}
            >
              <Plus size={12} />
            </button>
            <button
              class="more"
              onmousedown={(e) => e.stopPropagation()}
              onclick={(e) => openCtx(e, projectMenuItems(p))}
            >
              <MoreVertical size={13} />
            </button>
          </div>
          {#if expanded[p.id]}
            <div class="project-chats">
              {#each sortChats($projectChats[p.id] ?? [], $chatSortMode) as c (c.id)}
                <div
                  class="chat-row sub"
                  class:active={$currentChatId === c.id}
                  class:drag-over={dragOverId === c.id}
                  draggable="true"
                  role="button"
                  tabindex="0"
                  onclick={() => openChat(c.id)}
                  onkeydown={(e) => e.key === "Enter" && openChat(c.id)}
                  ondragstart={(e) => onDragStart(e, c.id)}
                  ondragend={clearDragState}
                  ondragover={(e) => onDragOverChatRow(e, c.id)}
                  ondrop={(e) => void dropOnChatRow(e, c.id, p.id)}
                  oncontextmenu={(e) => openCtx(e, chatMenuItems(c))}
                >
                  <span class="dot {statusClass($statusByChat[c.id])}"></span>
                  <span class="title">{c.title}</span>
                  <Star size={13} class={c.pinned ? "star filled" : "star"} title={c.pinned ? m.ctx_unpin() : m.ctx_pin()} />
                  <button
                    class="more"
                    onmousedown={(e) => e.stopPropagation()}
                    onclick={(e) => openCtx(e, chatMenuItems(c))}
                  >
                    <MoreVertical size={13} />
                  </button>
                </div>
              {/each}
              {#if ($projectChats[p.id] ?? []).length === 0}
                <div class="sub-empty">{m.sidebar_no_chats()}</div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    {/if}

    <div class="section-head px-3 pt-1 pb-1">
      <span class="label">{m.sidebar_chats()}</span>
      <Select
        class="sort-select"
        value={$chatSortMode}
        items={sortItems}
        onchange={setChatSort}
        title={m.sidebar_sort_label()}
      />
    </div>

    {#each listChats as c (c.id)}
      <div
        class="chat-row"
        class:active={$currentChatId === c.id}
        class:drag-over={dragOverId === c.id}
        draggable="true"
        onclick={() => openChat(c.id)}
        role="button"
        tabindex="0"
        onkeydown={(e) => e.key === "Enter" && openChat(c.id)}
        ondragstart={(e) => onDragStart(e, c.id)}
        ondragend={clearDragState}
        ondragover={(e) => onDragOverChatRow(e, c.id)}
        ondrop={(e) => void dropOnChatRow(e, c.id, null)}
        oncontextmenu={(e) => openCtx(e, chatMenuItems(c))}
      >
        <span class="dot {statusClass($statusByChat[c.id])}"></span>
        <span class="title">{c.title}</span>
        <Star size={13} class={c.pinned ? "star filled" : "star"} title={c.pinned ? m.ctx_unpin() : m.ctx_pin()} />
        <button
          class="more"
          onmousedown={(e) => e.stopPropagation()}
          onclick={(e) => openCtx(e, chatMenuItems(c))}
        >
          <MoreVertical size={13} />
        </button>
      </div>
    {/each}

    {#if standaloneChats.length === 0}
      <div class="empty">
        <MessageSquare size={22} />
        <span>{m.sidebar_no_chats()}</span>
      </div>
    {/if}
  </div>

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

  {#if ctxMenu}
    <ContextMenu x={ctxMenu.x} y={ctxMenu.y} items={ctxMenu.items} onclose={() => (ctxMenu = null)} />
  {/if}

  <RenameModal
    open={renameTarget !== null}
    title={m.rename_title()}
    initial={renameTarget?.name ?? ""}
    onsave={onRenameSave}
    oncancel={() => (renameTarget = null)}
  />

  <RenameModal
    open={createProjectOpen}
    title={m.sidebar_new_project()}
    initial=""
    onsave={onCreateProjectSave}
    oncancel={() => (createProjectOpen = false)}
  />
</aside>

<style>
  .sidebar-btn-primary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.375rem;
    height: 2rem;
    border-radius: var(--radius-md);
    background-color: var(--primary);
    color: var(--primary-foreground);
    font-size: 0.8125rem;
    font-weight: 500;
    border: none;
    cursor: default;
  }
  .sidebar-btn-primary:hover {
    opacity: 0.9;
  }
  .sidebar-btn-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 2rem;
    width: 2rem;
    border-radius: var(--radius-md);
    background: var(--secondary);
    color: var(--secondary-foreground);
    border: none;
    cursor: default;
  }
  .sidebar-btn-icon:hover {
    opacity: 0.85;
  }
  .sidebar-search {
    position: relative;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    height: 2rem;
    padding: 0 0.625rem;
    border-radius: var(--radius-md);
    background-color: var(--muted);
    color: var(--muted-foreground);
  }
  .sidebar-search input {
    background: transparent;
    border: none;
    outline: none;
    width: 100%;
    font-size: 0.8125rem;
    color: var(--foreground);
  }
  .search-close {
    background: transparent;
    border: none;
    color: var(--muted-foreground);
    font-size: 1rem;
    line-height: 1;
    cursor: default;
    padding: 0;
  }
  .fts-dropdown {
    position: relative;
    margin-top: 0.25rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--popover);
    color: var(--popover-foreground);
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.18);
    max-height: 280px;
    overflow-y: auto;
    z-index: 40;
  }
  .fts-head {
    padding: 0.3rem 0.5rem;
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted-foreground);
  }
  .fts-empty {
    padding: 0.4rem 0.5rem;
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }
  .fts-row {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    width: 100%;
    text-align: left;
    padding: 0.3rem 0.5rem;
    border: none;
    background: transparent;
    border-top: 1px solid var(--border);
    cursor: default;
  }
  .fts-row:hover {
    background: var(--accent);
  }
  .fts-snippet {
    font-size: 0.75rem;
    color: var(--foreground);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fts-meta {
    font-size: 0.68rem;
    color: var(--muted-foreground);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .section-head .label {
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--muted-foreground);
  }
  .section-head :global(.sort-select) {
    --sel-height: 1.4rem;
    --sel-max-width: 8rem;
    --sel-font-size: 0.66rem;
    --sel-radius: var(--radius-sm);
    --sel-background: var(--secondary);
    --sel-color: var(--muted-foreground);
    --sel-hover-background: var(--accent);
    --sel-hover-color: var(--foreground);
  }
  .chat-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.375rem 0.5rem;
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--sidebar-foreground);
    font-size: 0.8125rem;
    text-align: left;
    cursor: default;
    user-select: none;
  }
  .chat-row.sub {
    padding-left: 1.5rem;
  }
  .chat-row:hover {
    background-color: var(--sidebar-accent);
  }
  .chat-row.active {
    background-color: var(--sidebar-accent);
    color: var(--sidebar-accent-foreground);
  }
  .chat-row.drag-over {
    box-shadow: inset 0 0 0 1px var(--sidebar-ring);
  }
  .chat-row .title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chat-row :global(svg.star) {
    display: none;
    flex-shrink: 0;
    color: var(--muted-foreground);
    opacity: 0.5;
  }
  .chat-row:hover :global(svg.star),
  .chat-row :global(svg.star.filled) {
    display: block;
  }
  .chat-row :global(svg.star.filled) {
    color: hsl(38 92% 50%);
    opacity: 1;
    fill: currentColor;
  }
  .more {
    display: none;
    align-items: center;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    padding: 0;
    border-radius: var(--radius-sm);
  }
  .chat-row:hover .more,
  .project-head:hover .more {
    display: inline-flex;
  }
  .more:hover {
    color: var(--foreground);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 9999px;
    flex-shrink: 0;
    background: var(--muted-foreground);
    opacity: 0.5;
  }
  .dot.running {
    background: hsl(217 91% 60%);
    opacity: 1;
    animation: pulse 1.2s ease-in-out infinite;
  }
  .dot.error {
    background: var(--destructive);
    opacity: 1;
  }
  .dot.cancelled {
    background: var(--muted-foreground);
    opacity: 0.7;
  }
  @keyframes pulse {
    50% {
      opacity: 0.3;
    }
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
    padding: 2.5rem 1rem;
    color: var(--muted-foreground);
    font-size: 0.8125rem;
    text-align: center;
  }
  .project-row {
    margin-top: 0.2rem;
  }
  .project-head {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    padding: 0.3rem 0.4rem;
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--sidebar-foreground);
    font-size: 0.8125rem;
    text-align: left;
    cursor: default;
    user-select: none;
  }
  .project-head:hover {
    background-color: var(--sidebar-accent);
  }
  .project-head.drag-over {
    box-shadow: inset 0 0 0 1px var(--sidebar-ring);
    background-color: var(--sidebar-accent);
  }
  .project-head :global(.rot) {
    transform: rotate(90deg);
  }
  .project-head :global(svg.star) {
    display: none;
    flex-shrink: 0;
    color: var(--muted-foreground);
    opacity: 0.5;
  }
  .project-head:hover :global(svg.star),
  .project-head :global(svg.star.filled) {
    display: block;
  }
  .project-head :global(svg.star.filled) {
    color: hsl(38 92% 50%);
    opacity: 1;
    fill: currentColor;
  }
  .pcolor {
    width: 9px;
    height: 9px;
    border-radius: 9999px;
    flex-shrink: 0;
  }
  .ptitle {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .chev {
    display: inline-flex;
    align-items: center;
    flex-shrink: 0;
    background: transparent;
    border: none;
    color: var(--muted-foreground);
    padding: 0;
    cursor: default;
  }
  .pmain {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    flex: 1;
    min-width: 0;
    background: transparent;
    border: none;
    padding: 0;
    cursor: default;
    text-align: left;
    color: inherit;
    font: inherit;
  }
  .padd {
    display: none;
    align-items: center;
    background: transparent;
    border: none;
    color: var(--muted-foreground);
    padding: 0;
    cursor: default;
  }
  .project-head:hover .padd {
    display: inline-flex;
  }
  .padd:hover {
    color: var(--primary);
  }
  .sub-empty {
    padding: 0.4rem 1.5rem 0.4rem 2rem;
    font-size: 0.75rem;
    color: var(--muted-foreground);
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
</style>