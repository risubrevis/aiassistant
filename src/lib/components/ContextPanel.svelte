<script lang="ts">
  import { onMount } from "svelte";
  import { Folder, FilePlus, FileEdit, FileX, FileText, Trash2, Settings2, Plus, CornerDownLeft, Sparkles } from "@lucide/svelte";
  import * as ipc from "$lib/tauri";
  import type { ProjectContextSummary, ProjectPath } from "$lib/tauri";
  import {
    fileChanges,
    changedFileViews,
    loadChangedFileViews,
    insertFileRef,
    insertAllFileRefs,
    openProjectSettings,
  } from "$lib/stores/project";
  import { m } from "$lib/i18n";

  let { projectId, projectName }: { projectId: string; projectName: string } = $props();

  let paths = $state<ProjectPath[]>([]);
  let views = $derived($changedFileViews[projectId] ?? []);
  let expanded = $state<Record<string, boolean>>({});

  async function loadPaths() {
    try {
      paths = await ipc.projectPathsList(projectId);
    } catch (e) {
      console.error("projectPathsList failed", e);
    }
  }

  let ctxSummary = $state<ProjectContextSummary | null>(null);
  async function loadCtx() {
    try {
      ctxSummary = await ipc.projectContextSummary(projectId);
    } catch (e) {
      console.error("projectContextSummary failed", e);
      ctxSummary = null;
    }
  }

  onMount(() => {
    void loadPaths();
    void loadCtx();
  });

  $effect(() => {
    void projectId;
    void loadPaths();
    void loadCtx();
  });

  // Reload views on mount, on project switch and whenever a new file-changed
  // event lands in `fileChanges` (reads its length to track it without loops:
  // the reload writes to `changedFileViews`, not `fileChanges`).
  $effect(() => {
    void projectId;
    void $fileChanges[projectId]?.length;
    void loadChangedFileViews(projectId);
  });

  async function addPath() {
    const path = window.prompt(m.sidebar_add_path() + " — " + m.sidebar_add_folder());
    if (!path) return;
    try {
      await ipc.projectPathAdd(projectId, path, "dir", true, null);
      await loadPaths();
    } catch (e) {
      console.error("projectPathAdd failed", e);
    }
  }

  async function removePath(p: ProjectPath) {
    try {
      await ipc.projectPathDelete(p.id, projectId);
      await loadPaths();
    } catch (e) {
      console.error("projectPathDelete failed", e);
    }
  }

  function changeIcon(kind: string) {
    if (kind === "created") return FilePlus;
    if (kind === "deleted") return FileX;
    return FileEdit;
  }

  function toggleView(rel: string) {
    expanded = { ...expanded, [rel]: !expanded[rel] };
  }
</script>

