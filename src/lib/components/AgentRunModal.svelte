<script lang="ts">
  import { tick } from "svelte";
  import {
    Bot,
    X,
    Loader,
    Check,
    AlertTriangle,
    Clock,
    GitMerge,
    Circle,
    Wrench,
    FileDiff,
    Plus,
    Minus,
    RefreshCw,
    Trash2,
    ListTodo,
  } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import {
    agentRuns,
    eventsByRun,
    agentModalRunId,
    runDiffs,
    runDiffLoading,
    cancelRun,
    approveRun,
    rejectRun,
    loadRunDiff,
  } from "$lib/stores/agents";
  import { tasksByChat } from "$lib/stores/tasks";
  import TaskRow from "./TaskRow.svelte";

  let bodyEl: HTMLDivElement | undefined = $state();
  let view: "activity" | "changes" = $state("activity");
  let expanded: Record<string, boolean> = $state({});
  let diffRequested: Record<string, boolean> = $state({});
  let busy = $state<"approve" | "reject" | null>(null);

  async function handleApprove() {
    if (busy || !runId) return;
    busy = "approve";
    try { await approveRun(runId); } catch (e) { console.error(e); }
    busy = null;
  }

  async function handleReject() {
    if (busy || !runId) return;
    busy = "reject";
    try { await rejectRun(runId); } catch (e) { console.error(e); }
    busy = null;
  }

  let runId = $derived($agentModalRunId);
  let run = $derived(runId ? ($agentRuns.find((r) => r.id === runId) ?? null) : null);
  let events = $derived(runId ? ($eventsByRun[runId] ?? []) : []);
  let diff = $derived(runId ? ($runDiffs[runId] ?? null) : null);
  let diffLoading = $derived(runId ? ($runDiffLoading[runId] ?? false) : false);
  let runTasks = $derived(
    run ? ($tasksByChat[run.chat_id] ?? []).filter((t) => t.source === "agent" && t.run_id === runId) : [],
  );

  function statusIcon(status: string) {
    switch (status) {
      case "running": return Loader;
      case "done": return Check;
      case "error": return AlertTriangle;
      case "timeout": return Clock;
      case "cancelled": return X;
      case "merged": return GitMerge;
      default: return Clock;
    }
  }

  function statusLabel(status: string): string {
    switch (status) {
      case "running": return m.agents_status_running();
      case "done": return m.agents_status_done();
      case "error": return m.agents_status_error();
      case "timeout": return m.agents_status_timeout();
      case "cancelled": return m.agents_status_cancelled();
      case "merged": return m.agents_status_merged();
      case "queued": return m.agents_status_queued();
      default: return status;
    }
  }

  function fileIcon(status: string) {
    switch (status) {
      case "added": return Plus;
      case "deleted": return Trash2;
      default: return FileDiff;
    }
  }

  function fileClass(status: string): string {
    return status === "added" ? "add" : status === "deleted" ? "del" : "mod";
  }

  function fileStatusLabel(status: string): string {
    switch (status) {
      case "added": return m.agents_file_added();
      case "modified": return m.agents_file_modified();
      case "deleted": return m.agents_file_deleted();
      case "renamed": return m.agents_file_renamed();
      default: return status;
    }
  }

  function lineClass(line: string): string {
    if (line.startsWith("+++") || line.startsWith("---")) return "meta";
    if (line.startsWith("@@")) return "hunk";
    if (line.startsWith("+")) return "add";
    if (line.startsWith("-")) return "del";
    return "ctx";
  }

  function toggleFile(path: string) {
    expanded[path] = !expanded[path];
  }

  function close() {
    agentModalRunId.set(null);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }

  $effect(() => {
    void runId;
    view = "activity";
    expanded = {};
  });

  // Keep the event feed scrolled to the latest entry.
  $effect(() => {
    void events.length;
    void tick().then(() => {
      if (bodyEl) bodyEl.scrollTop = bodyEl.scrollHeight;
    });
  });

  $effect(() => {
    if (!runId || view !== "changes") return;
    if (diff || diffLoading || diffRequested[runId]) return;
    diffRequested[runId] = true;
    void loadRunDiff(runId);
  });