<aside class="ctx">
  <div class="ctx-head">
    <span class="ctx-title">{m.context_panel_title()}</span>
    <button class="icon-btn" title={m.project_settings()} onclick={() => openProjectSettings(projectId)}>
      <Settings2 size={14} />
    </button>
  </div>

  <div class="ctx-name">{projectName}</div>

  <div class="ctx-section">
    <div class="ctx-label">
      <span>{m.project_paths()}</span>
      <button class="icon-btn" title={m.sidebar_add_folder()} onclick={addPath}>
        <Plus size={13} />
      </button>
    </div>
    {#if paths.length === 0}
      <div class="ctx-empty">{m.context_no_paths()}</div>
    {:else}
      {#each paths as p (p.id)}
        <div class="path-row">
          <Folder size={12} />
          <span class="path" title={p.path}>{p.path}</span>
          <button class="icon-btn del" title={m.common_delete()} onclick={() => removePath(p)}>
            <Trash2 size={12} />
          </button>
        </div>
      {/each}
    {/if}
  </div>

  <div class="ctx-section">
    <div class="ctx-label"><span>{m.context_auto_title()}</span></div>
    {#if !ctxSummary || (ctxSummary.rule_files.length === 0 && ctxSummary.skills.length === 0)}
      <div class="ctx-empty">{m.context_no_auto()}</div>
      <div class="ctx-hint">{m.context_auto_hint()}</div>
    {:else}
      {#if ctxSummary.rule_files.length > 0}
        <div class="ctx-sub">{m.context_rule_files()}</div>
        {#each ctxSummary.rule_files as f}
          <div class="ctx-row"><FileText size={12} /><span class="ctx-name" title={f}>{f}</span></div>
        {/each}
      {/if}
      {#if ctxSummary.skills.length > 0}
        <div class="ctx-sub">{m.context_skills()}</div>
        {#each ctxSummary.skills as s (s.id)}
          <div class="ctx-row" title={s.description}><Sparkles size={12} /><span class="ctx-name">{s.id}</span></div>
        {/each}
      {/if}
    {/if}
  </div>

  <div class="ctx-section">
    <div class="ctx-label">
      <span>{m.context_recent_changes()}</span>
      {#if views.length > 0}
        <button class="icon-btn" title={m.context_pull_all()} onclick={() => insertAllFileRefs(views.map((v) => v.rel))}>
          <Plus size={13} />
        </button>
      {/if}
    </div>
    {#if views.length === 0}
      <div class="ctx-empty">—</div>
    {:else}
      {#each views.slice(0, 20) as c (c.rel)}
        {@const Icon = changeIcon(c.kind)}
        <div class="change-row">
          <button class="change-main" title={c.path} onclick={() => toggleView(c.rel)}>
            <span class="ico"><Icon size={12} /></span>
            <span class="path">{c.rel}</span>
          </button>
          <button class="insert-btn" title={m.context_insert_file()} onclick={(e) => { e.stopPropagation(); insertFileRef(c.rel); }}>
            <CornerDownLeft size={11} />
          </button>
        </div>
        {#if expanded[c.rel]}
          {#if c.content}
            <pre class="change-content">{c.content}</pre>
          {:else if c.content === null}
            <div class="change-content empty">({m.context_change_binary()})</div>
          {/if}
        {/if}
      {/each}
    {/if}
  </div>
</aside>

<style>
  .ctx {
    width: 260px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--border);
    background: var(--background);
    overflow-y: auto;
  }
  .ctx-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: var(--header-height);
    padding: 0 0.625rem;
    border-bottom: 1px solid var(--border);
  }
  .ctx-title {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--muted-foreground);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .ctx-name {
    padding: 0.5rem 0.75rem;
    font-size: 0.875rem;
    font-weight: 500;
  }
  .ctx-section {
    padding: 0.5rem 0.75rem;
    border-top: 1px solid var(--border);
  }
  .ctx-label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 0.7rem;
    font-weight: 600;
    color: var(--muted-foreground);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-bottom: 0.3rem;
  }
  .ctx-empty {
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }
  .ctx-sub {
    font-size: 0.68rem;
    font-weight: 600;
    color: var(--muted-foreground);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin: 0.35rem 0 0.15rem;
  }
  .ctx-row {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.2rem 0;
    font-size: 0.72rem;
    color: var(--foreground);
  }
  .ctx-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
  }
  .ctx-hint {
    font-size: 0.68rem;
    color: var(--muted-foreground);
    margin-top: 0.2rem;
    line-height: 1.3;
  }
  .path-row {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.2rem 0;
    font-size: 0.72rem;
    color: var(--foreground);
  }
  .change-row {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.2rem 0;
    font-size: 0.72rem;
    color: var(--foreground);
  }
  .change-row:hover {
    background: var(--accent);
    border-radius: var(--radius-sm);
  }
  .change-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.1rem 0.15rem;
    background: transparent;
    border: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: default;
  }
  .insert-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted-foreground);
    border-radius: var(--radius-sm);
    cursor: default;
    padding: 0.1rem;
    flex-shrink: 0;
  }
  .insert-btn:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .change-content {
    margin: 0.15rem 0 0.15rem 1.6rem;
    padding: 0.35rem 0.45rem;
    background: var(--muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 0.68rem;
    max-height: 180px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--foreground);
  }
  .change-content.empty {
    color: var(--muted-foreground);
  }
  .path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
  }
  .ico {
    display: inline-flex;
    color: var(--muted-foreground);
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted-foreground);
    border-radius: var(--radius-sm);
    cursor: default;
    padding: 0.15rem;
  }
  .icon-btn:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .del:hover {
    color: var(--destructive);
  }
</style>