</script>

<svelte:window onkeydown={onKeydown} />

{#if runId}
  {@const rid = runId}
  {@const SIcon = run ? statusIcon(run.status) : Clock}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div class="dialog" role="dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onKeydown}>
      <header class="head">
        <Bot size={15} />
        <span class="name">{run?.agent_connection_id ?? rid}</span>
        <span class="st {run?.status ?? "queued"}"><SIcon size={12} /> {run ? statusLabel(run.status) : ""}</span>
        <span class="task">{run?.subtask_prompt ?? ""}</span>
        <button class="x" title={m.common_close()} onclick={close}><X size={15} /></button>
      </header>
      <div class="tabs">
        <button class="tab" class:active={view === "activity"} onclick={() => (view = "activity")}>
          {m.agents_tab_activity()}
        </button>
        {#if run?.worktree_branch}
          <button class="tab" class:active={view === "changes"} onclick={() => (view = "changes")}>
            {m.agents_tab_changes()}
            {#if (diff?.files.length ?? 0) > 0}
              <span class="badge">{diff?.files.length}</span>
            {/if}
          </button>
        {/if}
      </div>
      {#if view === "activity" || !run?.worktree_branch}
        <div class="body" bind:this={bodyEl}>
          {#if runTasks.length > 0}
            <section class="run-tasks">
              <div class="rt-head"><ListTodo size={12} /> {m.tasks_title()}</div>
              <div class="rt-rows">
                {#each runTasks as t (t.id)}
                  <TaskRow task={t} />
                {/each}
              </div>
            </section>
          {/if}
          {#each events as ev, i (i)}
            {#if ev.kind === "text"}
              <div class="ev-text">{ev.text}</div>
            {:else if ev.kind === "tool_action"}
              <div class="ev-card">
                <div class="card-name"><Wrench size={12} /> {ev.name}</div>
                {#if ev.args}
                  <pre>{ev.args}</pre>
                {/if}
              </div>
            {:else if ev.kind === "diff"}
              <div class="ev-card">
                <div class="card-name"><FileDiff size={12} /> {ev.path}</div>
                <pre class="diff">{ev.patch}</pre>
              </div>
            {:else if ev.kind === "progress"}
              <div class="ev-progress">{ev.text}</div>
            {:else if ev.kind === "error"}
              <div class="ev-error">{ev.text}</div>
            {/if}
          {/each}
          {#if events.length === 0}
            <div class="empty">
              {run?.status === "done" || run?.status === "error"
                ? "—"
                : m.agents_working()}
            </div>
          {/if}
          {#if run?.result_summary}
            <div class="summary">{run.result_summary}</div>
          {/if}
        </div>
      {:else}
        <div class="changes">
          <div class="cbar">
            {#if diff}
              <span class="dnum add"><Plus size={11} /> {diff.total_additions}</span>
              <span class="dnum del"><Minus size={11} /> {diff.total_deletions}</span>
            {/if}
            <span class="dspacer"></span>
            <button class="btn" disabled={diffLoading} onclick={() => void loadRunDiff(rid)}>
              <span class="ricon" class:loading={diffLoading}><RefreshCw size={12} /></span>
              {m.agents_diff_refresh()}
            </button>
          </div>
          {#if !diff && diffLoading}
            <div class="dload"><Loader size={13} /> {m.agents_diff_loading()}</div>
          {:else if diff}
            {#if diff.stat}
              <pre class="dstat">{diff.stat}</pre>
            {/if}
            {#if diff.files.length === 0}
              <div class="empty">{m.agents_no_changes()}</div>
            {:else}
              <div class="files">
                {#each diff.files as f (f.path)}
                  {@const FIcon = fileIcon(f.status)}
                  <div class="file">
                    <div class="frow" role="button" tabindex="0"
                      onclick={() => toggleFile(f.path)}
                      onkeydown={(e) => e.key === "Enter" && toggleFile(f.path)}>
                      <span class="fico {fileClass(f.status)}"><FIcon size={12} /></span>
                      <span class="fpath" title={f.path}>{f.path}</span>
                      <span class="fst">{fileStatusLabel(f.status)}</span>
                      <span class="fnum add">+{f.additions}</span>
                      <span class="fnum del">-{f.deletions}</span>
                    </div>
                    {#if expanded[f.path]}
                      <pre class="fdiff">{#each f.patch.split("\n") as line, li (li)}<div class="dl {lineClass(line)}">{line}</div>{/each}</pre>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
          {/if}
        </div>
      {/if}
      <footer class="foot">
        {#if run && (run.status === "queued" || run.status === "running")}
          <button class="btn" onclick={() => void cancelRun(rid)}>
            <X size={12} /> {m.agents_cancel()}
          </button>
        {:else if run && run.status === "done" && run.worktree_branch}
          {#if busy}
            <span class="btn busy"><Loader size={12} class="spin" /> {busy === "approve" ? "Approving…" : "Rejecting…"}</span>
          {:else}
            <button class="btn ok" onclick={handleApprove}>{m.agents_approve()}</button>
            <button class="btn no" onclick={handleReject}>{m.agents_reject()}</button>
          {/if}
        {/if}
        <span class="spacer"></span>
        <button class="btn" onclick={close}>{m.common_close()}</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: rgb(0 0 0 / 0.4);
  }
  .dialog {
    width: 680px;
    max-width: 92vw;
    height: 520px;
    max-height: 82vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
    min-width: 0;
  }
  .name { font-size: 0.8125rem; font-weight: 600; flex-shrink: 0; }
  .st {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.6875rem;
    color: var(--muted-foreground);
    flex-shrink: 0;
  }
  .st.running { color: hsl(217 91% 60%); }
  .st.running :global(svg) { animation: spin 1s linear infinite; }
  .st.done { color: hsl(140 60% 40%); }
  .st.error { color: var(--destructive); }
  .st.timeout { color: hsl(38 92% 50%); }
  .st.merged { color: hsl(260 70% 60%); }
  .task {
    flex: 1;
    font-size: 0.75rem;
    color: var(--muted-foreground);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .x { display: inline-flex; align-items: center; justify-content: center; color: var(--muted-foreground); background: transparent; border: none; border-radius: var(--radius-sm); cursor: default; padding: 0.15rem; }
  .x:hover { background-color: var(--accent); color: var(--accent-foreground); }
  .tabs {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.3rem 0.875rem 0;
    border-bottom: 1px solid var(--border);
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.35rem 0.6rem;
    font-size: 0.75rem;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    cursor: default;
  }
  .tab:hover { color: var(--foreground); background: var(--accent); }
  .tab.active { color: var(--foreground); border-bottom-color: hsl(217 91% 60%); }
  .badge {
    min-width: 1rem;
    padding: 0 0.3rem;
    text-align: center;
    font-size: 0.62rem;
    font-weight: 600;
    border-radius: 9999px;
    background: hsl(217 91% 60%);
    color: white;
  }
  .body { flex: 1; overflow-y: auto; padding: 0.75rem 0.875rem; display: flex; flex-direction: column; gap: 0.4rem; }
  .run-tasks { border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--muted); padding: 0.4rem 0.5rem; display: flex; flex-direction: column; gap: 0.2rem; }
  .rt-head { display: flex; align-items: center; gap: 0.3rem; font-size: 0.72rem; font-weight: 600; }
  .rt-head :global(svg) { color: var(--muted-foreground); }
  .rt-rows { display: flex; flex-direction: column; }
  .ev-text { font-size: 0.8125rem; white-space: pre-wrap; color: var(--foreground); }
  .ev-card { border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--muted); overflow: hidden; }
  .card-name { display: flex; align-items: center; gap: 0.3rem; font-size: 0.72rem; font-weight: 500; padding: 0.3rem 0.45rem; }
  .card-name :global(svg) { color: var(--muted-foreground); }
  .ev-card pre, .diff { margin: 0; padding: 0.45rem; background: var(--background); border-top: 1px solid var(--border); font-family: var(--font-mono); font-size: 0.7rem; white-space: pre-wrap; max-height: 220px; overflow-y: auto; }
  .ev-progress { font-size: 0.72rem; color: var(--muted-foreground); font-style: italic; }
  .ev-error { font-size: 0.78rem; color: var(--destructive); white-space: pre-wrap; }
  .empty { color: var(--muted-foreground); font-size: 0.78rem; padding: 0.5rem 0; }
  .summary {
    margin-top: 0.25rem;
    padding: 0.45rem 0.55rem;
    border: 1px solid var(--border);
    border-left: 3px solid hsl(140 60% 40%);
    border-radius: var(--radius-sm);
    font-size: 0.78rem;
    white-space: pre-wrap;
    background: var(--muted);
  }
  .changes { flex: 1; min-height: 0; overflow-y: auto; padding: 0.6rem 0.875rem; display: flex; flex-direction: column; gap: 0.45rem; }
  .cbar { display: flex; align-items: center; gap: 0.6rem; }
  .dnum { display: inline-flex; align-items: center; gap: 0.2rem; font-size: 0.78rem; font-family: var(--font-mono); }
  .dnum.add { color: hsl(140 60% 40%); }
  .dnum.del { color: var(--destructive); }
  .dspacer { flex: 1; }
  .btn { display: inline-flex; align-items: center; gap: 0.3rem; padding: 0.3rem 0.7rem; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--background); color: var(--foreground); font-size: 0.78rem; cursor: default; }
  .btn:hover { background: var(--accent); }
  .btn:disabled { opacity: 0.55; }
  .btn.busy { color: var(--muted-foreground); background: var(--muted); border-color: var(--border); }
  :global(.spin) { animation: spin 1s linear infinite; }
  .ricon { display: inline-flex; }
  .ricon.loading :global(svg) { animation: spin 1s linear infinite; }
  .dload { display: flex; align-items: center; gap: 0.4rem; color: var(--muted-foreground); font-size: 0.75rem; padding: 0.5rem 0; }
  .dstat {
    margin: 0;
    padding: 0.45rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--muted);
    font-family: var(--font-mono);
    font-size: 0.68rem;
    white-space: pre;
    overflow-x: auto;
  }
  .files { display: flex; flex-direction: column; }
  .file { border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--background); overflow: hidden; }
  .file + .file { margin-top: 0.35rem; }
  .frow { display: flex; align-items: center; gap: 0.45rem; padding: 0.3rem 0.45rem; font-size: 0.72rem; }
  .frow:hover { background: var(--accent); }
  .fico { display: inline-flex; color: var(--muted-foreground); flex-shrink: 0; }
  .fico.add { color: hsl(140 60% 40%); }
  .fico.del { color: var(--destructive); }
  .fpath { flex: 1; font-family: var(--font-mono); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fst { font-size: 0.65rem; color: var(--muted-foreground); flex-shrink: 0; }
  .fnum { font-family: var(--font-mono); font-size: 0.68rem; flex-shrink: 0; }
  .fnum.add { color: hsl(140 60% 40%); }
  .fnum.del { color: var(--destructive); }
  .fdiff {
    margin: 0;
    padding: 0.25rem 0;
    border-top: 1px solid var(--border);
    background: var(--background);
    font-family: var(--font-mono);
    font-size: 0.68rem;
    overflow-x: auto;
  }
  .dl { white-space: pre; padding: 0 0.45rem; min-height: 1.05em; line-height: 1.35; }
  .dl.add { background: hsl(140 60% 45% / 0.16); }
  .dl.del { background: hsl(0 72% 51% / 0.14); }
  .dl.hunk { color: var(--muted-foreground); background: var(--muted); }
  .dl.meta { color: var(--muted-foreground); }
  .foot { display: flex; align-items: center; gap: 0.5rem; padding: 0.55rem 0.875rem; border-top: 1px solid var(--border); }
  .spacer { flex: 1; }
  .btn.ok { background: hsl(140 60% 40%); border-color: hsl(140 60% 40%); color: white; }
  .btn.ok:hover { background: hsl(140 60% 34%); }
  .btn.no { background: var(--destructive); border-color: var(--destructive); color: var(--destructive-foreground); }
  .btn.no:hover { background: var(--destructive); filter: brightness(0.92); }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>